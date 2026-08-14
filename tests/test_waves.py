"""Fix-wave convergence, polish pass, and blocked-terminal kinds.

The loop tests drive a real tmp git repo with the echo harness; reviewer and
judge verdicts are scripted per wave so the wave policy can be exercised
without an LLM.
"""
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src import statemachine  # noqa: E402
from src.adapters.base import FailureKind, RunResult  # noqa: E402
from src.fallback import ChainExhausted, ChainOutcome  # noqa: E402
from src.orchestrator import Orchestrator  # noqa: E402

import helpers  # noqa: E402


def group(root_cause="rc", verdict="FIX", defect_class="code_defect",
          owns="src/example/**"):
    return {"root_cause": root_cause, "claims": ["c"],
            "contract_ids": ["AC-1"], "verdict": verdict,
            "class": defect_class, "fix": "do x", "owns": owns}


def blocking(n: int, wave: int = 0):
    """A verdict with `n` blocking groups (distinct root causes per wave)."""
    return {"groups": [group(root_cause=f"rc-w{wave}-{i}") for i in range(n)]}


class ConvergenceRuleTest(unittest.TestCase):
    def state(self, history, count, *, wave=0, cap=5):
        return statemachine.convergence_state(history, count, wave=wave,
                                              max_total_waves=cap)

    def test_first_judgment_always_converging(self):
        self.assertEqual(self.state([], 7), statemachine.CONVERGING)

    def test_strict_decrease_converges(self):
        self.assertEqual(self.state([7], 4, wave=1), statemachine.CONVERGING)
        self.assertEqual(self.state([7, 4], 1, wave=2),
                         statemachine.CONVERGING)

    def test_equal_count_is_stalled(self):
        self.assertEqual(self.state([4], 4, wave=1), statemachine.STALLED)

    def test_increase_is_stalled(self):
        self.assertEqual(self.state([7, 4], 5, wave=2), statemachine.STALLED)

    def test_oscillation_below_previous_but_not_below_best_is_stalled(self):
        # 7 -> 4 -> 5 -> 4: better than the round before, never better than
        # the best round; that is not convergence.
        self.assertEqual(self.state([7, 4, 5], 4, wave=3),
                         statemachine.STALLED)

    def test_absolute_cap_wins_over_convergence(self):
        self.assertEqual(self.state([7, 4], 1, wave=2, cap=2),
                         statemachine.CAPPED)

    def test_zero_cap_forbids_any_fix_wave(self):
        self.assertEqual(self.state([], 3, wave=0, cap=0),
                         statemachine.CAPPED)


class ScriptedOrchestrator(Orchestrator):
    """Orchestrator with scripted reviewer/judge verdicts per wave."""

    def __init__(self, *args, verdicts_by_wave=None, polish_mode=None, **kw):
        super().__init__(*args, **kw)
        self.verdicts_by_wave = verdicts_by_wave or {}
        self.polish_mode = polish_mode  # None | "fail" | "rogue"
        self.human_answers: list[bool] = []
        self.asked: list[str] = []

    def _canned(self, role: str, data: dict) -> ChainOutcome:
        out_dir = self.run_dir / "outputs"
        out_dir.mkdir(parents=True, exist_ok=True)
        path = out_dir / f"scripted-{role}-w{self.state.wave}.out"
        path.write_text(
            "```gauntlet-verdict\n" + json.dumps(data) + "\n```\n",
            encoding="utf-8")
        return ChainOutcome(RunResult(FailureKind.NONE, 0, path, "scripted"),
                            "scripted", 1)

    def _run_role(self, role, capsule, worktree, write, lane_id=None):
        if role in ("reviewer", "judge"):
            return self._canned(
                role, self.verdicts_by_wave.get(self.state.wave,
                                                {"groups": []}))
        if role == "fixer" and capsule.name.startswith("polish"):
            if self.polish_mode == "fail":
                raise ChainExhausted("scripted polish failure")
            if self.polish_mode == "rogue":
                (Path(worktree) / "rogue.txt").write_text("nope\n",
                                                          encoding="utf-8")
                out = self.run_dir / "outputs" / "scripted-polish.out"
                out.write_text(
                    "```gauntlet-report\n" + json.dumps(
                        {"files_changed": ["rogue.txt"], "tests_run": [],
                         "tests_passed": True, "partial": False,
                         "notes": ""}) + "\n```\n", encoding="utf-8")
                return ChainOutcome(
                    RunResult(FailureKind.NONE, 0, out, "scripted"),
                    "scripted", 1)
        return super()._run_role(role, capsule, worktree, write, lane_id)

    def _ask_human(self, name, context):
        self.asked.append(name)
        if self.human_answers:
            return self.human_answers.pop(0)
        return super()._ask_human(name, context)


class WaveLoopTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.repo = helpers.make_git_repo(self.dir / "repo")
        self.config = helpers.write_echo_config(self.dir / "echo.toml")
        self.logs = []

    def _config_with(self, replacements: dict[str, str]) -> Path:
        text = helpers.ECHO_CONFIG
        for old, new in replacements.items():
            self.assertIn(old, text)
            text = text.replace(old, new)
        path = self.dir / "echo-custom.toml"
        path.write_text(text, encoding="utf-8")
        return path

    def _orch(self, *, verdicts_by_wave=None, gates=None, config=None, **kw):
        mission = helpers.write_mission(self.dir / "m.md", self.repo,
                                        gates=gates)
        return ScriptedOrchestrator(
            tool_dir=helpers.TOOL_DIR, mission_path=mission,
            config_path=config or self.config, auto=True,
            log=self.logs.append, verdicts_by_wave=verdicts_by_wave, **kw)

    # ------------------------------------------------------------ convergence

    def test_converging_run_gets_as_many_waves_as_it_needs(self):
        orch = self._orch(verdicts_by_wave={
            0: blocking(3, 0), 1: blocking(2, 1), 2: blocking(1, 2),
            3: {"groups": []},
        })
        self.assertEqual(orch.run(), 0)
        self.assertEqual(orch.state.phase, "READY")
        self.assertEqual(orch.state.blocking_history, [3, 2, 1, 0])
        self.assertEqual(orch.state.wave, 3)  # three fix waves, none capped

    def test_stalled_convergence_blocks_in_auto(self):
        orch = self._orch(verdicts_by_wave={
            0: blocking(3, 0), 1: blocking(3, 1),
        })
        self.assertEqual(orch.run(), 2)
        self.assertEqual(orch.state.phase, "BLOCKED_CONVERGENCE")
        self.assertIn("3 → 3", orch.state.blocked_reason)
        self.assertIn("wave-cap", orch.asked)  # director was consulted first

    def test_oscillation_blocks(self):
        orch = self._orch(verdicts_by_wave={
            0: blocking(3, 0), 1: blocking(1, 1), 2: blocking(2, 2),
        })
        self.assertEqual(orch.run(), 2)
        self.assertEqual(orch.state.phase, "BLOCKED_CONVERGENCE")
        self.assertEqual(orch.state.blocking_history, [3, 1, 2])

    def test_director_may_grant_one_more_wave_on_a_stall(self):
        orch = self._orch(verdicts_by_wave={
            0: blocking(3, 0), 1: blocking(3, 1), 2: {"groups": []},
        })
        orch.human_answers = [True]
        self.assertEqual(orch.run(), 0)
        self.assertEqual(orch.state.phase, "READY")
        self.assertEqual(orch.state.blocking_history, [3, 3, 0])

    def test_on_wave_cap_block_skips_the_director(self):
        config = self._config_with(
            {'on_wave_cap = "checkpoint"': 'on_wave_cap = "block"'})
        orch = self._orch(config=config, verdicts_by_wave={
            0: blocking(3, 0), 1: blocking(3, 1),
        })
        self.assertEqual(orch.run(), 2)
        self.assertEqual(orch.state.phase, "BLOCKED_CONVERGENCE")
        self.assertNotIn("wave-cap", orch.asked)

    def test_absolute_cap_stops_a_converging_run(self):
        config = self._config_with(
            {"max_total_waves = 5": "max_total_waves = 1"})
        orch = self._orch(config=config, verdicts_by_wave={
            0: blocking(3, 0), 1: blocking(2, 1),
        })
        self.assertEqual(orch.run(), 2)
        self.assertEqual(orch.state.phase, "BLOCKED_CONVERGENCE")
        self.assertIn("absolute cap", orch.state.blocked_reason)

    def test_redesign_blocks_as_an_architecture_terminal(self):
        orch = self._orch(verdicts_by_wave={
            0: {"groups": [group(verdict="REDESIGN")]},
            1: {"groups": [group(verdict="REDESIGN")]},
        })
        self.assertEqual(orch.run(), 2)
        self.assertEqual(orch.state.phase, "BLOCKED_ARCHITECTURE")
        report = (Path(orch.state.run_dir) / "report.md").read_text()
        self.assertIn("### Diagnosis", report)
        self.assertIn("blocking-group trajectory: 1 → 1", report)
        self.assertIn("Next:", report)

    def test_failed_gate_has_its_own_terminal(self):
        orch = self._orch(gates=["false"])
        self.assertEqual(orch.run(), 2)
        self.assertEqual(orch.state.phase, "BLOCKED_GATE")

    # ---------------------------------------------------------------- polish

    def test_polish_findings_neither_block_nor_cost_a_wave(self):
        orch = self._orch(verdicts_by_wave={0: {"groups": [
            group(defect_class="doc_drift", owns="docs/**"),
            group(root_cause="rc2", defect_class="evidence_gap",
                  owns="docs/**"),
        ]}})
        self.assertEqual(orch.run(), 0)
        self.assertEqual(orch.state.phase, "READY")
        self.assertEqual(orch.state.wave, 0)
        self.assertEqual(orch.state.blocking_history, [0])
        self.assertTrue(orch.state.polish_done)
        self.assertIn("cleared", orch.state.polish_detail)
        # The polish pass was delivered with the candidate.
        self.assertTrue((self.repo / "docs" / "echo-polish-w0.md").is_file())

    def test_polish_failure_does_not_block_delivery(self):
        orch = self._orch(polish_mode="fail", verdicts_by_wave={
            0: {"groups": [group(defect_class="doc_drift", owns="docs/**")]}})
        self.assertEqual(orch.run(), 0)
        self.assertEqual(orch.state.phase, "READY")
        self.assertIn("polish pass failed", orch.state.polish_detail)

    def test_polish_writing_outside_its_owns_is_discarded(self):
        orch = self._orch(polish_mode="rogue", verdicts_by_wave={
            0: {"groups": [group(defect_class="doc_drift", owns="docs/**")]}})
        self.assertEqual(orch.run(), 0)
        self.assertEqual(orch.state.phase, "READY")
        self.assertIn("discarded", orch.state.polish_detail)
        self.assertFalse((self.repo / "rogue.txt").exists())

    def test_polish_breaking_the_gates_is_discarded(self):
        orch = self._orch(
            gates=["test ! -f docs/echo-polish-w0.md"],
            verdicts_by_wave={0: {"groups": [
                group(defect_class="doc_drift", owns="docs/**")]}})
        self.assertEqual(orch.run(), 0)
        self.assertEqual(orch.state.phase, "READY")
        self.assertIn("gates failed", orch.state.polish_detail)
        self.assertFalse((self.repo / "docs").exists())

    def test_polish_survives_the_fix_waves_that_ignored_it(self):
        # A doc drift raised at wave 0 is in no fix lane; it must still be
        # cleared before delivery.
        orch = self._orch(verdicts_by_wave={
            0: {"groups": [group(root_cause="code-rc"),
                           group(root_cause="doc-rc",
                                 defect_class="doc_drift", owns="docs/**")]},
            1: {"groups": []},
        })
        self.assertEqual(orch.run(), 0)
        self.assertEqual(orch.state.phase, "READY")
        self.assertEqual(orch.state.wave, 1)
        self.assertIn("cleared", orch.state.polish_detail)
        self.assertTrue((self.repo / "docs" / "echo-polish-w1.md").is_file())


class ResumeTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.repo = helpers.make_git_repo(self.dir / "repo")
        self.config = helpers.write_echo_config(self.dir / "echo.toml")

    def test_re_judging_a_wave_overwrites_its_own_round(self):
        # A run that dies between JUDGE and the transition re-judges the same
        # wave on --resume: that must not look like a stalled extra round.
        mission = helpers.write_mission(self.dir / "m.md", self.repo)
        verdicts_by_wave = {0: blocking(3, 0), 1: {"groups": []}}
        orch = ScriptedOrchestrator(
            tool_dir=helpers.TOOL_DIR, mission_path=mission,
            config_path=self.config, auto=True, log=lambda _: None,
            verdicts_by_wave=verdicts_by_wave)
        while orch.state.phase != "JUDGE":
            getattr(orch, {
                "INIT": "_phase_init", "PLAN": "_phase_plan",
                "PLAN_CHECKPOINT": "_phase_plan_checkpoint",
                "IMPLEMENT": "_phase_implement", "INSPECT": "_phase_inspect",
                "INTEGRATE": "_phase_integrate", "GATES": "_phase_gates",
                "REVIEW": "_phase_review"}[orch.state.phase])()
        orch._phase_judge()  # first judgment of wave 0
        orch.state.phase = "JUDGE"  # crash before the transition took effect
        orch.state.wave = 0
        orch._phase_judge()  # resume: judge wave 0 again
        self.assertEqual(orch.state.blocking_history, [3])
        self.assertEqual(len(orch.state.judgments), 1)
        self.assertEqual(len(orch.state.reviews), 1)


class ReviewerMemoryTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = Path(self.tmp.name)
        self.repo = helpers.make_git_repo(self.dir / "repo")
        self.config = helpers.write_echo_config(self.dir / "echo.toml")

    def test_fix_wave_reviewer_capsule_carries_the_previous_rounds(self):
        mission = helpers.write_mission(self.dir / "m.md", self.repo)
        orch = ScriptedOrchestrator(
            tool_dir=helpers.TOOL_DIR, mission_path=mission,
            config_path=self.config, auto=True, log=lambda _: None,
            verdicts_by_wave={
                0: {"groups": [group(root_cause="accepted-rc"),
                               group(root_cause="deferred-rc",
                                     defect_class="doc_drift",
                                     owns="docs/**"),
                               group(root_cause="dismissed-rc",
                                     verdict="DISMISS")]},
                1: {"groups": []},
            })
        self.assertEqual(orch.run(), 0)
        capsule = (Path(orch.state.run_dir) / "capsules"
                   / "reviewer-w1.md").read_text()
        self.assertIn("accepted-rc", capsule)
        self.assertIn("dismissed-rc", capsule)
        self.assertIn("do not re-litigate", capsule)
        self.assertIn("Review discipline", capsule)
        # A deferred polish finding is not presented as already fixed.
        fixed_section = capsule.split("## Findings already accepted and "
                                      "deferred")[0]
        self.assertNotIn("deferred-rc", fixed_section)
        self.assertIn("deferred-rc", capsule)
        # Wave 0 had no history to carry.
        first = (Path(orch.state.run_dir) / "capsules"
                 / "reviewer-w0.md").read_text()
        self.assertNotIn("already accepted", first)


if __name__ == "__main__":
    unittest.main()
