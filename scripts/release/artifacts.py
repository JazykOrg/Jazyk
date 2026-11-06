"""Build and verify the exact distribution files shared by both publishers."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
PLATFORMS = json.loads((ROOT / 'scripts/release/platforms.json').read_text())
PDFIUM = 'chromium/8086'


def digest(file):
    return hashlib.sha256(Path(file).read_bytes()).hexdigest()


def run(*args, **kwargs):
    return subprocess.run(args, check=True, **kwargs)


def bundle(key, binary, output):
    meta = PLATFORMS[key]
    dest = Path(output) / key
    dest.mkdir(parents=True, exist_ok=False)
    name = 'jazyk.exe' if key.startswith('win32') else 'jazyk'
    shutil.copy2(binary, dest / name)
    shutil.copy2(ROOT / 'LICENSE', dest / 'LICENSE')
    (dest / name).chmod(0o755)
    with tempfile.TemporaryDirectory() as tmp:
        archive = Path(tmp) / 'pdfium.tgz'
        run('/usr/bin/curl' if sys.platform == 'darwin' else 'curl', '--fail', '--location', '--retry', '3', '--max-time', '180', '--output', str(archive),
            f'https://github.com/bblanchon/pdfium-binaries/releases/download/{PDFIUM}/pdfium-{meta["pdfium"]}.tgz')
        if digest(archive) != meta['sha256']:
            raise ValueError('PDFium checksum mismatch')
        extracted = Path(tmp) / 'extracted'
        with tarfile.open(archive) as tar:
            tar.extractall(extracted, filter='data')
        libs = list(extracted.rglob(meta['library']))
        if len(libs) != 1:
            raise ValueError('Expected one PDFium library')
        (dest / 'lib').mkdir()
        shutil.copy2(libs[0], dest / 'lib' / meta['library'])
        notices = [p for p in extracted.rglob('*') if p.is_file() and
                   ('license' in p.name.lower() or 'license' in str(p.relative_to(extracted).parent).lower())]
        if not notices:
            raise ValueError('Missing PDFium license notices')
        text = [f'PDFium binary: {PDFIUM}\nSource: https://github.com/bblanchon/pdfium-binaries\n'.encode()]
        for file in sorted(notices):
            text.append(f'\n--- {file.relative_to(extracted)} ---\n'.encode() + file.read_bytes())
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', meta['target']],
            cwd=ROOT / 'bootstrap', encoding='utf-8'))
        for package in sorted(metadata['packages'], key=lambda p: (p['name'], p['version'])):
            text.append(f"\n--- Rust dependency {package['name']} {package['version']} ---\nLicense: {package.get('license')}\nSource: {package.get('repository')}\n".encode())
            base = Path(package['manifest_path']).parent
            files = set()
            for child in base.iterdir():
                if child.name.lower().startswith(('license', 'copying', 'notice', 'unlicense')):
                    files.update(child.rglob('*') if child.is_dir() else [child])
            if package.get('license_file'):
                files.add(base / package['license_file'])
            for file in sorted(files):
                if file.is_file():
                    text.append(f"\n{file.relative_to(base)}\n".encode() + file.read_bytes())
        for file in sorted((ROOT / 'scripts/release/licenses').iterdir()):
            text.append(f'\n--- {file.name} ---\n'.encode() + file.read_bytes())
        (dest / 'THIRD_PARTY_NOTICES').write_bytes(b'\n'.join(text))


def archive_distributions(distributions, output, key=None):
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    selected = {key: PLATFORMS[key]} if key else PLATFORMS
    for key, meta in selected.items():
        folder = Path(distributions) / key
        if not key.startswith('win32'):
            (folder / 'jazyk').chmod(0o755)
        if key.startswith('win32'):
            with zipfile.ZipFile(output / f'jazyk-{meta["target"]}.zip', 'w', zipfile.ZIP_DEFLATED) as z:
                for file in sorted(folder.rglob('*')):
                    if file.is_file():
                        z.write(file, file.relative_to(folder))
        else:
            with tarfile.open(output / f'jazyk-{meta["target"]}.tar.gz', 'w:gz') as tar:
                for file in sorted(folder.iterdir()):
                    tar.add(file, arcname=file.name)


def extract(key, archives, output):
    meta = PLATFORMS[key]
    dest = Path(output) / key
    dest.mkdir(parents=True, exist_ok=False)
    if key.startswith('win32'):
        with zipfile.ZipFile(Path(archives) / f'jazyk-{meta["target"]}.zip') as archive:
            archive.extractall(dest)
    else:
        with tarfile.open(Path(archives) / f'jazyk-{meta["target"]}.tar.gz') as archive:
            archive.extractall(dest, filter='data')


def seal(output, version, commit):
    output = Path(output)
    hashes = {p.name: digest(p) for p in sorted(output.iterdir()) if p.is_file() and p.name not in ('SHA256SUMS', 'release-manifest.json')}
    if len(hashes) != 11:
        raise ValueError(f'Expected five archives and six npm tarballs, found {list(hashes)}')
    manifest = {'version': version, 'commit': commit, 'sha256': hashes}
    (output / 'release-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    hashes['release-manifest.json'] = digest(output / 'release-manifest.json')
    (output / 'SHA256SUMS').write_text(''.join(f'{value}  {name}\n' for name, value in hashes.items()))


def verify(output, version, commit):
    output = Path(output)
    manifest = json.loads((output / 'release-manifest.json').read_text())
    if manifest['version'] != version or manifest['commit'] != commit:
        raise ValueError('Release identity mismatch')
    lines = (output / 'SHA256SUMS').read_text().splitlines()
    expected = {**manifest['sha256'], 'release-manifest.json': digest(output / 'release-manifest.json')}
    if len(lines) != len(expected) or set(p.name for p in output.iterdir()) != {*expected, 'SHA256SUMS'}:
        raise ValueError('Unexpected or missing release assets')
    if lines != [f'{value}  {name}' for name, value in expected.items()]:
        raise ValueError('Checksum manifest mismatch')
    for name, value in expected.items():
        if Path(name).name != name or digest(output / name) != value:
            raise ValueError(f'Checksum mismatch: {name}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('command', choices=['bundle', 'archive', 'extract', 'seal', 'verify'])
    parser.add_argument('args', nargs='+')
    args = parser.parse_args()
    {'bundle': bundle, 'archive': archive_distributions, 'extract': extract, 'seal': seal, 'verify': verify}[args.command](*args.args)
