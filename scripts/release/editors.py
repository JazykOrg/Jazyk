"""Package thin editor adapters for an immutable editors-vVERSION GitHub release."""
import hashlib
import json
from pathlib import Path
import re
import shutil
import sys
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]

def package(version, vsix, jetbrains, destination, base_url=None):
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('Use a numeric editor release version')
    destination = Path(destination)
    destination.mkdir(parents=True, exist_ok=True)
    base_url = base_url or f'https://github.com/JazykOrg/Jazyk/releases/download/editors-v{version}'
    with zipfile.ZipFile(vsix) as archive:
        manifest = json.loads(archive.read('extension/package.json'))
        if manifest['version'] != version:
            raise ValueError('VSIX version does not match editor release')
        if any(Path(name).name in ('jazyk', 'jazyk.exe') for name in archive.namelist()):
            raise ValueError('Adapters must not contain a Jazyk runtime')
    with zipfile.ZipFile(jetbrains) as archive:
        jars = [name for name in archive.namelist() if name.endswith('/jazyk-jetbrains.jar')]
        if len(jars) != 1:
            raise ValueError('JetBrains ZIP must contain the Jazyk adapter JAR')
        from io import BytesIO
        with zipfile.ZipFile(BytesIO(archive.read(jars[0]))) as jar:
            if ET.fromstring(jar.read('META-INF/plugin.xml')).findtext('version') != version:
                raise ValueError('JetBrains plugin version does not match editor release')
    shutil.copyfile(vsix, destination / 'jazyk.vsix')
    shutil.copyfile(jetbrains, destination / 'jazyk-jetbrains.zip')
    zed = ROOT / 'bootstrap/editors/zed'
    import tomllib
    if tomllib.loads((zed / 'extension.toml').read_text())['version'] != version:
        raise ValueError('Zed adapter version does not match editor release')
    files = [zed / name for name in ('Cargo.toml', 'Cargo.lock', 'extension.toml', 'README.md')]
    files += sorted((zed / 'src').rglob('*.rs'))
    with zipfile.ZipFile(destination / 'jazyk-zed.zip', 'w', compression=zipfile.ZIP_DEFLATED) as archive:
        for file in files:
            entry = zipfile.ZipInfo(file.relative_to(zed).as_posix(), (2026, 1, 1, 0, 0, 0))
            entry.external_attr = 0o100644 << 16
            archive.writestr(entry, file.read_bytes())
    root = ET.Element('plugins')
    plugin = ET.SubElement(root, 'plugin', id='org.jazyk.lsp', version=version,
                           url=f'{base_url}/jazyk-jetbrains.zip')
    ET.SubElement(plugin, 'idea-version', {'since-build': '242'})
    ET.SubElement(plugin, 'name').text = 'Jazyk'
    (destination / 'updatePlugins.xml').write_bytes(ET.tostring(root, encoding='utf-8', xml_declaration=True))
    names = ['jazyk.vsix', 'jazyk-jetbrains.zip', 'jazyk-zed.zip', 'updatePlugins.xml']
    (destination / 'SHA256SUMS').write_text(''.join(
        f'{hashlib.sha256((destination / name).read_bytes()).hexdigest()}  {name}\n' for name in names))
    return names

if __name__ == '__main__':
    package(*sys.argv[1:])
