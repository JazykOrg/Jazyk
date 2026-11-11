"""Publish source packages, accepting an existing version only with identical contents."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[2]


def files(data):
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:gz') as archive:
        return {m.name.split('/', 1)[1]: hashlib.sha256(archive.extractfile(m).read()).hexdigest()
                for m in archive if m.isfile() and not m.name.endswith('/.cargo_vcs_info.json')}


def publish(manifest):
    manifest = Path(manifest).resolve()
    package = tomllib.loads(manifest.read_text())['package']
    name, version = package['name'], package['version']
    subprocess.run(['cargo', 'package', '--locked', '--manifest-path', str(manifest)], check=True)
    local = manifest.parent / 'target/package' / f'{name}-{version}.crate'
    base = f'https://crates.io/api/v1/crates/{name}/{version}'
    with tempfile.TemporaryDirectory() as tmp:
        metadata = Path(tmp) / 'version.json'
        dest = Path(tmp) / 'existing.crate'
        curl = '/usr/bin/curl' if sys.platform == 'darwin' else 'curl'

        def fetch(url, output):
            return subprocess.check_output([curl, '--silent', '--show-error', '--location',
                '--retry', '3', '--output', str(output), '--write-out', '%{http_code}',
                '--user-agent', 'jazyk-release (jazyk.org)', url], text=True)

        # Missing static downloads can return 403. The registry metadata uses 404.
        status = fetch(base, metadata)
        if status == '404':
            subprocess.run(['cargo', 'publish', '--locked', '--manifest-path', str(manifest)], check=True)
            return
        if status != '200':
            raise RuntimeError(f'Registry lookup failed: HTTP {status}')
        status = fetch(base + '/download', dest)
        if status != '200':
            raise RuntimeError(f'Registry download failed: HTTP {status}')
        expected = json.loads(metadata.read_text())['version']['checksum']
        if hashlib.sha256(dest.read_bytes()).hexdigest() != expected:
            raise ValueError(f'Registry checksum mismatch: {name} {version}')
        if files(local.read_bytes()) != files(dest.read_bytes()):
            raise ValueError(f'Published {name} {version} has different contents')
        print(f'{name} {version} already published with matching contents')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('kind', choices=['fork', 'jazyk'])
    parser.add_argument('manifest', nargs='?')
    args = parser.parse_args()
    publish(args.manifest or ROOT / 'bootstrap/vendor/goose-context-management/Cargo.toml')
