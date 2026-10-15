"""Small real-model generation, runtime verification, falsification, and repair loop."""
import argparse
import json
from pathlib import Path
import time

from runtime_model_acceptance import ACP, environment
from runtime_lifecycle_acceptance import MCP


RUNNER = """import unittest, sys
from pathlib import Path
from xml.etree.ElementTree import Element, SubElement, tostring
class Reports(unittest.TextTestResult):
    def __init__(self, *args):
        super().__init__(*args)
        self.cases = {}
    def startTest(self, test):
        super().startTest(test)
        self.cases[test.id()] = Element('testcase', classname=type(test).__name__, name=test._testMethodName)
    def addFailure(self, test, err):
        super().addFailure(test, err)
        SubElement(self.cases[test.id()], 'failure').text = self._exc_info_to_string(err, test)
    def addError(self, test, err):
        super().addError(test, err)
        SubElement(self.cases[test.id()], 'error').text = self._exc_info_to_string(err, test)
    def addSkip(self, test, reason):
        super().addSkip(test, reason)
        SubElement(self.cases[test.id()], 'skipped').text = reason
result = unittest.TextTestRunner(resultclass=Reports).run(unittest.defaultTestLoader.discover('tests'))
suite = Element('testsuite')
suite.extend(result.cases.values())
Path('reports').mkdir(exist_ok=True)
Path('reports/tests.xml').write_bytes(tostring(suite))
sys.exit(not result.wasSuccessful())
"""


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--binary', required=True)
    p.add_argument('--env-file', required=True)
    p.add_argument('--evidence', required=True)
    p.add_argument('--model', default='gemma4:31b-mlx')
    p.add_argument('--codec', choices=['native', 'text'], default='native')
    p.add_argument('--engine', choices=['podman', 'docker'], default='podman')
    p.add_argument('--chat', action='store_true', help='Use chat tools with a manual release and binding prerequisite')
    a = p.parse_args()
    evidence = Path(a.evidence).resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    root = evidence / 'project'
    out = root / '.jazyk/ds_main'
    (out / 'graph').mkdir(parents=True)
    (root / 'tests').mkdir()
    (root / 'docs').mkdir()
    (root / 'docs/main.md').write_text('# Greeter\n\nThe Python module greet.py provides greeting(), which returns exactly "Hello, Jazyk!".\n')
    (root / 'runner.py').write_text(RUNNER)
    (root / '.jazyk/jazyk.toml').write_text('''version=1
[[sets]]
id="ds_main"
name="Main"
docs=["docs/**/*.md"]
source=["greet.py", "tests/**", "runner.py"]
[[tasks]]
id="check"
name="Check Python tests"
description="Run unittest tests/test*.py, emitting classname::method JUnit cases under reports. Use this task to verify generated Python source. runner.py is a supplied test adapter; preserve it."
command="python runner.py"
image="docker.io/library/python:3.12-alpine"
timeout_seconds=30
reports=["reports/*.xml"]
''')
    (out / 'status.yaml').write_text(json.dumps({'version': 2, 'generation': 1 if a.chat else 0}))
    (out / 'graph/entities.yaml').write_text(json.dumps({'ent:greeter': {'name': 'Greeter'}}))
    statement = 'The Python module greet.py provides greeting(), which returns exactly "Hello, Jazyk!".'
    (out / 'graph/requirements.yaml').write_text(json.dumps({'req:greeting': {'statement': statement, 'entities': ['ent:greeter']}}))
    binary = str(Path(a.binary).resolve())
    env = environment(a.env_file, a.engine, a.model, a.codec, 32 if a.chat else 16)
    env.update(JAZYK_CALL_TIMEOUT='600', JAZYK_READ_TIMEOUT='600')
    agent = ACP(binary, root, env, evidence, False)
    mcp = None
    sid = None
    started = time.monotonic()
    result = dict(model=a.model, codec=a.codec, engine=a.engine, chat=a.chat, passed=False)
    try:
        agent.receive(agent.send('initialize', {'protocolVersion': 1, 'clientCapabilities': {},
                       'clientInfo': {'name': 'generation-acceptance', 'version': '1'}}), 30)
        sid = agent.receive(agent.send('session/new', {'cwd': str(root), 'mcpServers': [{
            'name': 'jazyk', 'command': binary,
            'args': ['mcp', 'chat', '--ephemeral'] if a.chat else ['mcp', 'generate,verify', '--ephemeral', '--serve-files'], 'env': []}]}), 30)['sessionId']
        def prompt(text):
            reply = agent.receive(agent.send('session/prompt', {'sessionId': sid,
                                  'prompt': [{'type': 'text', 'text': text}]}), 2400 if a.chat else 1200)
            assert reply.get('stopReason') == 'end_turn', reply
        initial = 'Generate the implementation, tests, and run this project. Keep it tiny: one function and one behavioral test. Reuse the existing check task and preserve runner.py. Inspect actual execution results and finish with a short evidence summary.' if a.chat else 'Generate ent:greeter from its begin_generation package. Write greet.py and a small falsifiable unittest under tests/. Reuse the existing check task and preserve runner.py. Record the medium if needed, the complete generation manifest with its factHash and task:check test binding, then run_tests and inspect the actual result. Keep this tiny: one function and one behavioral test. Finish with a short evidence summary.'
        prompt(initial)
        names = [c.get('title') for c in agent.calls]
        for name in ['begin_generation', 'write_text_file', 'record_generation', 'run_tests']:
            assert name in names, names
        if a.chat:
            for name in ['binding_tasks', 'release_work', 'begin_binding', 'record_binding']:
                assert name in names, names
        assert (root / 'runner.py').read_text() == RUNNER
        tests = {str(p.relative_to(root)): p.read_bytes() for p in (root / 'tests').glob('*.py')}
        assert tests, 'No tests generated'
        mcp = MCP(binary, root, a.engine, 'generate,verify', timeout=120)
        # Chat can reconcile the seeded requirement with its quoted document fact.
        # Verify the surviving identity instead of requiring a deleted fixture ID.
        requirements = mcp.tool('get_entity', id='ent:greeter')['requirements']
        assert len(requirements) == 1, requirements
        requirement = requirements[0]['id']
        result['requirement'] = requirement
        def status():
            return mcp.tool('begin_verification', requirement=requirement)['status']
        assert status() == 'verified'
        result['generated_and_verified'] = True
        (root / 'greet.py').write_text('def greeting():\n    return "BROKEN"\n')
        broken = mcp.tool('run_tests', all=True)
        (evidence / 'falsification.json').write_text(json.dumps(broken, indent=2))
        assert status() == 'failing', broken
        result['falsified'] = True
        prompt('The test now fails because greet.py was deliberately broken. Inspect the failure and repair the implementation to satisfy the documented requirement. Preserve the generated test and runner.py. Run the bound check again and report its actual verdict.')
        assert status() == 'verified'
        assert (root / 'runner.py').read_text() == RUNNER
        assert {str(p.relative_to(root)): p.read_bytes() for p in (root / 'tests').glob('*.py')} == tests
        runs = mcp.tool('task_context')['execution']['runs']
        assert len(runs) >= 3 and all(r['terminated'] for r in runs), runs
        assert {'succeeded', 'failed'} <= {r['state'] for r in runs}, runs
        result.update(passed=True, seconds=round(time.monotonic()-started, 2), calls=[c.get('title') for c in agent.calls], runs=len(runs))
        print('PASS', json.dumps(result), flush=True)
    except (Exception, KeyboardInterrupt) as error:
        result.update(error=str(error) or type(error).__name__, seconds=round(time.monotonic()-started, 2), calls=[c.get('title') for c in agent.calls])
        raise
    finally:
        (evidence / 'result.json').write_text(json.dumps(result, indent=2))
        agent.close(sid)
        if mcp is None:
            mcp = MCP(binary, root, a.engine)
        for run in mcp.tool('task_context')['execution']['runs']:
            if not run.get('terminated'):
                mcp.tool('cancel_run', run_id=run['id'])
                mcp.wait(run)
        mcp.close()
        print('Evidence:', evidence, flush=True)


if __name__ == '__main__':
    main()
