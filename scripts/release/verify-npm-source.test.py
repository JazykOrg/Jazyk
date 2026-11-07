"""Guard cross-run artifact resume against source and published-byte mismatches."""
import contextlib
import io
import json
import os
from pathlib import Path
import runpy
import tempfile
import unittest
from unittest.mock import patch

module = runpy.run_path(str(Path(__file__).with_name('verify-npm-source.py')))
verify_source = module['verify_source']


class ResumeSource(unittest.TestCase):
    def invoke(self, *, branch='master', remote_manifest=b'approved', tag_commit='source'):
        run = {'repository': {'full_name': 'JazykOrg/Jazyk'},
               'path': '.github/workflows/release.yml', 'event': 'workflow_dispatch',
               'head_branch': branch, 'status': 'completed', 'head_sha': 'source'}

        def gh(*args):
            if args[0] == 'release':
                Path(args[-1], 'release-manifest.json').write_bytes(remote_manifest)
                return ''
            endpoint = args[1]
            if '/actions/runs/' in endpoint:
                return json.dumps(run)
            if '/releases/tags/' in endpoint:
                return '{"draft":false}'
            if '/git/ref/' in endpoint:
                return json.dumps({'object': {'type': 'commit', 'sha': tag_commit}})
            self.fail(f'Unexpected API request: {args}')

        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, 'release-manifest.json').write_bytes(b'approved')
            with patch.dict(os.environ, GITHUB_REPOSITORY='JazykOrg/Jazyk',
                            GITHUB_SHA='publisher-code', GITHUB_REF='refs/heads/master'), \
                 patch.dict(verify_source.__globals__, gh=gh), \
                 patch.dict(verify_source.__globals__, verify=lambda *args: self.assertEqual(args[2], 'source')), \
                 contextlib.redirect_stdout(io.StringIO()):
                verify_source('0.2.0', tmp, '123')

    def test_validated_source_is_independent_of_publisher_code_revision(self):
        self.invoke()

    def test_unapproved_branch_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'completed manual release run'):
            self.invoke(branch='feature')

    def test_different_published_manifest_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'differ from the published'):
            self.invoke(remote_manifest=b'different bytes')

    def test_moved_published_tag_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'tag does not match'):
            self.invoke(tag_commit='other-source')


if __name__ == '__main__':
    unittest.main()
