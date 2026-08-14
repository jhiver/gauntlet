import io
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src import capsules, verdicts  # noqa: E402
from src.adapters.base import FailureKind  # noqa: E402
from src.adapters.echo import EchoAdapter  # noqa: E402
from src.adapters.human import HumanAdapter, read_decision  # noqa: E402
from src.mission import Lane, Mission, Repo  # noqa: E402


def _mission():
    return Mission(slug="t", repos=[Repo(path="/tmp/x")],
                   lanes=[], body="# Obj\n\n- AC-1: x\n",
                   contract_ids={"AC-1"}, source_path=Path("m.md"))


def _lane_capsule(tmp: Path) -> Path:
    lane = Lane(id="L1", owns=["src/example/**"], tests=["true"],
                brief="b")
    text = capsules.implementer(_mission(), lane, wave=0, run_id="RUN")
    path = tmp / "capsule.md"
    path.write_text(text, encoding="utf-8")
    return path


class EchoAdapterTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def _run(self, write=True, worktree=None):
        capsule = _lane_capsule(self.dir)
        return EchoAdapter().run(
            capsule=capsule, worktree=worktree or self.dir / "wt",
            write=write, model=None, effort=None, hard_timeout_s=5,
            idle_timeout_s=None, out_dir=self.dir / "out")

    def test_emits_valid_blocks_for_every_role(self):
        (self.dir / "wt").mkdir()
        result = self._run()
        self.assertEqual(result.failure, FailureKind.NONE)
        text = result.output_path.read_text(encoding="utf-8")
        report = verdicts.validate_report(
            verdicts.extract_block(text, "report"))
        self.assertEqual(report["files_changed"],
                         ["src/example/echo-L1-w0.md"])
        self.assertEqual(report["tests_run"], ["true"])
        groups = verdicts.validate_verdict(
            verdicts.extract_block(text, "verdict"), {"AC-1"})
        self.assertEqual(groups, [])  # NO_CLAIMS
        lanes = verdicts.validate_plan(verdicts.extract_block(text, "plan"))
        self.assertEqual(lanes[0]["id"], "E1")

    def test_write_mode_creates_file_inside_owned_path(self):
        wt = self.dir / "wt"
        wt.mkdir()
        self._run(worktree=wt)
        created = wt / "src" / "example" / "echo-L1-w0.md"
        self.assertTrue(created.is_file())

    def test_missing_worktree_means_no_write(self):
        result = self._run(worktree=self.dir / "nope")
        text = result.output_path.read_text(encoding="utf-8")
        report = verdicts.validate_report(
            verdicts.extract_block(text, "report"))
        self.assertEqual(report["files_changed"], [])


class HumanAdapterTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self._stdin = sys.stdin

    def tearDown(self):
        sys.stdin = self._stdin

    def _run_with_stdin(self, data: str):
        sys.stdin = io.StringIO(data)
        capsule = self.dir / "capsule.md"
        capsule.write_text("do the thing\n", encoding="utf-8")
        return HumanAdapter().run(
            capsule=capsule, worktree=self.dir, write=False, model=None,
            effort=None, hard_timeout_s=5, idle_timeout_s=None,
            out_dir=self.dir / "out")

    def test_approve_decision(self):
        result = self._run_with_stdin("approve\n")
        self.assertEqual(result.failure, FailureKind.NONE)
        self.assertEqual(read_decision(result.output_path), "approve")

    def test_reject_decision(self):
        result = self._run_with_stdin("reject\n")
        self.assertEqual(result.failure, FailureKind.NONE)
        self.assertEqual(read_decision(result.output_path), "reject")

    def test_pasted_block_is_captured(self):
        block = "```gauntlet-verdict\n{\"groups\": []}\n```\n"
        result = self._run_with_stdin(block)
        self.assertEqual(result.failure, FailureKind.NONE)
        self.assertEqual(read_decision(result.output_path), "output")
        data = verdicts.extract_block_from_file(result.output_path, "verdict")
        self.assertEqual(data, {"groups": []})

    def test_eof_is_crash_not_block(self):
        result = self._run_with_stdin("")
        self.assertEqual(result.failure, FailureKind.CRASH)
        self.assertIn("EOF", result.detail)

    def test_json_dumps_in_capsule_lines(self):
        # The echo harness relies on JSON-parseable lane-owns lines.
        capsule = _lane_capsule(self.dir)
        text = capsule.read_text(encoding="utf-8")
        owns_line = next(l for l in text.splitlines()
                         if l.startswith("lane-owns:"))
        self.assertEqual(json.loads(owns_line.split(":", 1)[1].strip()),
                         ["src/example/**"])


if __name__ == "__main__":
    unittest.main()
