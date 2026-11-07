"""Validate current-run artifacts or resume an already published native release."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from artifacts import verify


def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)


def verify_source(version, directory, source_run_id):
    assets = Path(directory)
    repo = os.environ['GITHUB_REPOSITORY']
    commit = os.environ['GITHUB_SHA']
    if source_run_id:
        if os.environ['GITHUB_REF'] != 'refs/heads/master' or not source_run_id.isdigit():
            raise ValueError('Resume requires master and a numeric source run ID')
        run = json.loads(gh('api', f'repos/{repo}/actions/runs/{source_run_id}'))
        if (run['repository']['full_name'].lower() != repo.lower()
                or run['path'] != '.github/workflows/release.yml'
                or run['event'] != 'workflow_dispatch'
                or run['head_branch'] != 'master' or run['status'] != 'completed'):
            raise ValueError('Source must be a completed manual release run on master')
        commit = run['head_sha']
    verify(assets, version, commit)
    if source_run_id:
        tag = f'v{version}'
        release = json.loads(gh('api', f'repos/{repo}/releases/tags/{tag}'))
        if release['draft']:
            raise ValueError('Resume requires a published GitHub Release')
        obj = json.loads(gh('api', f'repos/{repo}/git/ref/tags/{tag}'))['object']
        while obj['type'] == 'tag':
            obj = json.loads(gh('api', f'repos/{repo}/git/tags/{obj["sha"]}'))['object']
        if obj['type'] != 'commit' or obj['sha'] != commit:
            raise ValueError('Published tag does not match the source run')
        with tempfile.TemporaryDirectory() as tmp:
            gh('release', 'download', tag, '--pattern', 'release-manifest.json', '--dir', tmp)
            if (Path(tmp) / 'release-manifest.json').read_bytes() != (assets / 'release-manifest.json').read_bytes():
                raise ValueError('Source artifacts differ from the published release manifest')
    print(f'Verified npm artifacts from source commit {commit}')


if __name__ == '__main__':
    verify_source(*sys.argv[1:])
