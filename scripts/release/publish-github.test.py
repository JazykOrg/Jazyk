"""Exercise draft publication without making network requests."""
import contextlib
import io
import json
import os
from pathlib import Path
import runpy
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch


class DraftPublication(unittest.TestCase):
    def publish(self, existing=False, target='source-commit'):
        state = {'release': None, 'created': 0, 'uploads': [], 'published': False}
        draft = {'tag_name': 'v0.2.0', 'draft': True,
                 'target_commitish': target, 'assets': []}
        if existing:
            state['release'] = draft

        def api(args, **kwargs):
            # GitHub does not expose the unpublished tag through either endpoint.
            self.assertEqual(args[1], 'api')
            return subprocess.CompletedProcess(args, 1, '', 'gh: Not Found (HTTP 404)')

        def gh(args, **kwargs):
            if args[1:4] == ['api', '--paginate', '--slurp']:
                return json.dumps([[state['release']] if state['release'] else []])
            self.assertEqual(args[1], 'release')
            if args[2] == 'create':
                state['created'] += 1
                state['release'] = draft
            elif args[2] == 'upload':
                state['uploads'].append(Path(args[4]).name)
            elif args[2] == 'edit':
                self.assertIn('--draft=false', args)
                self.assertIn('--latest=true', args)
                state['published'] = True
            elif args[2] == 'view':
                return '{"url":"https://github.com/JazykOrg/Jazyk/releases/tag/v0.2.0"}'
            else:
                self.fail(f'Unexpected gh command: {args}')
            return ''

        with tempfile.TemporaryDirectory() as tmp:
            Path(tmp, 'SHA256SUMS').write_text('fixture')
            with patch.dict(os.environ, GITHUB_REPOSITORY='JazykOrg/Jazyk'), \
                 patch.object(sys, 'argv', ['publish-github.py', '0.2.0', 'source-commit', tmp]), \
                 patch('subprocess.run', side_effect=api), \
                 patch('subprocess.check_output', side_effect=gh), \
                 contextlib.redirect_stdout(io.StringIO()):
                runpy.run_path(str(Path(__file__).with_name('publish-github.py')), run_name='__main__')
        return state

    def test_new_draft_is_discovered_before_tag_publication(self):
        state = self.publish()
        self.assertEqual(state['created'], 1)
        self.assertEqual(state['uploads'], ['SHA256SUMS'])
        self.assertTrue(state['published'])

    def test_existing_draft_is_resumed_without_duplicate_creation(self):
        state = self.publish(existing=True)
        self.assertEqual(state['created'], 0)
        self.assertTrue(state['published'])

    def test_draft_for_another_commit_is_rejected(self):
        with self.assertRaisesRegex(ValueError, 'different source commit'):
            self.publish(existing=True, target='another-commit')


if __name__ == '__main__':
    unittest.main()
