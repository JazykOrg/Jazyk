"""Create a release, or finish a draft, without replacing published bytes."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from artifacts import digest

version, commit, directory = sys.argv[1:]
assets = Path(directory).resolve()
tag = f'v{version}'
repo = __import__('os').environ['GITHUB_REPOSITORY']

def gh(*args):
    return subprocess.check_output(['gh', *args], text=True)

def optional(endpoint):
    result = subprocess.run(['gh', 'api', endpoint], capture_output=True, text=True)
    if result.returncode == 0:
        return json.loads(result.stdout)
    if '(HTTP 404)' in result.stderr:
        return None
    raise RuntimeError(result.stderr)

def find_release():
    release = optional(f'repos/{repo}/releases/tags/{tag}')
    if release:
        return release
    # Drafts can exist before their tag and are absent from lookup-by-tag.
    pages = json.loads(gh('api', '--paginate', '--slurp', f'repos/{repo}/releases?per_page=100'))
    matches = [item for page in pages for item in page if item['tag_name'] == tag]
    if len(matches) > 1:
        raise ValueError('Multiple releases use the same tag')
    return matches[0] if matches else None

ref = optional(f'repos/{repo}/git/ref/tags/{tag}')
if ref:
    obj = ref['object']
    while obj['type'] == 'tag':
        obj = json.loads(gh('api', f'repos/{repo}/git/tags/{obj["sha"]}'))['object']
    if obj['type'] != 'commit' or obj['sha'] != commit:
        raise ValueError('Existing tag points to a different commit')
release = find_release()
if not release:
    with tempfile.TemporaryDirectory() as tmp:
        notes = Path(tmp) / 'notes.md'
        notes.write_text(f'Jazyk {version}\n\nSource commit: `{commit}`\n\nRun `npx -y jazyk@{version}` or extract the archive for your platform.\nKeep `lib/` beside the executable. Verify downloads against `SHA256SUMS`.\nLinux requires glibc 2.35+. Node.js 22+ is required only for npm.\n')
        gh('release', 'create', tag, '--target', commit, '--draft', '--title', f'Jazyk {version}', '--notes-file', str(notes))
    release = find_release()
    if not release:
        raise RuntimeError('Created draft release could not be found')
if release['draft'] and release['target_commitish'] != commit:
    raise ValueError('Existing draft targets a different source commit')
expected = {p.name for p in assets.iterdir()}
remote = {a['name'] for a in release['assets']}
if remote - expected or (not release['draft'] and remote != expected):
    raise ValueError('Existing release has unexpected or missing assets')
with tempfile.TemporaryDirectory() as tmp:
    for name in sorted(remote):
        gh('release', 'download', tag, '--pattern', name, '--dir', tmp)
        if digest(Path(tmp) / name) != digest(assets / name):
            raise ValueError(f'Existing release asset differs: {name}; publish a new version')
for name in sorted(expected - remote):
    gh('release', 'upload', tag, str(assets / name))
if release['draft']:
    gh('release', 'edit', tag, '--draft=false', '--prerelease=' + str('-' in version).lower(), '--latest=' + str('-' not in version).lower())
print(gh('release', 'view', tag, '--json', 'url'))
