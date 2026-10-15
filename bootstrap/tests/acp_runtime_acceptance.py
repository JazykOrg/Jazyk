"""Real ACP proxy/MCP/runtime conformance with a deterministic downstream agent.
No IDE terminal or filesystem capability is advertised. No model verdict is claimed.
"""
import argparse,json,os,queue,subprocess,sys,tempfile,threading,time
from pathlib import Path

def agent():
 lock=threading.Lock();mcp=None;seq=0
 def send(value):
  with lock:print(json.dumps(value),flush=True)
 def tool(name,**args):
  nonlocal seq
  seq+=1;mcp.stdin.write(json.dumps({'jsonrpc':'2.0','id':seq,'method':'tools/call','params':{'name':name,'arguments':args}})+'\n');mcp.stdin.flush()
  while True:
   line=mcp.stdout.readline()
   if not line:raise RuntimeError('injected MCP exited')
   reply=json.loads(line)
   if reply.get('id')==seq:
    assert 'error' not in reply,reply
    result=reply['result'];assert not result.get('isError'),result
    return json.loads(result['content'][0]['text'])
 def prompt(message):
  try:
   params=message['params'];sid=params['sessionId'];text=params['prompt'][0]['text'];context=tool('task_context')
   run=tool('run_task',task_id='slow' if text=='cancel' else 'product',expected_revision=context['revision'],operation_key='acp-'+str(time.time_ns()))
   deadline=time.time()+360;announced=False
   while time.time()<deadline:
    run=tool('inspect_run',run_id=run['id'])
    if run['state'] in ('executing','running') and not announced and 'CHILD_READY' in tool('read_run_log',run_id=run['id'])['text']:
     send({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':sid,'update':{'sessionUpdate':'tool_call','toolCallId':run['id'],'title':'Runtime executing','kind':'execute','status':'in_progress'}}});announced=True
    if run['terminated']:break
    time.sleep(.3)
   else:raise TimeoutError(run)
   send({'jsonrpc':'2.0','method':'session/update','params':{'sessionId':sid,'update':{'sessionUpdate':'agent_message_chunk','content':{'type':'text','text':json.dumps(run)}}}})
   send({'jsonrpc':'2.0','id':message['id'],'result':{'stopReason':'end_turn'}})
  except Exception as e:send({'jsonrpc':'2.0','id':message['id'],'error':{'code':-32603,'message':str(e)}})
 for line in sys.stdin:
  message=json.loads(line);method=message.get('method')
  if method=='initialize':send({'jsonrpc':'2.0','id':message['id'],'result':{'protocolVersion':1,'agentCapabilities':{'mcpCapabilities':{'http':False,'sse':False}},'authMethods':[]}})
  elif method=='session/new':
   server=next(s for s in message['params']['mcpServers'] if s['name']=='jazyk')
   mcp=subprocess.Popen([server['command'],*server['args']],cwd=message['params']['cwd'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=sys.stderr,text=True,bufsize=1)
   send({'jsonrpc':'2.0','id':message['id'],'result':{'sessionId':'runtime-ide'}})
  elif method=='session/prompt':threading.Thread(target=prompt,args=(message,),daemon=True).start()
  elif method=='session/cancel':pass # Deliberately do not implement cancellation: the proxy must own it.
  elif 'id' in message:send({'jsonrpc':'2.0','id':message['id'],'result':{}})
 if mcp:mcp.terminate()

if '--agent' in sys.argv:
 agent();sys.exit()
p=argparse.ArgumentParser();p.add_argument('--binary',required=True);a=p.parse_args()
root=Path(tempfile.mkdtemp(prefix='jazyk-acp-runtime-'));(root/'.jazyk').mkdir()
(root/'.jazyk/jazyk.toml').write_text(f'''version = 1
[acp]
agent = "acceptance"
[acp.agents.acceptance]
command = {json.dumps(sys.executable)}
args = [{json.dumps(str(Path(__file__).resolve()))}, "--agent"]
[[tasks]]
id = "slow"
name = "Cancellation"
description = "Exercise cancellation while running."
command = "echo CHILD_READY; sleep 120 & wait"
image = "docker.io/library/python:3.12-alpine"
[[tasks]]
id = "product"
name = "Product"
description = "Create a retained product."
command = "mkdir -p products; echo ACP-verified > products/proof.txt"
image = "docker.io/library/python:3.12-alpine"
products = ["products/**"]
''')
proc=subprocess.Popen([str(Path(a.binary).resolve()),'acp'],cwd=root,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
messages=queue.Queue();seq=0
def read_messages():
 for line in proc.stdout:messages.put(line)
 messages.put(None)
threading.Thread(target=read_messages,daemon=True).start()

def send(method,params,notify=False):
 global seq
 seq+=1;value={'jsonrpc':'2.0','method':method,'params':params}
 if not notify:value['id']=seq
 proc.stdin.write(json.dumps(value)+'\n');proc.stdin.flush();return seq

def receive(id,cancel=False):
 deadline=time.time()+420;result=None;sent=False
 while time.time()<deadline:
  try:line=messages.get(timeout=1)
  except queue.Empty:continue
  if not line:raise RuntimeError(proc.stderr.read()[:2000])
  message=json.loads(line)
  update=message.get('params',{}).get('update',{})
  if update.get('sessionUpdate')=='tool_call' and cancel and not sent:
   send('session/cancel',{'sessionId':'runtime-ide'},True);sent=True
  if update.get('sessionUpdate')=='agent_message_chunk':
   try:result=json.loads(update['content']['text'])
   except (ValueError,KeyError):pass
  if message.get('id')==id:
   assert 'error' not in message,message
   return result if result is not None else message['result']
 raise TimeoutError('ACP request timed out')
try:
 receive(send('initialize',{'protocolVersion':1,'clientCapabilities':{},'clientInfo':{'name':'no-terminal-ide','version':'1'}}))
 receive(send('session/new',{'cwd':str(root),'mcpServers':[]}))
 run=receive(send('session/prompt',{'sessionId':'runtime-ide','prompt':[{'type':'text','text':'cancel'}]}),True)
 assert run['state']=='cancelled' and run['terminated'],run
 run=receive(send('session/prompt',{'sessionId':'runtime-ide','prompt':[{'type':'text','text':'product'}]}))
 assert run['state']=='succeeded' and run['terminated'],run
 assert (root/'products/proof.txt').read_text().strip()=='ACP-verified'
 print('PASS ACP: IDE without terminal, injected task tools before compilation, executing-run cancellation independent of agent cooperation, next prompt, product import, shutdown.')
finally:
 proc.stdin.close()
 try:proc.wait(timeout=5)
 except subprocess.TimeoutExpired:proc.terminate()
 print('Evidence:',root)
