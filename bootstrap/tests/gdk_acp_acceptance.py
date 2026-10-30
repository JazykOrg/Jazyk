"""Exercise the bundled GDK ACP agent with a real SDK-connected MCP child.

Uses a deterministic local HTTP provider. No model capability verdict is claimed.
"""
import argparse
import http.server
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time


MCP = r'''import json,sys
for line in sys.stdin:
    request=json.loads(line)
    if 'id' not in request: continue
    method=request['method']
    if method=='initialize':
        result={'protocolVersion':request['params']['protocolVersion'],'serverInfo':{'name':'fixture','version':'1'},'capabilities':{'tools':{}},'instructions':'Use fixture to read the test value.'}
    elif method=='tools/list':
        result={'tools':[{'name':'fixture','description':'Read test value','inputSchema':{'type':'object','properties':{'value':{'type':'integer'}},'required':['value']}}]}
    elif method=='tools/call':
        assert request['params']['name']=='fixture'
        assert request['params']['arguments']=={'value':7}
        with open(sys.argv[1],'a') as log:log.write(json.dumps(request['params'])+'\n')
        result={'content':[{'type':'text','text':'fixture complete'}],'structuredContent':{'value':7},'_meta':{'source':'fixture'},'isError':False}
    else:result={}
    print(json.dumps({'jsonrpc':'2.0','id':request['id'],'result':result}),flush=True)
open(sys.argv[2],'w').write('closed on EOF')
'''


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--binary', required=True)
    arguments = parser.parse_args()
    requests = []
    cancelled_request = threading.Event()
    release_request = threading.Event()

    class Provider(http.server.BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def reply(self, value):
            data = json.dumps(value).encode()
            try:
                self.send_response(200)
                self.send_header('Content-Type', 'application/json')
                self.send_header('Content-Length', str(len(data)))
                self.end_headers()
                self.wfile.write(data)
            except (BrokenPipeError, ConnectionResetError):
                pass

        def do_GET(self):
            self.reply({'data': [{'id': 'fixture'}]})

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            assert body['tools'][0]['function']['name'] == 'fixture', body
            messages = body['messages']
            last = messages[-1]
            if last.get('role') == 'user' and 'cancel this turn' in last.get('content', ''):
                cancelled_request.set()
                release_request.wait(10)
                message = {'role': 'assistant', 'content': 'late result'}
                finish = 'stop'
            elif last.get('role') == 'tool':
                assert last['content'] == 'fixture complete', last
                message = {'role': 'assistant', 'content': 'Finished using the fixture.'}
                finish = 'stop'
            else:
                message = {'role': 'assistant', 'content': '', 'tool_calls': [
                    {'id': 'provider-call', 'type': 'function', 'function': {
                        'name': 'fixture', 'arguments': json.dumps({'value': 7})}}
                ]}
                finish = 'tool_calls'
            self.reply({'choices': [{'message': message, 'finish_reason': finish}],
                        'usage': {'prompt_tokens': 10, 'completion_tokens': 3, 'total_tokens': 13}})

    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    try:
        with tempfile.TemporaryDirectory(prefix='jazyk-gdk-acp-') as directory:
            root = Path(directory)
            script = root / 'mcp.py'
            script.write_text(MCP)
            calls = root / 'calls.jsonl'
            shutdown = root / 'shutdown.txt'
            environment = os.environ.copy()
            environment.update({'JAZYK_LLM_BASE_URL': f'http://127.0.0.1:{server.server_port}/v1',
                                'JAZYK_MODEL': 'fixture', 'JAZYK_API_KEY': '', 'JAZYK_ACP_IDLE_TIMEOUT': '5'})
            environment.pop('JAZYK_CODEC', None)
            process = subprocess.Popen([str(Path(arguments.binary).resolve()), 'agent'], cwd=root,
                                       env=environment, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                       stderr=subprocess.PIPE, text=True, bufsize=1)
            incoming = queue.Queue()

            def read():
                for line in process.stdout:
                    incoming.put(json.loads(line))
                incoming.put(None)

            threading.Thread(target=read, daemon=True).start()
            sequence = 0

            def send(method, params, notification=False):
                nonlocal sequence
                sequence += 1
                message = {'jsonrpc': '2.0', 'method': method, 'params': params}
                if not notification:
                    message['id'] = sequence
                process.stdin.write(json.dumps(message) + '\n')
                process.stdin.flush()
                return sequence

            def receive(request_id):
                updates = []
                deadline = time.monotonic() + 10
                while time.monotonic() < deadline:
                    message = incoming.get(timeout=max(0.01, deadline - time.monotonic()))
                    assert message is not None, process.stderr.read()
                    if message.get('method') == 'session/update':
                        updates.append(message['params']['update'])
                    if message.get('id') == request_id:
                        assert 'error' not in message, message
                        return message['result'], updates
                raise TimeoutError(f'ACP response {request_id}')

            try:
                receive(send('initialize', {'protocolVersion': 1, 'clientCapabilities': {},
                                           'clientInfo': {'name': 'acceptance', 'version': '1'}}))
                created, _ = receive(send('session/new', {'cwd': str(root), 'mcpServers': [{
                    'name': 'fixture', 'command': sys.executable,
                    'args': [str(script), str(calls), str(shutdown)], 'env': []}],
                    '_meta': {'jazyk': {'purpose': 'chat'}}}))
                session = created['sessionId']
                receive(send('session/set_config_option', {'sessionId': session, 'configId': 'model', 'value': 'fixture'}))
                tool_ids = []
                for text in ['Run fixture.', 'Run fixture again.']:
                    result, updates = receive(send('session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': text}]}))
                    assert result['stopReason'] == 'end_turn', (result, updates)
                    starts = [update for update in updates if update['sessionUpdate'] == 'tool_call']
                    ends = [update for update in updates if update['sessionUpdate'] == 'tool_call_update']
                    assert len(starts) == len(ends) == 1, updates
                    assert ends[0]['status'] == 'completed', ends
                    raw = ends[0]['rawOutput']
                    raw = json.loads(raw) if isinstance(raw, str) else raw
                    assert raw['structuredContent']['value'] == 7, raw
                    tool_ids.append(starts[0]['toolCallId'])
                assert len(set(tool_ids)) == 2, tool_ids
                assert len(calls.read_text().splitlines()) == 2
                prompt = send('session/prompt', {'sessionId': session, 'prompt': [{'type': 'text', 'text': 'cancel this turn'}]})
                assert cancelled_request.wait(5), requests
                started = time.monotonic()
                send('session/cancel', {'sessionId': session}, notification=True)
                result, _ = receive(prompt)
                assert result['stopReason'] == 'cancelled', result
                assert time.monotonic() - started < 2
                release_request.set()
                receive(send('session/close', {'sessionId': session}))
                assert shutdown.read_text() == 'closed on EOF'
                process.stdin.close()
                assert process.wait(timeout=10) == 0, process.stderr.read()
                print(json.dumps({'status': 'pass', 'modelRequests': len(requests),
                                  'toolCalls': 2, 'cancel': 'cancelled', 'mcpClose': 'EOF'}))
            finally:
                release_request.set()
                if process.poll() is None:
                    process.kill()
                    process.wait(timeout=5)
    finally:
        server.shutdown()
        server.server_close()


if __name__ == '__main__':
    main()
