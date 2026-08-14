import io
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from src.adapters.base import FailureKind, HarnessAdapter, RunResult  # noqa: E402
from src.fallback import (AuthAbort, ChainExhausted, HarnessHealth,  # noqa: E402
                          execute_chain)

POLICY = {
    "on_quota": "next_and_break",
    "on_auth": "break",
    "on_rate_limit": "backoff_retry_then_next",
    "on_timeout": "retry_once_then_next",
    "on_crash": "retry_once_then_next",
    "on_invalid_output": "retry_once_then_next",
    "max_attempts_per_task": 3,
    "backoff_s": 0,
}


class MockAdapter(HarnessAdapter):
    """Scripted harness: pops one FailureKind per call, NONE when empty."""

    def __init__(self, name, script):
        super().__init__(name, {"supports_write": True})
        self.script = list(script)
        self.calls = 0

    def run(self, *, capsule, worktree, write, model, effort,
            hard_timeout_s, idle_timeout_s, out_dir):
        self.calls += 1
        kind = self.script.pop(0) if self.script else FailureKind.NONE
        out_dir = Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        out_path = out_dir / f"mock-{self.name}-{self.calls}.out"
        out_path.write_text("ok" if kind == FailureKind.NONE else "bad",
                            encoding="utf-8")
        return RunResult(kind, 0 if kind == FailureKind.NONE else 1,
                         out_path, f"scripted {kind.value}")


class FallbackMatrixTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out_dir = Path(self.tmp.name)
        self.logs = []

    def _execute(self, adapters, links, health=None, validate=None,
                 auto=True, checkpoint=None):
        def run_once(link, attempt):
            from src.adapters.human import HumanAdapter
            adapter = adapters.get(link["harness"]) or HumanAdapter("human")
            return adapter.run(
                capsule=Path("capsule.md"), worktree=self.out_dir,
                write=False, model=None, effort=None, hard_timeout_s=5,
                idle_timeout_s=None, out_dir=self.out_dir)
        return execute_chain(
            role="tester", links=links,
            health=health or HarnessHealth(), policy=POLICY,
            run_once=run_once, validate=validate, auto=auto,
            checkpoint=checkpoint, log=self.logs.append)

    def test_quota_opens_breaker_and_moves_to_next_link(self):
        a = MockAdapter("a", [FailureKind.QUOTA_EXHAUSTED])
        b = MockAdapter("b", [])
        health = HarnessHealth()
        outcome = self._execute({"a": a, "b": b},
                                [{"harness": "a"}, {"harness": "b"}],
                                health=health)
        self.assertEqual(outcome.harness, "b")
        self.assertTrue(health.is_open("a"))
        self.assertEqual(a.calls, 1)

    def test_open_breaker_skips_harness_on_later_task(self):
        a = MockAdapter("a", [FailureKind.QUOTA_EXHAUSTED])
        b = MockAdapter("b", [])
        health = HarnessHealth()
        links = [{"harness": "a"}, {"harness": "b"}]
        self._execute({"a": a, "b": b}, links, health=health)
        self._execute({"a": a, "b": b}, links, health=health)
        self.assertEqual(a.calls, 1)  # second task never touched 'a'
        self.assertEqual(b.calls, 2)

    def test_success_closes_breaker(self):
        health = HarnessHealth({"a": "open"})
        a = MockAdapter("a", [FailureKind.NONE])
        # breaker open -> 'a' skipped entirely; prove close() via direct run
        b = MockAdapter("b", [FailureKind.NONE])
        outcome = self._execute({"b": b}, [{"harness": "b"}], health=health)
        self.assertEqual(outcome.harness, "b")
        self.assertFalse(health.is_open("b"))

    def test_auth_aborts_without_draining_chain(self):
        a = MockAdapter("a", [FailureKind.AUTH_EXPIRED])
        b = MockAdapter("b", [])
        health = HarnessHealth()
        with self.assertRaises(AuthAbort):
            self._execute({"a": a, "b": b},
                          [{"harness": "a"}, {"harness": "b"}], health=health)
        self.assertEqual(b.calls, 0)  # no chain drain
        self.assertTrue(health.is_open("a"))

    def test_timeout_retries_once_on_same_link(self):
        a = MockAdapter("a", [FailureKind.TIMEOUT_HARD, FailureKind.NONE])
        outcome = self._execute({"a": a}, [{"harness": "a"}])
        self.assertEqual(outcome.harness, "a")
        self.assertEqual(a.calls, 2)
        self.assertEqual(outcome.attempts, 2)

    def test_timeout_then_next_link_after_one_retry(self):
        a = MockAdapter("a", [FailureKind.TIMEOUT_IDLE,
                              FailureKind.TIMEOUT_HARD])
        b = MockAdapter("b", [])
        outcome = self._execute({"a": a, "b": b},
                                [{"harness": "a"}, {"harness": "b"}])
        self.assertEqual(outcome.harness, "b")
        self.assertEqual(a.calls, 2)

    def test_rate_limit_backoff_retry_then_next(self):
        a = MockAdapter("a", [FailureKind.RATE_LIMITED, FailureKind.NONE])
        outcome = self._execute({"a": a}, [{"harness": "a"}])
        self.assertEqual(outcome.harness, "a")
        self.assertEqual(a.calls, 2)

    def test_invalid_output_retries_once_then_next(self):
        a = MockAdapter("a", [FailureKind.NONE])
        b = MockAdapter("b", [])
        calls = {"n": 0}

        def validate(result):
            calls["n"] += 1
            if calls["n"] <= 2:  # invalidate both attempts on 'a'
                return FailureKind.OUTPUT_INVALID, "bad block"
            return None, ""

        outcome = self._execute({"a": a, "b": b},
                                [{"harness": "a"}, {"harness": "b"}],
                                validate=validate)
        self.assertEqual(outcome.harness, "b")
        self.assertEqual(a.calls, 2)

    def test_chain_exhaustion_falls_through_to_human(self):
        a = MockAdapter("a", [FailureKind.CRASH, FailureKind.CRASH])
        stdin = sys.stdin
        sys.stdin = io.StringIO("approve\n")
        try:
            outcome = self._execute({"a": a}, [{"harness": "a"}], auto=False)
        finally:
            sys.stdin = stdin
        self.assertEqual(outcome.harness, "human")

    def test_human_eof_exhausts_chain(self):
        a = MockAdapter("a", [FailureKind.CRASH, FailureKind.CRASH])
        stdin = sys.stdin
        sys.stdin = io.StringIO("")  # piped stdin at EOF
        try:
            with self.assertRaises(ChainExhausted):
                self._execute({"a": a}, [{"harness": "a"}], auto=False)
        finally:
            sys.stdin = stdin

    def test_auto_mode_skips_human_link(self):
        a = MockAdapter("a", [FailureKind.CRASH, FailureKind.CRASH])
        with self.assertRaises(ChainExhausted):
            self._execute({"a": a}, [{"harness": "a"}], auto=True)

    def test_max_attempts_triggers_chain_exhaustion(self):
        a = MockAdapter("a", [FailureKind.CRASH, FailureKind.CRASH])
        b = MockAdapter("b", [FailureKind.CRASH])
        # attempts: 2 on 'a' (crash + one retry), 1 on 'b' -> cap hit.
        with self.assertRaises(ChainExhausted):
            self._execute({"a": a, "b": b},
                          [{"harness": "a"}, {"harness": "b"}], auto=True)
        self.assertEqual(b.calls, 1)

    def test_checkpoint_approval_resets_attempt_budget(self):
        a = MockAdapter("a", [FailureKind.CRASH, FailureKind.CRASH])
        b = MockAdapter("b", [])
        outcome = self._execute({"a": a, "b": b},
                                [{"harness": "a"}, {"harness": "b"}],
                                auto=True, checkpoint=lambda msg: True)
        self.assertEqual(outcome.harness, "b")


if __name__ == "__main__":
    unittest.main()


class ModelUnavailableTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.out_dir = Path(self.tmp.name)

    def test_model_unavailable_goes_next_without_breaker_or_retry(self):
        # A 403 MODEL_NOT_IN_PLAN is model-scoped: retrying is pointless and
        # tripping the harness breaker would kill other models on the same CLI.
        policy = dict(POLICY, on_model_unavailable="next")
        a = MockAdapter("a", [FailureKind.MODEL_UNAVAILABLE])
        b = MockAdapter("b", [])
        health = HarnessHealth()

        def run_once(link, attempt):
            adapter = {"a": a, "b": b}[link["harness"]]
            return adapter.run(
                capsule=Path("capsule.md"), worktree=self.out_dir,
                write=False, model=None, effort=None, hard_timeout_s=5,
                idle_timeout_s=None, out_dir=self.out_dir)

        outcome = execute_chain(
            role="tester", links=[{"harness": "a"}, {"harness": "b"}],
            health=health, policy=policy, run_once=run_once, auto=True)
        self.assertEqual(outcome.harness, "b")
        self.assertEqual(a.calls, 1)          # no retry
        self.assertFalse(health.is_open("a"))  # no breaker
