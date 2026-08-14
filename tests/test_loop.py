"""End-to-end loop tests: echo harnesses, real git in tmp repos only."""
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.orchestrator import Orchestrator  # noqa: E402
from src import statemachine  # noqa: E402

import helpers  # noqa: E402


class LoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.repo = helpers.make_git_repo(self.dir / "repo")
        self.config = helpers.write_echo_config(self.dir / "echo.toml")
        self.logs = []

    def _orch(self, mission_path, **kw):
        kw.setdefault("auto", True)
        kw.setdefault("log", self.logs.append)
        return Orchestrator(tool_dir=helpers.TOOL_DIR,
                            mission_path=mission_path,
                            config_path=self.config, **kw)

    def test_full_loop_echo_reaches_ready(self):
        mission = helpers.write_mission(self.dir / "m.md", self.repo)
        orch = self._orch(mission)
        rc = orch.run()
        self.assertEqual(rc, 0)
        state = statemachine.load(orch.run_dir)
        self.assertEqual(state.phase, "READY")
        # The echo lane file was delivered into the target branch.
        delivered = self.repo / "src" / "example" / "echo-L1-w0.md"
        self.assertTrue(delivered.is_file())
        # Worktrees and gauntlet branches were cleaned up.
        self.assertFalse(Path(state.worktrees[0]).exists())
        branches = helpers.git(self.repo, "branch", "--list", "gauntlet/*")
        self.assertEqual(branches.strip(), "")
        # Run directory has the expected artifacts.
        run_dir = Path(state.run_dir)
        self.assertTrue((run_dir / "mission.md").is_file())
        self.assertTrue((run_dir / "config.toml").is_file())
        self.assertTrue((run_dir / "state.json").is_file())
        self.assertTrue((run_dir / "report.md").is_file())
        review = json.loads(
            (run_dir / "verdicts" / "review-w0.json").read_text())
        self.assertEqual(review, {"groups": []})
        # Lane capsule was deleted after successful integration.
        self.assertFalse(
            (run_dir / "capsules" / "implementer-L1-w0.md").exists())

    def test_full_loop_without_lanes_uses_planner(self):
        mission = helpers.write_mission(self.dir / "m.md", self.repo,
                                        lanes=[])
        orch = self._orch(mission)
        rc = orch.run()
        self.assertEqual(rc, 0)
        state = statemachine.load(orch.run_dir)
        self.assertEqual(state.phase, "READY")
        # Echo planner cut lane E1 owning "**"; echo wrote at repo root.
        self.assertTrue((self.repo / "echo-E1-w0.md").is_file())
        self.assertEqual([l.id for l in state.lanes], ["E1"])

    def test_dry_run_prints_commands_and_changes_nothing(self):
        mission = helpers.write_mission(self.dir / "m.md", self.repo)
        orch = self._orch(mission, dry_run=True)
        rc = orch.run()
        self.assertEqual(rc, 0)
        state = statemachine.load(orch.run_dir)
        # Nothing executed -> nothing integrated -> READY_NO_CHANGE.
        self.assertEqual(state.phase, "READY_NO_CHANGE")
        out = "\n".join(self.logs)
        self.assertIn("DRY-RUN: (cd", out)
        self.assertIn("git worktree add", out)
        self.assertIn("git branch -D", out)  # cleanup commands printed
        self.assertIn("&& true)", out)  # gate command printed, not executed
        # The repo was not modified (only the run record under .missions/).
        self.assertFalse((self.repo / "src").exists())
        status = helpers.git(self.repo, "status", "--porcelain")
        self.assertEqual([l for l in status.splitlines()
                          if ".missions" not in l], [])

    def test_resume_after_init_completes(self):
        mission = helpers.write_mission(self.dir / "m.md", self.repo)
        orch = self._orch(mission)
        orch._phase_init()  # stop right after INIT
        self.assertEqual(orch.state.phase, "PLAN")
        run_dir = orch.run_dir

        resumed_logs = []
        orch2 = Orchestrator(tool_dir=helpers.TOOL_DIR, resume_dir=run_dir,
                             auto=True, log=resumed_logs.append)
        rc = orch2.run()
        self.assertEqual(rc, 0)
        self.assertEqual(orch2.state.phase, "READY")
        self.assertEqual(orch2.state.run_id, orch.state.run_id)
        self.assertTrue(
            (self.repo / "src" / "example" / "echo-L1-w0.md").is_file())

    def test_init_refuses_staged_changes(self):
        (self.repo / "dirty.txt").write_text("staged\n", encoding="utf-8")
        helpers.git(self.repo, "add", "dirty.txt")
        mission = helpers.write_mission(self.dir / "m.md", self.repo)
        orch = self._orch(mission)
        rc = orch.run()
        self.assertEqual(rc, 2)
        self.assertEqual(orch.state.phase, "BLOCKED")
        self.assertIn("staged changes", orch.state.blocked_reason)

    def test_overlapping_prewritten_lanes_blocked(self):
        mission = helpers.write_mission(
            self.dir / "m.md", self.repo, lanes=[
                {"lid": "L1", "owns": '"src/**"'},
                {"lid": "L2", "owns": '"src/example/**"'},
            ])
        orch = self._orch(mission)
        rc = orch.run()
        self.assertEqual(rc, 2)
        self.assertEqual(orch.state.phase, "BLOCKED")
        self.assertIn("overlap", orch.state.blocked_reason)


if __name__ == "__main__":
    unittest.main()
