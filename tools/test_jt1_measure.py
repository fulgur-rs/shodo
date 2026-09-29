import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "raikiri"))
import jt1_measure as m  # noqa: E402


class FailureRecords(unittest.TestCase):
    def test_nonzero_exit_is_failure_record(self):
        probe = lambda *a: {"ok": False, "returncode": 1, "stderr_tail": "unknown operation"}
        with tempfile.TemporaryDirectory() as d:
            row = m.measure_document(Path(d), "isolated", m.DOCUMENTS[0], probe)
        self.assertEqual(row["failed"]["engine"], "native")
        self.assertEqual(row["failed"]["returncode"], 1)

    def test_malformed_report_is_failure_record(self):
        probe = lambda *a: {"ok": True, "report": {}}
        with tempfile.TemporaryDirectory() as d:
            row = m.measure_document(Path(d), "pipeline", m.DOCUMENTS[0], probe)
        self.assertIn("error", row["failed"])
        self.assertEqual(row["failed"]["repeat"], 0)


if __name__ == "__main__":
    unittest.main()
