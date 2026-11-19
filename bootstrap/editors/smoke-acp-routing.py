#!/usr/bin/env python3
"""Gateway routing contracts against two local fake ACP agents, with no model inference."""
import json
import os
from pathlib import Path
import selectors
import subprocess
import sys
import tempfile
import threading
import time

if len(sys.argv)>1 and sys.argv[1]=='--fixture':
    name,log=sys.argv[2:]
    count=0
    permission='default'
    output_lock=threading.Lock()
    def emit(message):
        with output_lock:print(json.dumps({'jsonrpc':'2.0',**message}),flush=True)
    def update(sid,text):emit({'method':'session/update','params':{'sessionId':sid,'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':text}}}})
    def options():return [{'id':'model','name':'Model','category':'model','type':'select','currentValue':name+'.default','options':[{'value':name+'.default','name':name+' Default'},{'value':'failure','name':'Rejected fixture'}]},{'id':'permissions','name':'Permissions','category':'mode','type':'select','currentValue':permission,'options':[{'value':'default','name':'Default'},{'value':'ask','name':'Ask'}]}]
    def record(message):
        with open(log,'a') as f:f.write(json.dumps({'backend':name,**message})+'\n')
    for line in sys.stdin:
        request=json.loads(line);record(request)
        if 'method' not in request:continue
        method=request['method'];params=request.get('params',{});identifier=request.get('id')
        if method=='initialize':result={'protocolVersion':1,'agentCapabilities':{'sessionCapabilities':{'close':{}}},'agentInfo':{'name':name,'version':'1'}}
        elif method=='session/new':
            count+=1
            result={'sessionId':f'down-{count}','configOptions':options()}
        elif method=='session/set_config_option':
            if params['value']=='failure':
                emit({'id':identifier,'error':{'code':-32602,'message':'fixture rejected model'}});continue
            if params['configId']=='permissions':permission=params['value']
            emit({'method':'session/update','params':{'sessionId':params['sessionId'],'update':{'sessionUpdate':'config_option_update','configOptions':options()}}})
            result={'configOptions':options()}
        elif method=='session/prompt':
            def finish(identifier=identifier,params=params):
                if any('slow turn' in b.get('text','') for b in params['prompt']):time.sleep(.5)
                sid=params['sessionId']
                update(sid,name+' answer')
                for kind,extra in [('fs/read_text_file',{'path':'/fixture.txt'}),('fs/write_text_file',{'path':'/fixture.txt','content':'fixture'}),('session/request_permission',{'toolCall':{'toolCallId':'fixture','title':'Fixture read','kind':'read','status':'pending'},'options':[{'optionId':'allow','name':'Allow','kind':'allow_once'}]})]:
                    emit({'id':name+kind+str(identifier),'method':kind,'params':{'sessionId':sid,**extra}})
                emit({'id':identifier,'result':{'stopReason':'end_turn'}})
            threading.Thread(target=finish,daemon=True).start();continue
        elif method=='session/close':
            update(params['sessionId'],'STALE '+name)
            emit({'id':'stale-'+name,'method':'fs/write_text_file','params':{'sessionId':params['sessionId'],'path':'/stale.txt','content':'stale'}})
            result={}
        elif method=='session/cancel':continue
        else:
            emit({'id':identifier,'error':{'code':-32601,'message':'unknown method'}});continue
        emit({'id':identifier,'result':result})
    sys.exit(0)

