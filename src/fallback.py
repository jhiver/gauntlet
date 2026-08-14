"""Chain executor: retry policy + run-level circuit breaker.

Every role chain implicitly ends with the "human" link. Failure -> policy
action mapping (DESIGN.md "fallback" section):

- on_quota      next_and_break          open the harness breaker, next link
- on_auth       break                   open the breaker and abort the task
- on_rate_limit backoff_retry_then_next backoff 30s, 1 retry, then next link
- on_timeout    retry_once_then_next
- on_crash      retry_once_then_next    (PARTIAL_DELIVERY follows on_crash)
- on_invalid_output retry_once_then_next

Beyond max_attempts_per_task: human checkpoint (if one is provided); in
non-interactive mode the chain is simply exhausted.
"""
from __future__ import annotations

import threading
import time
from dataclasses import dataclass

from src.adapters.base import FailureKind, RunResult


class ChainExhausted(Exception):
    pass


class AuthAbort(Exception):
    pass


FAILURE_POLICY_KEY = {
    FailureKind.QUOTA_EXHAUSTED: "on_quota",
    FailureKind.AUTH_EXPIRED: "on_auth",
    FailureKind.RATE_LIMITED: "on_rate_limit",
    FailureKind.TIMEOUT_IDLE: "on_timeout",
    FailureKind.TIMEOUT_HARD: "on_timeout",
    FailureKind.CRASH: "on_crash",
    FailureKind.PARTIAL_DELIVERY: "on_crash",
    FailureKind.OUTPUT_INVALID: "on_invalid_output",
}


class HarnessHealth:
    """Run-level circuit breakers (persisted in state.json, thread-safe)."""

    def __init__(self, initial: dict | None = None):
        self._states = dict(initial or {})
        self._lock = threading.Lock()

    def is_open(self, name: str) -> bool:
        with self._lock:
            return self._states.get(name) == "open"

    def open(self, name: str) -> None:
        with self._lock:
            self._states[name] = "open"

    def close(self, name: str) -> None:
        """First success after a trip restores the harness to 'ok'."""
        with self._lock:
            self._states[name] = "ok"

    def snapshot(self) -> dict:
        with self._lock:
            return dict(self._states)


@dataclass
class ChainOutcome:
    result: RunResult
    harness: str
    attempts: int


def _short(text: str, limit: int = 120) -> str:
    text = " ".join((text or "").split())
    return text if len(text) <= limit else text[: limit - 1] + "…"


def execute_chain(*, role: str, links: list[dict], health: HarnessHealth,
                  policy: dict, run_once, validate=None, auto: bool = False,
                  checkpoint=None, backoff_s: float | None = None,
                  log=lambda msg: None) -> ChainOutcome:
    """Run `run_once(link, attempt)` down the chain until one attempt both
    succeeds and passes `validate(result) -> (FailureKind|None, detail)`.

    Raises ChainExhausted when no link delivers, AuthAbort on on_auth="break".
    """
    max_attempts = policy.get("max_attempts_per_task", 3)
    if backoff_s is None:
        backoff_s = policy.get("backoff_s", 30)
    chain = list(links) + [{"harness": "human"}]  # implicit terminal link
    attempts = 0
    for link in chain:
        hname = link.get("harness", "?")
        if hname == "human" and auto:
            log(f"[{role}] skipping human link in --auto mode")
            continue
        if hname != "human" and health.is_open(hname):
            log(f"[{role}] harness '{hname}' circuit breaker open; skipping")
            continue
        rate_retried = False
        generic_retried = False
        while True:
            if attempts >= max_attempts:
                if checkpoint is not None and checkpoint(
                        f"role '{role}': {attempts} attempts exhausted; "
                        "approve to keep trying the remaining chain"):
                    attempts = 0
                else:
                    raise ChainExhausted(
                        f"{role}: exhausted after {attempts} attempts")
            attempts += 1
            result = run_once(link, attempts)
            if result.failure == FailureKind.NONE and validate is not None:
                vfailure, detail = validate(result)
                if vfailure is not None:
                    result = RunResult(vfailure, result.exit_code,
                                       result.output_path, detail)
            if result.failure == FailureKind.NONE:
                if hname != "human":
                    health.close(hname)
                return ChainOutcome(result, hname, attempts)
            log(f"[{role}] {hname} attempt {attempts}: "
                f"{result.failure.value} — {_short(result.detail)}")
            action = policy.get(FAILURE_POLICY_KEY[result.failure],
                                "retry_once_then_next")
            if action == "break":
                if hname != "human":
                    health.open(hname)
                raise AuthAbort(
                    f"{role}/{hname}: {result.failure.value}: "
                    f"{_short(result.detail)}")
            if action == "next_and_break":
                if hname != "human":
                    health.open(hname)
                break
            if action == "backoff_retry_then_next" and not rate_retried:
                rate_retried = True
                time.sleep(backoff_s)
                continue
            if action == "retry_once_then_next" and not generic_retried:
                generic_retried = True
                continue
            break  # next link
    raise ChainExhausted(f"{role}: chain exhausted")
