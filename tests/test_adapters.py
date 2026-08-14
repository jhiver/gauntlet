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


class CmdAdapterTest(unittest.TestCase):
    def _argv(self, write):
        from src.adapters.cmd import CmdAdapter
        return CmdAdapter("cmd", {"default_model": "m"}).build_argv(
            capsule=Path("/c.md"), worktree=Path("/wt"), write=write,
            model=None, effort="medium")

    def test_write_uses_yolo_not_auto_accept(self):
        argv = self._argv(write=True)
        self.assertIn("--yolo", argv)
        self.assertNotIn("--auto-accept", argv)

    def test_read_only_uses_plan_mode(self):
        argv = self._argv(write=False)
        self.assertIn("--permission-mode", argv)
        self.assertIn("plan", argv)
        self.assertNotIn("--yolo", argv)


class JsonlStreamFinalizeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)

    def _finalize(self, content: str) -> str:
        from src.adapters.base import SubprocessAdapter
        out = self.dir / "x.out"
        out.write_text(content, encoding="utf-8")
        SubprocessAdapter._finalize_stream(out)
        self.assertTrue((self.dir / "x.raw").is_file())
        return out.read_text(encoding="utf-8")

    def test_fenced_block_inside_json_strings_becomes_extractable(self):
        # Real newlines here: json.dumps escapes them as \n, and the
        # finalizer's json.loads must restore them as literal lines.
        payload = {"files_changed": [], "tests_run": [],
                   "tests_passed": True, "partial": False, "notes": ""}
        block = f"```gauntlet-report\n{json.dumps(payload)}\n```"
        lines = [
            json.dumps({"role": "assistant", "content": [
                {"type": "text", "text": "working…"}]}),
            "plain non-json line",
            json.dumps({"type": "result", "finalText":
                        f"Done.\n\n{block}"}),
        ]
        text = self._finalize("\n".join(lines) + "\n")
        report = verdicts.validate_report(
            verdicts.extract_block(text, "report"))
        self.assertEqual(report["files_changed"], [])
        self.assertIn("plain non-json line", text)

    def test_empty_file(self):
        self.assertEqual(self._finalize(""), "\n")


if __name__ == "__main__":
    unittest.main()


class AgyAdapterStagingTest(unittest.TestCase):
    """The capsule must be staged inside the lane worktree (and cleaned up):
    a capsule in the main checkout made agy write outside its worktree."""

    def test_capsule_staged_in_worktree_and_cleaned(self):
        from src.adapters import base
        from src.adapters.agy import AgyAdapter
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        d = Path(tmp.name)
        wt = d / "wt"
        wt.mkdir()
        capsule = d / "capsule.md"
        capsule.write_text("mission", encoding="utf-8")
        seen = {}

        def fake_run(self, *, capsule, worktree, write, model, effort,
                     hard_timeout_s, idle_timeout_s, out_dir):
            seen["capsule"] = Path(capsule)
            seen["existed_during_run"] = Path(capsule).is_file()
            return base.RunResult(base.FailureKind.NONE, 0, Path(capsule))

        original = base.SubprocessAdapter.run
        base.SubprocessAdapter.run = fake_run
        try:
            AgyAdapter("agy", {"launcher": "/nonexistent"}).run(
                capsule=capsule, worktree=wt, write=True, model=None,
                effort=None, hard_timeout_s=1, idle_timeout_s=None,
                out_dir=d / "out")
        finally:
            base.SubprocessAdapter.run = original
        self.assertEqual(seen["capsule"], wt / ".gauntlet" / "capsule.md")
        self.assertTrue(seen["existed_during_run"])
        self.assertFalse((wt / ".gauntlet").exists())


class ReasonixAdapterTest(unittest.TestCase):
    def _argv(self, write):
        from src.adapters.reasonix import ReasonixAdapter
        return ReasonixAdapter("reasonix", {}).build_argv(
            capsule=Path("/c.md"), worktree=Path("/wt"), write=write,
            model="deepseek-v4-pro", effort=None)

    def test_print_mode_with_stream_json(self):
        argv = self._argv(write=False)
        self.assertIn("-p", argv)
        self.assertIn("stream-json", argv)
        self.assertNotIn("--events-jsonl", argv)

    def test_read_only_denies_write_bash_git(self):
        argv = self._argv(write=False)
        idx = argv.index("--allowed-tools")
        self.assertEqual(argv[idx + 1], "deny:write,deny:bash,deny:git")

    def test_write_mode_has_no_deny_rules(self):
        self.assertNotIn("--allowed-tools", self._argv(write=True))


class ReviewerCapsuleTest(unittest.TestCase):
    def test_diff_path_is_referenced(self):
        text = capsules.reviewer(_mission(), wave=0, run_id="RUN",
                                 diff_path="/run/reviews/diff-w0.patch")
        self.assertIn("/run/reviews/diff-w0.patch", text)


class ProtocolFidelityTest(unittest.TestCase):
    """The canonical protocol (gauntlet-loop SKILL.md) requires the verbatim
    reviewer stance and the judge action rule inside the capsules."""

    def test_reviewer_capsule_starts_with_verbatim_stance(self):
        text = capsules.reviewer(_mission(), wave=0, run_id="RUN")
        self.assertIn("you HATE what you are seeing", text)
        self.assertIn("Saint-Exupéry", text)
        stance = text.index("You are a senior dev")
        safety = text.index("## Safety")
        self.assertLess(stance, safety)  # the prompt starts with the stance

    def test_judge_capsule_carries_action_rule_and_boundaries(self):
        text = capsules.judge(_mission(), wave=0, run_id="RUN",
                              review_json='{"groups": []}')
        self.assertIn("FIX = justified AND aligned AND (", text)
        self.assertIn("REDESIGN", text)
        self.assertIn("REPORT_ONLY", text)
        self.assertIn("never add a compensating layer", text)
