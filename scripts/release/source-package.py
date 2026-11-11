"""Stage the publishable crate with its canonical repository-relative resources."""
import argparse
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]


def prepare(output):
    output = Path(output).resolve()
    output.mkdir(parents=True, exist_ok=False)
    files = subprocess.check_output(['git', 'ls-files', '-z', 'bootstrap/src',
        'bootstrap/examples', 'bootstrap/fixtures', 'bootstrap/tests', 'bootstrap/editors', 'docs',
        'runtime-worker', 'bootstrap/vendor/rst-grammar', 'bootstrap/vendor/goose-context-management'], cwd=ROOT).decode().split('\0')
    for name in filter(None, files):
        source = ROOT / name
        if source.is_file():
            # Cargo excludes nested packages, even with an explicit include list.
            # Keep adapter build manifests as resources without defining nested packages.
            staged = name + '.source' if name.startswith('bootstrap/editors/') and source.name == 'Cargo.toml' else name
            dest = output / staged
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, dest)
            if name.startswith(('bootstrap/editors/', 'bootstrap/vendor/rst-grammar/')):
                resource = output / staged.removeprefix('bootstrap/')
                resource.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(source, resource)
    manifest = (ROOT / 'bootstrap/Cargo.toml').read_text()
    manifest = manifest.replace('path = "src/', 'path = "bootstrap/src/')
    manifest = manifest.replace('path = "vendor/', 'path = "bootstrap/vendor/')
    for example in ('distribution_smoke', 'document_proof'):
        manifest = manifest.replace(f'name = "{example}"',
            f'name = "{example}"\npath = "bootstrap/examples/{example}.rs"')
    manifest = manifest.replace('[lib]', '''[package.metadata.jazyk]
source-layout = "repository"

[lib]''', 1)
    manifest = manifest.replace('edition = "2021"', '''edition = "2021"
readme = "README.md"
include = ["vendor/rst-grammar/**", "editors/**", "bootstrap/src/**", "bootstrap/examples/**", "bootstrap/fixtures/**", "bootstrap/tests/**", "bootstrap/editors/**", "docs/**", "runtime-worker/**", "LICENSE", "README.md"]''', 1)
    (output / 'Cargo.toml').write_text(manifest)
    shutil.copy2(ROOT / 'bootstrap/Cargo.lock', output / 'Cargo.lock')
    shutil.copy2(ROOT / 'LICENSE', output / 'LICENSE')
    (output / 'README.md').write_text('''# Jazyk

A natural language compiler with a persistent semantic graph.

Install with `cargo install --locked jazyk`. Rust 1.99.1 or newer and native
compiler tools are required. For PDF input, install PDFium and set
`JAZYK_PDFIUM` to its library path. Prebuilt distributions include PDFium.

Documentation: https://jazyk.org. Source: https://github.com/JazykOrg/Jazyk.
''')
    return output


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output')
    print(prepare(parser.parse_args().output))
