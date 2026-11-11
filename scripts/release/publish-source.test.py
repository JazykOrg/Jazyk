import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('publisher', Path(__file__).with_name('publish-source.py'))
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


def archive(content):
    buffer = io.BytesIO()
    with tarfile.open(fileobj=buffer, mode='w:gz') as tar:
        info = tarfile.TarInfo('example-1.2.3/src/lib.rs')
        info.size = len(content)
        tar.addfile(info, io.BytesIO(content))
    return buffer.getvalue()


class Publication(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.manifest = self.root / 'Cargo.toml'
        self.manifest.write_text('[package]\nname = "example"\nversion = "1.2.3"\n')
        (self.root / 'target/package').mkdir(parents=True)
        self.local = self.root / 'target/package/example-1.2.3.crate'
        self.local.write_bytes(archive(b'local source'))

    def test_missing_version_publishes_without_fetching_download(self):
        with patch.object(publisher.subprocess, 'run') as run, patch.object(
                publisher.subprocess, 'check_output', return_value='404') as fetch:
            publisher.publish(self.manifest)
        self.assertEqual(fetch.call_count, 1)
        self.assertFalse(fetch.call_args.args[0][-1].endswith('/download'))
        self.assertEqual([c.args[0][1] for c in run.call_args_list], ['package', 'publish'])

    def test_registry_failure_does_not_publish(self):
        with patch.object(publisher.subprocess, 'run') as run, patch.object(
                publisher.subprocess, 'check_output', return_value='403'):
            with self.assertRaisesRegex(RuntimeError, 'lookup failed'):
                publisher.publish(self.manifest)
        self.assertEqual(run.call_count, 1)

    def existing(self, data, expected_error=None):
        def fetch(args, **kwargs):
            dest = Path(args[args.index('--output') + 1])
            if args[-1].endswith('/download'):
                dest.write_bytes(data)
            else:
                dest.write_text(json.dumps({'version': {'checksum': hashlib.sha256(data).hexdigest()}}))
            return '200'
        with patch.object(publisher.subprocess, 'run') as run, patch.object(
                publisher.subprocess, 'check_output', side_effect=fetch):
            if expected_error:
                with self.assertRaisesRegex(ValueError, expected_error):
                    publisher.publish(self.manifest)
            else:
                publisher.publish(self.manifest)
        self.assertEqual(run.call_count, 1)

    def test_identical_published_source_is_idempotent(self):
        self.existing(self.local.read_bytes())

    def test_different_published_source_is_refused(self):
        self.existing(archive(b'different source'), 'different contents')


if __name__ == '__main__':
    unittest.main()
