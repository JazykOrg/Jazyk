#!/usr/bin/env python3
"""Native endpoint form, named settings, URL readiness and CSRF checks. No model requests."""
import json,os,subprocess,sys,tempfile,time,urllib.request,urllib.error,urllib.parse
from pathlib import Path
binary=Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix='jazyk browser setup ') as tmp:
    root=Path(tmp);home=root/'home';project=root/'project';ready=root/'ready.json'
    (project/'.jazyk').mkdir(parents=True);(home/'.jazyk').mkdir(parents=True)
    (project/'.jazyk/jazyk.toml').write_text('version=1\nsets=[]\n')
    env={k:v for k,v in os.environ.items() if not k.startswith(('JAZYK_','PASEO_'))}
    env.update(HOME=str(home),JAZYK_CHAT_SETUP='endpoint-form',JAZYK_CHAT_SETUP_READY=str(ready))
    with (root/'helper.log').open('w') as output:
        child=subprocess.Popen([str(binary),'gui','--no-open','--port','0'],cwd=project,env=env,stdin=subprocess.DEVNULL,stdout=output,stderr=output)
        try:
            end=time.monotonic()+20
            while not ready.exists() and time.monotonic()<end:
                assert child.poll() is None,(root/'helper.log').read_text();time.sleep(.05)
            assert ready.exists(),'setup did not report its URL'
            link=json.loads(ready.read_text());assert link.startswith('https://jazyk.dev/account/endpoint-setup#')
            fields=urllib.parse.parse_qs(urllib.parse.urlparse(link).fragment)
            url=f"http://127.0.0.1:{fields['port'][0]}/chat-setup?token={fields['token'][0]}"
            class NoRedirect(urllib.request.HTTPRedirectHandler):
                def redirect_request(self,*args):return None
            opener=urllib.request.build_opener(NoRedirect)
            def post(target,origin):
                data=urllib.parse.urlencode({'name':'Work','url':'http://127.0.0.1:9/v1','model':'fixture','key':'fixture-private-key'}).encode()
                request=urllib.request.Request(target,data=data,headers={'Origin':origin,'Content-Type':'application/x-www-form-urlencoded'})
                try:
                    with opener.open(request) as response:return response.status,response.read().decode()
                except urllib.error.HTTPError as error:return error.code,error.headers.get('Location') or error.read().decode()
            origin='https://jazyk.dev'
            try:urllib.request.urlopen(url.replace('/chat-setup','/api/status'))
            except urllib.error.HTTPError as error:assert error.code==401
            else:raise AssertionError('setup nonce must not authorize the project API')
            assert post(url,'https://untrusted.test')[0]==403
            assert post(url.replace('token=','token=wrong'),origin)[0]==401
            status,reply=post(url,origin);assert status==303 and reply=='https://jazyk.dev/account/endpoint-setup?complete=1'
            assert post(url,origin)[0]==401,'callback must be one-use'
            assert 'fixture-private-key' not in reply
            stored=home/'.jazyk/chat-provider.json';saved=json.loads(stored.read_text())
            assert saved['providers']['endpoint:Work']['api_key']=='fixture-private-key'
            assert stored.stat().st_mode&0o777==0o600
            assert child.poll() is None,'saving must not disconnect the browser'
            print('Native form URL, name collection, same-origin save, CSRF/token rejection and private persistence passed')
        finally:child.terminate();child.wait(timeout=10)
