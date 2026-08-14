import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.gates import all_ok, run_gates  # noqa: E402


class GatesTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def test_passing_gate(self):
        results = run_gates(["true"], cwd=self.dir,
                            out_dir=self.dir / "out", log=lambda m: None)
        self.assertTrue(all_ok(results))
        self.assertEqual(results[0].returncode, 0)
        self.assertTrue(results[0].log_path.is_file())

    def test_failing_gate(self):
        results = run_gates(["false"], cwd=self.dir,
                            out_dir=self.dir / "out", log=lambda m: None)
        self.assertFalse(all_ok(results))
        self.assertEqual(results[0].returncode, 1)

    def test_gate_runs_in_cwd(self):
        results = run_gates(["test -f marker.txt"], cwd=self.dir,
                            out_dir=self.dir / "out", log=lambda m: None)
        self.assertFalse(all_ok(results))
        (self.dir / "marker.txt").write_text("x")
        results = run_gates(["test -f marker.txt"], cwd=self.dir,
                            out_dir=self.dir / "out", log=lambda m: None)
        self.assertTrue(all_ok(results))

    def test_dry_run_prints_and_passes(self):
        logs = []
        results = run_gates(["false"], cwd=self.dir / "missing",
                            out_dir=self.dir / "out", dry_run=True,
                            log=logs.append)
        self.assertTrue(all_ok(results))
        self.assertTrue(any("DRY-RUN" in m and "false" in m for m in logs))


if __name__ == "__main__":
    unittest.main()
