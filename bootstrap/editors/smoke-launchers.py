#!/usr/bin/env python3
"""Check launcher files, commands and preferences in an isolated home, without inference."""
import hashlib
from io import BytesIO
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import zipfile

binary = Path(sys.argv[1]).resolve()
with tempfile.TemporaryDirectory(prefix='jazyk launcher ') as temporary:
    base=Path(temporary).resolve()
    home, project, programs, artifacts=(base/name for name in ('home','project','bin','artifacts'))
    for directory in (home,project/'.jazyk',programs,artifacts):directory.mkdir(parents=True)
    shared='version = 1\nsets = []\n'
    (project/'.jazyk/jazyk.toml').write_text(shared)
    jar=BytesIO()
    with zipfile.ZipFile(jar,'w') as z:z.writestr('META-INF/plugin.xml','<idea-plugin><version>0.2.4</version></idea-plugin>')
    (artifacts/'adapter.jar').write_bytes(jar.getvalue())
    with zipfile.ZipFile(artifacts/'jazyk-jetbrains.zip','w') as z:z.writestr('jazyk/lib/jazyk-jetbrains.jar',jar.getvalue())
    (artifacts/'updatePlugins.xml').write_text('<plugins/>')
    with zipfile.ZipFile(artifacts/'jazyk.vsix','w') as z:z.writestr('extension/package.json','{"version":"0.2.4"}')
    (artifacts/'SHA256SUMS').write_text(''.join(f'{hashlib.sha256(p.read_bytes()).hexdigest()}  {p.name}\n' for p in sorted(artifacts.iterdir())))
    launcher='''#!/usr/bin/python3
import json, os, pathlib, shutil, sys
home=pathlib.Path(os.environ['HOME']); args=sys.argv[1:]; name=pathlib.Path(sys.argv[0]).name
with (home/'calls.jsonl').open('a') as out:out.write(json.dumps([name,args])+'\\n')
if args[:1]==['--version']:sys.exit(0)
if args[:1]==['--list-extensions']:
    p=home/(name+'-extensions.txt')
    if p.exists():print(p.read_text())
elif args[:1]==['--install-extension']:
    ext='jazyk.jazyk' if args[1].endswith('.vsix') else args[1]
    with (home/(name+'-extensions.txt')).open('a') as out:out.write(ext+'@0.2.4\\n')
elif args[:1]==['installPlugins']:
    properties=pathlib.Path(os.environ['TESTIDE_PROPERTIES']).read_text()
    (home/'installer-properties.txt').write_text(properties)
    values=dict(line.split('=',1) for line in properties.splitlines())
    plugins=pathlib.Path(values['idea.plugins.path'].encode('ascii').decode('unicode_escape'))
    (plugins/'lsp4ij').mkdir(parents=True,exist_ok=True)
    (plugins/'jazyk/lib').mkdir(parents=True,exist_ok=True)
    shutil.copyfile(pathlib.Path(os.environ['JAZYK_EDITOR_ARTIFACT_DIR'])/'adapter.jar',plugins/'jazyk/lib/jazyk-jetbrains.jar')
elif name=='paseo' and args==['reload']:pass
else:(home/'launched.json').write_text(json.dumps({'program':name,'args':args,'cwd':os.getcwd()}))
'''
    for name in ['code','cursor','zed','paseo','claude','codex','gemini','opencode','pi','droid']:
        p=programs/name;p.write_text(launcher);p.chmod(0o755)
    env={k:v for k,v in os.environ.items() if not k.startswith(('JAZYK_','PASEO_'))}
    env |= {'PASEO_HOME':str(home/'.paseo'),'HOME':str(home),'PATH':f'{programs}:/usr/bin:/bin','XDG_DATA_HOME':str(home/'data'),
            'APPDATA':str(home/'roaming'),'LOCALAPPDATA':str(home/'local'),'JAZYK_EDITOR_ARTIFACT_DIR':str(artifacts)}
    def launch(*args):
        result=subprocess.run([str(binary),*args],cwd=project,env=env,stdin=subprocess.DEVNULL,capture_output=True,text=True,timeout=60)
        assert result.returncode==0,(args,result.stdout,result.stderr)
        assert 'Choose again: jazyk --setup' in result.stdout
        assert (project/'.jazyk/jazyk.toml').read_text()==shared
        return result.stdout
    for name,rel in [('claude','.mcp.json'),('codex','.codex/config.toml'),('gemini','.gemini/settings.json'),('opencode','opencode.json'),('pi','.pi/mcp.json'),('droid','.factory/mcp.json')]:
        launch(name);assert (project/rel).is_file()
        assert tomllib.loads((project/'.jazyk/local/launcher.toml').read_text())['target']==name
    print('Six coding-agent configurations and local target preferences passed')
    for choice,editor in [('code','code'),('cursor','cursor')]:
        launch(choice,'--agent','embedded')
        if sys.platform=='darwin':settings=home/'Library/Application Support'/('Code' if editor=='code' else 'Cursor')/'User/settings.json'
        else:settings=home/'.config'/('Code' if editor=='code' else 'Cursor')/'User/settings.json'
        doc=json.loads(settings.read_text())
        assert doc['jazyk.server.path']==str(binary)
        assert doc['acp.agents']['Jazyk']['args']==['acp','--agent','embedded']
        before=settings.read_bytes();modified=settings.stat().st_mtime_ns
        launch(choice)
        assert before==settings.read_bytes() and modified==settings.stat().st_mtime_ns,'repeat setup must not rewrite current settings'
        launch(choice,'--agent','claude')
        assert 'anthropic.claude-code' in (home/(editor+'-extensions.txt')).read_text()
    print('VS Code alias, Cursor, LSP settings, ACP, native Claude extension and repeat setup passed')
    output=launch('zed','--agent','embedded')
    assert 'install dev extension' in output and 'installed Zed LSP adapter' not in output
    zed=json.loads((project/'.zed/settings.json').read_text())
    assert zed['lsp']['jazyk']['binary']['path']==str(binary)
    assert not (home/'Library/Application Support/Zed/extensions/installed/jazyk').exists()
    print('Zed project LSP and supported development-extension handoff passed')
    for choice in ('idea','phpstorm','webstorm','pycharm','goland','rustrover','clion','rider','rubymine'):
        product=base/'products'/choice;(product/'bin').mkdir(parents=True)
        executable=product/'bin'/choice;executable.write_text(launcher);executable.chmod(0o755)
        (programs/choice).symlink_to(executable)
        (product/'product-info.json').write_text(json.dumps({'dataDirectoryName':f'JazykTest-{choice}','envVarBaseName':'TESTIDE',
            'launch':[{'os':'macOS' if sys.platform=='darwin' else 'Linux','launcherPath':f'bin/{choice}'}]}))
        launch(choice,'--agent','embedded')
        config=json.loads((project/'.jazyk/local/ide.json').read_text());assert config['lsp']['command']==str(binary)
        properties=(home/'installer-properties.txt').read_text();assert 'idea.config.path=' in properties and 'idea.system.path=' in properties
        calls=(home/'calls.jsonl').read_text().count('installPlugins');launch(choice)
        assert calls==(home/'calls.jsonl').read_text().count('installPlugins')
    print('All nine JetBrains launchers, native installer, LSP and idempotent setup passed')
    launch('paseo','--agent','embedded')
    config=json.loads((home/'.paseo/config.json').read_text())
    assert config['agents']['providers']['jazyk']['command']==[str(binary),'acp','--agent','embedded']
    launched=json.loads((home/'launched.json').read_text());assert launched['program']=='paseo' and launched['args']==[str(project)]
    assert '["paseo", ["reload"]]' in (home/'calls.jsonl').read_text()
    before=(home/'calls.jsonl').read_text();output=launch()
    assert (home/'calls.jsonl').read_text()==before,'nonterminal bare jazyk must not start an app'
    assert 'IDEs:' in output and 'Coding agents:' in output and 'Jazyk Web' in output
    assert 'Orca' not in output and 'acpx' not in output
    print('Paseo ACP registration/reload/project opening and noninteractive catalog passed')
    # A real terminal distinguishes remembered startup from explicit reselection.
    if os.name == 'posix':
        import pty, select, time, fcntl, termios, struct
        def terminal(arguments, steps):
            master,slave=pty.openpty()
            fcntl.ioctl(slave,termios.TIOCSWINSZ,struct.pack('HHHH',50,200,0,0))
            child=subprocess.Popen([str(binary),*arguments],cwd=project,env=env,stdin=slave,stdout=slave,stderr=slave)
            os.close(slave);output=b''
            try:
                for expected,keys in steps:
                    deadline=time.monotonic()+30
                    while expected not in output and time.monotonic()<deadline:
                        if select.select([master],[],[],1)[0]:
                            try:output+=os.read(master,65536)
                            except OSError:break
                    assert expected in output,(expected,output.decode(errors='replace'))
                    if keys:os.write(master,keys)
                deadline=time.monotonic()+30
                while child.poll() is None and time.monotonic()<deadline:
                    if select.select([master],[],[],1)[0]:
                        try:output+=os.read(master,65536)
                        except OSError:break
                assert child.wait(timeout=5)==0,output.decode(errors='replace')
                while select.select([master],[],[],0)[0]:
                    try:
                        chunk=os.read(master,65536)
                        if not chunk:break
                        output+=chunk
                    except OSError:break
                return output.decode(errors='replace')
            finally:
                os.close(master)
                if child.poll() is None:child.terminate();child.wait(timeout=5)
        reopened=terminal([],[])
        assert 'Choose again: jazyk --setup' in reopened and 'Coding agents' not in reopened
        selection=terminal(['--setup'],[(b'Coding agents',b'v\r'),(b'LSP + ACP in',b'\x1b[B'),(b'MCP in .mcp.json',b'\r')])
        prefs=tomllib.loads((project/'.jazyk/local/launcher.toml').read_text())
        assert prefs['target']=='vscode' and prefs['agents']['vscode']=='claude'
        assert 'Selected:' not in selection and 'vscode    Visual Studio Code' not in selection
        print('Interactive remembered startup, --setup, names-only menus and changing action preview passed')
print('All launcher smoke checks passed')
