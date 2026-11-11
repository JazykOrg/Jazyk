import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('managers', Path(__file__).with_name('package-managers.py'))
managers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(managers)


class Manifests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.assets = Path(self.tmp.name) / 'assets'
        self.assets.mkdir()
        self.output = Path(self.tmp.name) / 'output'
        self.hashes = {f'jazyk-{p["target"]}.{ "zip" if k.startswith("win32") else "tar.gz"}':
            str(i + 1) * 64 for i, (k, p) in enumerate(managers.PLATFORMS.items())}
        self.manifest = {'version': '1.2.3', 'commit': 'a' * 40, 'sha256': self.hashes}
        self.save()

    def save(self):
        (self.assets / 'release-manifest.json').write_text(json.dumps(self.manifest))
        (self.assets / 'SHA256SUMS').write_text(''.join(f'{v}  {k}\n' for k, v in self.hashes.items()))

    def test_complete_distribution_and_platforms(self):
        managers.generate(self.assets, self.output)
        formula = (self.output / 'jazyk.rb').read_text()
        self.assertIn('libexec.install Dir["*"]', formula)
        self.assertIn('bin.install_symlink libexec/"jazyk"', formula)
        self.assertEqual(formula.count('sha256 "'), 4)
        self.assertNotIn('/latest/', formula)
        scoop = json.loads((self.output / 'jazyk.json').read_text())
        self.assertEqual(scoop['bin'], 'jazyk.exe')
        self.assertIn('/v1.2.3/', scoop['architecture']['64bit']['url'])
        self.assertEqual(scoop['architecture']['64bit']['hash'], '5' * 64)

    def test_reject_mismatched_checksums(self):
        (self.assets / 'SHA256SUMS').write_text('0' * 64 + '  ' + next(iter(self.hashes)))
        with self.assertRaises(ValueError):
            managers.generate(self.assets, self.output)

    def test_reject_prerelease(self):
        self.manifest['version'] = '1.2.3-rc.1'
        self.save()
        with self.assertRaises(ValueError):
            managers.generate(self.assets, self.output)

    def test_reject_duplicate_checksums(self):
        p = self.assets / 'SHA256SUMS'
        p.write_text(p.read_text() * 2)
        with self.assertRaises(ValueError):
            managers.generate(self.assets, self.output)


if __name__ == '__main__':
    unittest.main()
