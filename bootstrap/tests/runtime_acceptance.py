"""Opt-in real engine/MCP acceptance. Run with a built jazyk and docker or podman."""
import argparse, base64, json, os, selectors, subprocess, tempfile, time
from pathlib import Path

p=argparse.ArgumentParser();p.add_argument('--binary',required=True);p.add_argument('--engine',choices=['docker','podman'],default='docker');p.add_argument('--keep',action='store_true');a=p.parse_args()
root=Path(tempfile.mkdtemp(prefix='jazyk-runtime-acceptance-'))
(root/'.jazyk').mkdir();(root/'docs').mkdir();(root/'src').mkdir();(root/'tests').mkdir()
(root/'docs/main.md').write_text('# Runtime acceptance\n\nProduce files and report test evidence.\n')
(root/'src/value.txt').write_text('source input');(root/'tests/value.txt').write_text('test input')
command='''import json, os, platform, secrets, sys
from pathlib import Path
assert sys.version_info[:2] == (3,12)
assert 'alpine' in Path('/etc/os-release').read_text()
assert Path('src/value.txt').read_text() == 'source input'
assert Path('tests/value.txt').read_text() == 'test input'
assert Path('docs/main.md').exists() and Path('.jazyk/jazyk.toml').exists()
assert not Path('scratch.txt').exists()
c=Path('.cache/state.json'); old=json.loads(c.read_text()) if c.exists() else None
state={'count': old['count']+1 if old else 1,'nonce':old['nonce'] if old else secrets.token_hex(16),'arch':platform.machine()}
if old:
 assert json.loads(Path('products/result.json').read_text())['nonce']==old['nonce']
 assert Path('target/retained.bin').read_text()==old['nonce']
Path('target').mkdir(exist_ok=True);Path('target/retained.bin').write_text(state['nonce'])
c.parent.mkdir(exist_ok=True);c.write_text(json.dumps(state))
Path('products').mkdir(exist_ok=True);Path('products/result.json').write_text(json.dumps(state));Path('products/binary.bin').write_bytes(bytes(range(256))*4)
Path('reports').mkdir(exist_ok=True);Path('reports/tests.xml').write_text('<testsuite><testcase classname="runtime" name="files"/><testcase classname="runtime" name="cache"/></testsuite>')
Path('scratch.txt').write_text('disposable')
print('PASS runtime acceptance',json.dumps(state))
'''
(root/'check.py').write_text(command)
(root/'.jazyk/jazyk.toml').write_text('''version = 1
[[sets]]
id = "ds_main"
name = "Main"
docs = ["docs/**/*.md"]
[[tasks]]
id = "roundtrip"
name = "Roundtrip"
description = "Verify image, files, products, cache, and reports."
command = "python check.py"
image = "docker.io/library/python:3.12-alpine"
products = ["products/**", "target/*.bin"]
cache = [".cache/**"]
reports = ["reports/*.xml"]
[[tasks]]
id = "cancel"
name = "Cancel check"
description = "Exercise cancellation of a running process tree."
command = "echo CHILD_READY; sleep 120 & wait"
image = "docker.io/library/python:3.12-alpine"
timeout_seconds = 180
''')
env={**os.environ,'JAZYK_CONTAINER_ENGINE':a.engine}
fence=root/'.jazyk/owner-cancel'
proc=subprocess.Popen([str(Path(a.binary).resolve()),'mcp','generate,verify','--ephemeral','--cancel-file',str(fence)],cwd=root,env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
seq=0;active=[]
def rpc(method,params):
 global seq
 seq+=1;proc.stdin.write(json.dumps({'jsonrpc':'2.0','id':seq,'method':method,'params':params})+'\n');proc.stdin.flush()
 deadline=time.time()+50
 selector=selectors.DefaultSelector();selector.register(proc.stdout,selectors.EVENT_READ)
 try:
  while time.time()<deadline:
   if not selector.select(1):continue
   line=proc.stdout.readline()
   if not line:raise RuntimeError('MCP exited: '+proc.stderr.read()[:2000])
   result=json.loads(line)
   if result.get('id')==seq:
    assert 'error' not in result,result
    return result['result']
  raise TimeoutError(method)
 finally:selector.close()
def tool(name,**args):
 result=rpc('tools/call',{'name':name,'arguments':args})
 assert not result.get('isError'),result
 for item in result.get('content',[]):
  if item.get('type')=='text':
   value=json.loads(item['text']);return value
 raise AssertionError(result)
def wait(id):
 deadline=time.time()+600
 while time.time()<deadline:
  run=tool('inspect_run',run_id=id)
  if run['terminated']:return run
  time.sleep(1)
 raise TimeoutError(id)
try:
 rpc('initialize',{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'runtime-acceptance','version':'1'}})
 names={t['name'] for t in rpc('tools/list',{})['tools']}
 assert {'run_task','cancel_run','task_context','read_run_changes'}<=names,names
 context=tool('task_context');assert context['execution']['available'],context
 for n in (1,2):
  run=tool('run_task',task_id='roundtrip',expected_revision=context['revision'],operation_key=f'roundtrip-{n}');active.append(run['id'])
  result=wait(run['id']);assert result['state']=='succeeded',result;active.remove(run['id'])
  product=json.loads((root/'products/result.json').read_text());assert product['count']==n,product
  assert (root/'products/binary.bin').read_bytes()==bytes(range(256))*4
  assert not (root/'.cache').exists() and not (root/'scratch.txt').exists()
  assert len(result['result']['tests'])==2,result
  assert 'PASS runtime acceptance' in tool('read_run_log',run_id=run['id'])['text']
  binary=tool('read_run_changes',run_id=run['id'],path='products/binary.bin',download='1');assert base64.b64decode(binary['content'])==bytes(range(256))*4
  print(json.dumps({'engine':a.engine,'run':run['id'],'product':product,'state':result['state']}),flush=True)
 run=tool('run_task',task_id='cancel',expected_revision=context['revision'],operation_key='cancel-owned');active.append(run['id'])
 deadline=time.time()+120
 while time.time()<deadline:
  state=tool('inspect_run',run_id=run['id'])
  if state['state']=='executing' and 'CHILD_READY' in tool('read_run_log',run_id=run['id'])['text']:break
  assert not state['terminated'],state;time.sleep(1)
 else:raise TimeoutError('Did not start cancellation fixture')
 fence.write_text('cancelled')
 result=wait(run['id']);assert result['state']=='cancelled',result;active.remove(run['id'])
 fence.unlink()
 run=tool('run_task',task_id='cancel',expected_revision=context['revision'],operation_key='cancel-invalid-settings');active.append(run['id'])
 deadline=time.time()+120
 while time.time()<deadline:
  state=tool('inspect_run',run_id=run['id'])
  if state['state']=='executing' and 'CHILD_READY' in tool('read_run_log',run_id=run['id'])['text']:break
  assert not state['terminated'],state;time.sleep(1)
 else:raise TimeoutError('Did not start invalid-settings fixture')
 config=root/'.jazyk/jazyk.toml';original=config.read_text()
 try:
  config.write_text('[[tasks]]\ninvalid = [')
  tool('cancel_run',run_id=run['id'])
  result=wait(run['id']);assert result['state']=='cancelled',result;active.remove(run['id'])
 finally:config.write_text(original)
 print('PASS '+a.engine+': MCP submission, fresh containers, image, project inputs, cache, binary products, reports, logs, ownership cancellation, and shutdown.',flush=True)
finally:
 for id in active:
  try:tool('cancel_run',run_id=id);wait(id)
  except Exception as e:print('CLEANUP ERROR',id,str(e),flush=True)
 proc.stdin.close()
 try:proc.wait(timeout=5)
 except subprocess.TimeoutExpired:proc.kill()
 print('Evidence:',root,flush=True)
 if not a.keep and not active:
  import shutil;shutil.rmtree(root)
