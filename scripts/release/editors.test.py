import importlib.util
from io import BytesIO
from pathlib import Path
import tempfile
import unittest
import zipfile
import hashlib

spec = importlib.util.spec_from_file_location('editors', Path(__file__).with_name('editors.py'))
editors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(editors)

class EditorPackaging(unittest.TestCase):
    def test_artifacts_checksums_versions_and_source_only_zed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary); vsix=root/'input.vsix'; plugin=root/'input.zip'; out=root/'release'
            with zipfile.ZipFile(vsix,'w') as archive:
                archive.writestr('extension/package.json','{"version":"0.2.4"}')
            jar=BytesIO()
            with zipfile.ZipFile(jar,'w') as archive:
                archive.writestr('META-INF/plugin.xml','<idea-plugin><version>0.2.4</version></idea-plugin>')
            with zipfile.ZipFile(plugin,'w') as archive:
                archive.writestr('jazyk/lib/jazyk-jetbrains.jar',jar.getvalue())
            names=editors.package('0.2.4',vsix,plugin,out)
            for line in (out/'SHA256SUMS').read_text().splitlines():
                checksum,name=line.split()
                self.assertEqual(hashlib.sha256((out/name).read_bytes()).hexdigest(),checksum)
            with zipfile.ZipFile(out/'jazyk-zed.zip') as archive:
                self.assertIn('Cargo.toml',archive.namelist())
                self.assertIn('src/lib.rs',archive.namelist())
                self.assertFalse(any('target/' in p or p.endswith('.wasm') for p in archive.namelist()))
            self.assertIn('/editors-v0.2.4/jazyk-jetbrains.zip',(out/'updatePlugins.xml').read_text())
            previous={n:(out/n).read_bytes() for n in names}
            editors.package('0.2.4',vsix,plugin,out)
            self.assertEqual(previous,{n:(out/n).read_bytes() for n in names})
            with self.assertRaises(ValueError):editors.package('0.2.5',vsix,plugin,out)

if __name__=='__main__':unittest.main()
