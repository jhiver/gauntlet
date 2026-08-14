import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src import statemachine  # noqa: E402


class StateRoundTripTest(unittest.TestCase):
    def test_save_load_round_trip(self):
        with tempfile.TemporaryDirectory() as tmp:
            state = statemachine.State(
                run_id="20260814-demo", slug="demo", phase="JUDGE", wave=1,
                repo="/tmp/repo", target_branch="main", gates=["true"],
                base_commit="abc123", run_dir=tmp,
                lanes=[statemachine.LaneState(
                    id="L1", owns=["src/**"], status="integrated",
                    changed=["src/a.md"])],
                harness_health={"agy": "open"},
                reviews=["verdicts/review-w0.json"],
                worktrees=["/tmp/repo-worktree-gauntlet-20260814-demo"],
                branches=["gauntlet/20260814-demo/integration"],
                integrated_changes=True, auto=True, dry_run=False)
            statemachine.save(state)
            loaded = statemachine.load(tmp)
            self.assertEqual(loaded.to_dict(), state.to_dict())
            self.assertEqual(loaded.lanes[0].id, "L1")
            self.assertEqual(loaded.lanes[0].status, "integrated")
            self.assertEqual(loaded.harness_health, {"agy": "open"})

    def test_from_dict_ignores_unknown_keys(self):
        state = statemachine.State.from_dict(
            {"run_id": "x", "future_field": 1})
        self.assertEqual(state.run_id, "x")
        self.assertEqual(state.phase, "INIT")

    def test_save_requires_run_dir(self):
        with self.assertRaises(ValueError):
            statemachine.save(statemachine.State())


if __name__ == "__main__":
    unittest.main()