binary=Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix='jazyk routing ') as temporary:
    root=Path(temporary);home=root/'home';project=root/'project';log=root/'downstream.jsonl'
    (home/'.jazyk').mkdir(parents=True);(project/'.jazyk').mkdir(parents=True)
    # Compiler defaults must never turn into human chat choices or inference.
    (home/'.jazyk/config.toml').write_text('[llm]\nbase_url="http://127.0.0.1:9/v1"\nmodel="compiler-only-ollama"\n')
    fixture=str(Path(__file__).resolve())
    config='version=1\nsets=[]\n'
    for name in ['claude','codex']:
        config+=f'\n[acp.agents.{name}]\ncommand={json.dumps(sys.executable)}\nargs={json.dumps([fixture,"--fixture",name,str(log)])}\n'
    config+='\n[acp.agents.broken]\ncommand="/nonexistent/jazyk-fixture"\n'
    config_path=project/'.jazyk/jazyk.toml';config_path.write_text(config)
    env={k:v for k,v in os.environ.items() if not k.startswith(('JAZYK_','PASEO_'))}
    env.update(HOME=str(home),PATH='/usr/bin:/bin')
    with (root/'stderr.log').open('w') as stderr:
        process=subprocess.Popen([str(binary),'acp','--agent','embedded'],cwd=project,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=stderr,bufsize=0)
        pending=b'';selector=selectors.DefaultSelector();selector.register(process.stdout,selectors.EVENT_READ)
        messages=[];client_requests=[];identifier=0
        def send(method,params):
            global identifier
            identifier+=1
            process.stdin.write((json.dumps({'jsonrpc':'2.0','id':identifier,'method':method,'params':params})+'\n').encode());process.stdin.flush();return identifier
        def receive(target):
            global pending
            deadline=time.monotonic()+25
            while time.monotonic()<deadline:
                if b'\n' not in pending:
                    if not selector.select(.1):continue
                    data=os.read(process.stdout.fileno(),65536)
                    assert data,'ACP exited: '+(root/'stderr.log').read_text()
                    pending+=data;continue
                line,pending=pending.split(b'\n',1)
                if not line.strip():continue
                message=json.loads(line);messages.append(message)
                if 'id' in message and 'method' in message:
                    client_requests.append(message)
                    assert message['params']['sessionId']==sid,message
                    assert message['params'].get('path')!='/stale.txt','retired backend reached client'
                    result={'outcome':{'outcome':'selected','optionId':'allow'}} if message['method']=='session/request_permission' else {'content':'fixture'} if message['method']=='fs/read_text_file' else {}
                    process.stdin.write((json.dumps({'jsonrpc':'2.0','id':message['id'],'result':result})+'\n').encode());process.stdin.flush()
                elif message.get('id')==target:return message
            raise AssertionError('timeout: '+(root/'stderr.log').read_text())
        def rpc(method,params):
            message=receive(send(method,params));assert 'error' not in message,message;return message['result']
        def select(value,error=False):
            message=receive(send('session/set_config_option',{'sessionId':sid,'configId':'model','value':value}))
            assert ('error' in message)==error,message
            return message.get('result',{})
        def prompt(text):return rpc('session/prompt',{'sessionId':sid,'prompt':[{'type':'text','text':text}]})
        def logs():return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
        try:
            rpc('initialize',{'protocolVersion':1,'clientCapabilities':{'fs':{'readTextFile':True,'writeTextFile':True}},'clientInfo':{'name':'routing-smoke','version':'1'}})
            session=rpc('session/new',{'cwd':str(project),'mcpServers':[]});sid=session['sessionId']
            model=session['configOptions'][0];values={o['value'] for o in model['options']}
            assert model['currentValue']=='setup' and {'setup','cloud','claude','codex'}<=values,model
            assert 'compiler-only-ollama' not in json.dumps(session)
            prompt('Please change all project files');prompt('/compile')
            assert not logs(),'Setup must not spawn external agents'
            assert '/login' in json.dumps(messages) and '/setup' in json.dumps(messages)
            select('cloud');assert not logs(),'unsigned Cloud selection must be side-effect-free'
            select('claude');assert not [r for r in logs() if r.get('method')=='session/prompt']
            changed=rpc('session/set_config_option',{'sessionId':sid,'configId':'permissions','value':'ask'})
            assert changed['configOptions'][0]['currentValue']=='claude',changed
            assert next(o for o in changed['configOptions'] if o['id']=='permissions')['currentValue']=='ask'
            prompt('First completed human turn')
            selected=select('codex');assert selected['configOptions'][0]['currentValue']=='codex'
            prompt('Continue from that answer')
            turns=[r for r in logs() if r.get('method')=='session/prompt']
            assert len(turns)==2,turns
            forwarded=json.dumps(turns[-1]['params']['prompt'])
            assert 'First completed human turn' in forwarded and 'claude answer' in forwarded and 'Continue from that answer' in forwarded
            assert 'Please change all project files' not in forwarded,'blocked Setup work must not be replayed'
            assert turns[0]['params']['sessionId']==turns[1]['params']['sessionId']=='down-1','fixture exercises colliding backend ids'
            assert any(o['value']=='codex/codex.default' for o in selected['configOptions'][0]['options'])
            concrete=select('codex/codex.default')
            assert concrete['configOptions'][0]['currentValue']=='codex/codex.default'
            select('codex/failure',error=True)
            prompt('Route survives rejected concrete model')
            assert [r for r in logs() if r.get('method')=='session/prompt'][-1]['backend']=='codex'
            select('broken',error=True)
            prompt('/agent claude');prompt('Current conversation changed again')
            assert [r for r in logs() if r.get('method')=='session/prompt'][-1]['backend']=='claude'
            slow=send('session/prompt',{'sessionId':sid,'prompt':[{'type':'text','text':'slow turn'}]})
            time.sleep(.1);select('codex',error=True)
            assert 'result' in receive(slow)
            prompt('Flush pending updates')
            assert all('STALE' not in json.dumps(m) for m in messages),messages
            assert len(client_requests)>=6,'permission and filesystem requests must reach the client'
            assert config_path.read_text()==config,'chat choices must not write compiler/project preferences'
            rpc('session/close',{'sessionId':sid})
            loaded=rpc('session/load',{'sessionId':sid,'cwd':str(project),'mcpServers':[]})
            assert loaded['configOptions'][0]['currentValue']=='claude'
            prompt('Resume after reload')
            assert 'First completed human turn' in json.dumps([r for r in logs() if r.get('method')=='session/prompt'][-1])
            rpc('session/close',{'sessionId':sid})
            print('Setup isolation; compiler/chat separation; picker and /agent live switching; model discovery; context preservation; failed-switch rollback; colliding IDs; stale-route rejection; client permissions/filesystem; busy-turn rejection; persistence passed')
        finally:
            process.stdin.close()
            try:process.wait(timeout=5)
            except subprocess.TimeoutExpired:process.terminate();process.wait(timeout=5)
            selector.close()
