#!/usr/bin/env python3
"""Exercise native ACP onboarding and live provider reload using a local mock model."""
import http.server
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import tempfile
import threading
import time

binary=Path(sys.argv[1]).resolve()
requests=[]
reject=False
class Model(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        requests.append(json.loads(self.rfile.read(int(self.headers['Content-Length']))))
        if reject:
            body=b'{"error":{"message":"Invalid API key","type":"authentication_error"}}'
            self.send_response(401);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body);return
        body=json.dumps({'id':'fixture','object':'chat.completion','model':'fixture',
          'choices':[{'index':0,'message':{'role':'assistant','content':'Configured endpoint works.'},'finish_reason':'stop'}],
          'usage':{'prompt_tokens':1,'completion_tokens':1,'total_tokens':2}}).encode()
        self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(body)));self.end_headers();self.wfile.write(body)
    def log_message(self,*args):pass
with tempfile.TemporaryDirectory(prefix='jazyk chat setup ') as temporary:
    root=Path(temporary);home=root/'home';project=root/'project'
    (project/'.jazyk').mkdir(parents=True);(home/'.jazyk').mkdir(parents=True)
    (project/'.jazyk/jazyk.toml').write_text('version = 1\nsets = []\n')
    env={k:v for k,v in os.environ.items() if not k.startswith(('JAZYK_','PASEO_'))}
    env.update(HOME=str(home),PATH='/usr/bin:/bin')
    model=http.server.HTTPServer(('127.0.0.1',0),Model)
    threading.Thread(target=model.serve_forever,daemon=True).start()
    with (root/'stderr.log').open('w') as stderr:
        process=subprocess.Popen([str(binary),'acp','--agent','embedded'],cwd=project,env=env,
           stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,bufsize=0)
        selector=selectors.DefaultSelector();selector.register(process.stdout,selectors.EVENT_READ)
        pending=b'';messages=[]
        def rpc(identifier,method,params):
            global pending
            process.stdin.write((json.dumps({'jsonrpc':'2.0','id':identifier,'method':method,'params':params})+'\n').encode());process.stdin.flush()
            deadline=time.monotonic()+30
            while time.monotonic()<deadline:
                if b'\n' not in pending:
                    if not selector.select(1):continue
                    chunk=os.read(process.stdout.fileno(),65536)
                    if not chunk:raise AssertionError('ACP exited: '+(root/'stderr.log').read_text())
                    pending+=chunk
                    continue
                line,pending=pending.split(b'\n',1)
                if not line.strip():continue
                message=json.loads(line);messages.append(message)
                if message.get('id')==identifier:
                    assert 'error' not in message,message
                    return message['result']
            raise AssertionError(f'Timed out on {method}: '+(root/'stderr.log').read_text())
        try:
            rpc(1,'initialize',{'protocolVersion':1,'clientCapabilities':{},'clientInfo':{'name':'setup-smoke','version':'1'}})
            session=rpc(2,'session/new',{'cwd':str(project),'mcpServers':[]})['sessionId']
            rpc(3,'session/prompt',{'sessionId':session,'prompt':[{'type':'text','text':'Hello before setup'}]})
            assert '/jazyk login' in json.dumps(messages) and '/jazyk openai' in json.dumps(messages)
            assert not requests,'unconfigured message must not reach a model'
            saved=home/'.jazyk/chat-provider.json'
            provider={'source':'endpoint','base_url':f'http://127.0.0.1:{model.server_port}/v1',
                      'model':'fixture','api':'chat_completions','api_key':'fixture-key'}
            saved.write_text(json.dumps(provider));saved.chmod(0o600)
            rpc(4,'session/prompt',{'sessionId':session,'prompt':[{'type':'text','text':'Now configured'}]})
            assert len(requests)==1,requests
            assert 'Configured endpoint works.' in json.dumps(messages)
            assert 'Hello before setup' not in json.dumps(requests),'blocked input must not leak into a later model request'
            rpc(35,'session/set_config_option',{'sessionId':session,'configId':'model','value':'setup'})
            rpc(36,'session/prompt',{'sessionId':session,'prompt':[{'type':'text','text':'Do project work while Setup is selected'}]})
            assert len(requests)==1,'explicit Setup must stay isolated even with saved credentials'
            rpc(37,'session/set_config_option',{'sessionId':session,'configId':'model','value':'endpoint'})
            reject=True;before=len(messages)
            refused=rpc(40,'session/prompt',{'sessionId':session,'prompt':[{'type':'text','text':'Reject invalid credential'}]})
            assert refused['stopReason']=='refusal',refused
            failure=json.dumps(messages[before:])
            assert 'Endpoint authentication failed' in failure and '/jazyk openai' in failure,failure
            assert 'transient' not in failure and 'Please retry' not in failure,failure
            assert len(requests)==2,'401 must not be retried'
            before=len(messages)
            configured=rpc(5,'session/set_config_option',{'sessionId':session,'configId':'model','value':'cloud'})
            assert configured['configOptions'][0]['currentValue']=='cloud'
            assert len(requests)==2,'selecting unsigned Cloud must not infer or open a browser'
            rpc(6,'session/prompt',{'sessionId':session,'prompt':[{'type':'text','text':'/help'}]})
            assert len(requests)==2,'slash commands must work without model authorization'
            rpc(7,'session/close',{'sessionId':session})
            print('ACP initialize, credential-free session, onboarding, live endpoint reload, unsigned Cloud selection and offline slash commands passed')
        finally:
            process.stdin.close()
            try:process.wait(timeout=5)
            except subprocess.TimeoutExpired:process.terminate();process.wait(timeout=5)
            model.shutdown();selector.close()
    # Exercise the browser selection's actual save/shutdown path, using loopback only.
    import urllib.request
    import urllib.error
    import re
    class Cloud(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            assert self.path=='/api/inference/exchange'
            body=json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert len(body['proof'])>=32
            reply=json.dumps({'token':'f'*64,'expires_in':3600}).encode()
            self.send_response(200);self.send_header('Content-Type','application/json');self.send_header('Content-Length',str(len(reply)));self.end_headers();self.wfile.write(reply)
        def log_message(self,*args):pass
    cloud=http.server.HTTPServer(('127.0.0.1',0),Cloud)
    threading.Thread(target=cloud.serve_forever,daemon=True).start()
    try:
        for source in ['endpoint','cloud','endpoint:demo']:
            with (root/f'{source}-gui.log').open('w') as stderr:
                process=subprocess.Popen([str(binary),'gui','--no-open','--port','0'],cwd=project,
                    env=env|{'JAZYK_CHAT_SETUP':source},stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=stderr,bufsize=0)
                try:
                    output=b'';deadline=time.monotonic()+30;match=None
                    while time.monotonic()<deadline and not match:
                        import select
                        if select.select([process.stdout],[],[],1)[0]:output+=os.read(process.stdout.fileno(),65536)
                        match=re.search(rb'port=(\d+)&token=([^\s&]+)',output)
                    assert match,'setup GUI did not start'
                    port,token=match.group(1).decode(),match.group(2).decode()
                    def api(path,body):
                        request=urllib.request.Request(f'http://127.0.0.1:{port}{path}',data=json.dumps(body).encode(),
                          headers={'Content-Type':'application/json','Authorization':f'Bearer {token}','Origin':'https://jazyk.dev'})
                        with urllib.request.urlopen(request,timeout=10) as response:return json.load(response)
                    if source.startswith('endpoint'):
                        body={'agent':'embedded','source':'endpoint','baseUrl':'http://127.0.0.1:1/v1','model':'fixture','apiKey':'fixture-key'}
                    else:
                        challenge=api('/api/agents/cloud-challenge',{})
                        body={'agent':'embedded','source':'cloud','cloudOrigin':f'http://127.0.0.1:{cloud.server_port}','challengeId':challenge['id']}
                    response=api('/api/agents/selections',body)
                    assert 'fixture-key' not in json.dumps(response) and 'f'*64 not in json.dumps(response)
                    provider=json.loads((home/'.jazyk/chat-provider.json').read_text())
                    assert provider['selected']==source
                    if source=='cloud':assert set(provider['providers'])=={'endpoint','cloud'},provider.keys()
                    provider=provider['providers'][source]
                    if source=='cloud':assert provider['expires_at']>time.time() and provider['api_key']=='f'*64
                    assert (home/'.jazyk/chat-provider.json').stat().st_mode&0o777==0o600
                    assert process.poll() is None,'setup helper must remain available after saving, so the browser stays connected'
                finally:
                    if process.poll() is None:process.terminate();process.wait(timeout=5)
                    process.stdout.close()
        print('Browser endpoint and Cloud grant selections save private credentials and keep the setup helper connected, using loopback fixtures only')
    finally:cloud.shutdown()
