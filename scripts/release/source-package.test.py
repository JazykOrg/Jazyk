import importlib.util
from pathlib import Path
import re
import tempfile
import tomllib
import unittest

spec = importlib.util.spec_from_file_location('source', Path(__file__).with_name('source-package.py'))
source = importlib.util.module_from_spec(spec)
spec.loader.exec_module(source)


class SourcePackage(unittest.TestCase):
    def test_all_literal_embedded_resources_survive_staging(self):
        with tempfile.TemporaryDirectory() as tmp:
            output = source.prepare(Path(tmp) / 'package')
            manifest = tomllib.loads((output / 'Cargo.toml').read_text())
            self.assertTrue((output / manifest['lib']['path']).is_file())
            self.assertTrue((output / manifest['bin'][0]['path']).is_file())
            count = 0
            for file in (output / 'bootstrap/src').rglob('*.rs'):
                for name in re.findall(r'include_(?:str|bytes)!\(\s*"([^"\n]+)"', file.read_text()):
                    self.assertTrue((file.parent / name).is_file(), f'{file}: {name}')
                    count += 1
            self.assertGreater(count, 50)
            self.assertTrue((output / 'runtime-worker/main.go').is_file())
            self.assertTrue((output / 'LICENSE').is_file())
            self.assertEqual((output / 'vendor/rst-grammar/rst.pest').read_bytes(),
                             (source.ROOT / 'bootstrap/vendor/rst-grammar/rst.pest').read_bytes())
            self.assertFalse((output / '.env').exists())
            self.assertFalse((output / 'AGENTS.md').exists())
            with self.assertRaises(FileExistsError):
                source.prepare(output)


if __name__ == '__main__':
    unittest.main()
