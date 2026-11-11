"""Generate stable package-manager metadata from verified release assets."""
import argparse
import json
from pathlib import Path
import re

PLATFORMS = json.loads((Path(__file__).parent / 'platforms.json').read_text())


def generate(assets, output):
    assets, output = Path(assets), Path(output)
    manifest = json.loads((assets / 'release-manifest.json').read_text())
    version = manifest['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('Stable manifests require a stable semantic version')
    if not re.fullmatch(r'[0-9a-f]{40}', manifest['commit']):
        raise ValueError('Invalid source commit')
    checksums = {}
    for line in (assets / 'SHA256SUMS').read_text().splitlines():
        digest, name = line.split('  ', 1)
        if name in checksums or not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError('Invalid checksums')
        checksums[name] = digest
    base = f'https://github.com/JazykOrg/Jazyk/releases/download/v{version}'

    def archive(key):
        ext = 'zip' if key.startswith('win32') else 'tar.gz'
        name = f'jazyk-{PLATFORMS[key]["target"]}.{ext}'
        digest = manifest['sha256'][name]
        if checksums.get(name) != digest or not re.fullmatch(r'[0-9a-f]{64}', digest):
            raise ValueError(f'Archive checksum mismatch: {name}')
        return f'{base}/{name}', digest

    lines = ['# frozen_string_literal: true', '', 'class Jazyk < Formula',
        '  desc "Natural language compiler with a persistent semantic graph"',
        '  homepage "https://jazyk.org"', f'  version "{version}"', '  license "MIT"', '']
    for os, prefix in [('macos', 'darwin'), ('linux', 'linux')]:
        lines.append(f'  on_{os} do')
        for arch, suffix in [('arm', 'arm64'), ('intel', 'x64')]:
            url, digest = archive(f'{prefix}-{suffix}')
            lines.extend([f'    on_{arch} do', f'      url "{url}"',
                f'      sha256 "{digest}"', '    end'])
        lines.extend(['  end', ''])
    lines.extend(['  def install', '    libexec.install Dir["*"]',
        '    bin.install_symlink libexec/"jazyk"', '  end', '', '  test do',
        '    assert_match "usage: jazyk", shell_output("#{bin}/jazyk --help")',
        '    assert_predicate libexec/"lib", :directory?', '  end', 'end', ''])
    output.mkdir(parents=True, exist_ok=True)
    (output / 'jazyk.rb').write_text('\n'.join(lines))
    url, digest = archive('win32-x64')
    scoop = {'version': version, 'description': 'Natural language compiler with a persistent semantic graph',
        'homepage': 'https://jazyk.org', 'license': 'MIT',
        'architecture': {'64bit': {'url': url, 'hash': digest}}, 'bin': 'jazyk.exe',
        'checkver': {'github': 'https://github.com/JazykOrg/Jazyk'}}
    (output / 'jazyk.json').write_text(json.dumps(scoop, indent=2) + '\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('assets')
    parser.add_argument('output')
    args = parser.parse_args()
    generate(args.assets, args.output)
