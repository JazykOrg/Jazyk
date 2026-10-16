"""Local process regressions for acceptance-harness cleanup, without model calls."""
import errno
import os
from pathlib import Path
import sys
import tempfile
import unittest

from runtime_model_acceptance import ACP


class FullTrace:
    def write(self, _):
        raise OSError(errno.ENOSPC, "Fixture disk full")

    def close(self):
        raise OSError(errno.ENOSPC, "Fixture disk full")


class CleanupTests(unittest.TestCase):
    def test_cancel_reaches_child_when_trace_storage_fails(self):
        with tempfile.TemporaryDirectory(prefix="jazyk-agent-cleanup-") as directory:
            root = Path(directory)
            executable = root / "agent"
            executable.write_text(
                f"#!{sys.executable}\n"
                "import json,sys\n"
                "for line in sys.stdin:\n"
                " if json.loads(line).get('method') == 'session/cancel':\n"
                "  sys.exit(0)\n"
                "sys.exit(19)\n"
            )
            executable.chmod(0o700)
            agent = ACP(str(executable), root, dict(os.environ), root, False)
            agent.trace.close()
            agent.trace = FullTrace()
            agent.close("fixture-session")
            self.assertEqual(agent.proc.returncode, 0, "child must receive cancellation, not just EOF")
            self.assertTrue(agent.stderr.closed)


if __name__ == "__main__":
    unittest.main()
