"""Harness adapter interface and shared subprocess machinery.

See DESIGN.md section "Adapter interface". run() is blocking; the
orchestrator runs lanes in threads (one per lane).
"""
from __future__ import annotations

import re
import shlex
import subprocess
import time
from abc import ABC, abstractmethod
from dataclasses import dataclass
from enum import Enum
from pathlib import Path

_STDERR_TAIL = 4000


class FailureKind(Enum):
    NONE = "none"
    QUOTA_EXHAUSTED = "quota"
    RATE_LIMITED = "rate_limit"
    AUTH_EXPIRED = "auth"
    MODEL_UNAVAILABLE = "model_unavailable"  # model not in plan / removed
    TIMEOUT_IDLE = "timeout_idle"
    TIMEOUT_HARD = "timeout_hard"
    CRASH = "crash"
    PARTIAL_DELIVERY = "partial"
    OUTPUT_INVALID = "invalid_output"


@dataclass
class RunResult:
    failure: FailureKind
    exit_code: int | None
    output_path: Path  # captured stdout (final report lives here)
    detail: str = ""   # stderr tail / classification reason


class HarnessAdapter(ABC):
    name: str = "?"
    supports_write: bool = False

    def __init__(self, name: str, cfg: dict | None = None):
        cfg = cfg or {}
        self.name = name
        self.supports_write = bool(cfg.get("supports_write", self.supports_write))

    @abstractmethod
    def run(self, *, capsule: Path, worktree: Path, write: bool,
            model: str | None, effort: str | None,
            hard_timeout_s: int, idle_timeout_s: int | None,
            out_dir: Path, **kwargs) -> RunResult:
        """Run the harness once, blocking, and classify the outcome."""

    def describe(self, *, capsule: Path, worktree: Path, write: bool,
                 model: str | None, effort: str | None) -> str:
        """Human-readable preview of what run() would do (used by --dry-run)."""
        return f"<{self.name} harness>"


class SubprocessAdapter(HarnessAdapter):
    """Base for CLI harnesses: builds argv, tees stdout/stderr to files under
    out_dir, enforces hard/idle timeouts (idle = no stdout mtime change), and
    classifies failures from configured stderr regexes, then exit codes.

    Adapters whose harness emits a JSONL event stream (jsonl_output = True)
    get their stdout post-processed after exit: the raw stream is kept in
    <stem>.raw and <stem>.out receives every string payload extracted from
    the JSON lines, so fenced gauntlet-* blocks embedded in escaped JSON
    strings become literal and parseable.
    """

    jsonl_output = False

    def __init__(self, name: str, cfg: dict | None = None):
        super().__init__(name, cfg)
        cfg = cfg or {}
        self.default_model = cfg.get("default_model")
        pats = cfg.get("errors") or {}
        self._compiled = {
            FailureKind.QUOTA_EXHAUSTED: self._compile(pats.get("quota", [])),
            FailureKind.AUTH_EXPIRED: self._compile(pats.get("auth", [])),
            FailureKind.RATE_LIMITED: self._compile(pats.get("rate_limit", [])),
            FailureKind.MODEL_UNAVAILABLE:
                self._compile(pats.get("model_unavailable", [])),
        }
        self._counter = 0

    @staticmethod
    def _compile(patterns):
        return [re.compile(p, re.IGNORECASE) for p in patterns]

    def build_argv(self, *, capsule: Path, worktree: Path, write: bool,
                   model: str | None, effort: str | None) -> list[str]:
        raise NotImplementedError

    def describe(self, *, capsule: Path, worktree: Path, write: bool,
                 model: str | None, effort: str | None) -> str:
        argv = self.build_argv(capsule=capsule, worktree=worktree,
                               write=write, model=model, effort=effort)
        return f"(cd {worktree} && {shlex.join(argv)})"

    def run(self, *, capsule: Path, worktree: Path, write: bool,
            model: str | None, effort: str | None,
            hard_timeout_s: int, idle_timeout_s: int | None,
            out_dir: Path, role: str = "", lane_id: str | None = None) -> RunResult:
        argv = self.build_argv(capsule=capsule, worktree=worktree, write=write,
                               model=model, effort=effort)
        out_dir = Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        self._counter += 1
        stem = f"{Path(capsule).stem}-{self.name}-{self._counter}"
        out_path = out_dir / f"{stem}.out"
        err_path = out_dir / f"{stem}.err"
        start_time = time.monotonic()
        try:
            with open(out_path, "wb") as out, open(err_path, "wb") as err:
                proc = subprocess.Popen(
                    argv, cwd=str(worktree), stdin=subprocess.DEVNULL,
                    stdout=out, stderr=err)
                timeout = self._watch(proc, out_path, hard_timeout_s,
                                      idle_timeout_s, role=role,
                                      harness=self.name, model=model or self.default_model,
                                      lane_id=lane_id)
        except FileNotFoundError as exc:
            out_path.write_text("")
            return RunResult(FailureKind.CRASH, None, out_path,
                             f"harness launch failed: {exc}")
        if self.jsonl_output:
            self._finalize_stream(out_path)
        if timeout is not None:
            return RunResult(timeout, None, out_path, self._tail(err_path))
        tail = self._tail(err_path)
        dur = time.monotonic() - start_time
        if proc.returncode == 0:
            return RunResult(FailureKind.NONE, 0, out_path, tail)
        classified = self._classify(tail)
        kind = classified if classified is not None else FailureKind.CRASH
        return RunResult(kind, proc.returncode, out_path,
                         tail or f"exit code {proc.returncode}")

    @classmethod
    def _watch(cls, proc, out_path: Path, hard_timeout_s: int,
               idle_timeout_s: int | None, role: str = "",
               harness: str = "", model: str | None = None,
               lane_id: str | None = None):
        """Poll the child; kill on hard deadline or idle stdout. Returns the
        FailureKind on kill, None when the child exited on its own."""
        deadline = time.monotonic() + hard_timeout_s
        start_time = time.monotonic()
        last_activity = time.monotonic()
        last_mtime = -1.0
        from src.ui import default_ui
        while proc.poll() is None:
            now = time.monotonic()
            elapsed = now - start_time
            if now >= deadline:
                proc.kill()
                proc.wait()
                default_ui.finish_ticker()
                return FailureKind.TIMEOUT_HARD
            file_bytes = 0
            if idle_timeout_s:
                try:
                    st = out_path.stat()
                    mtime = st.st_mtime
                    file_bytes = st.st_size
                except FileNotFoundError:
                    mtime = -1.0
                if mtime != last_mtime:
                    last_mtime = mtime
                    last_activity = now
                elif now - last_activity >= idle_timeout_s:
                    proc.kill()
                    proc.wait()
                    default_ui.finish_ticker()
                    return FailureKind.TIMEOUT_IDLE
            else:
                try:
                    file_bytes = out_path.stat().st_size
                except FileNotFoundError:
                    file_bytes = 0

            idle_s = now - last_activity
            default_ui.ticker(role=role or "task", harness=harness or getattr(cls, "name", "cli"),
                              model=model, lane_id=lane_id, elapsed_s=elapsed,
                              bytes_count=file_bytes, idle_s=idle_s)
            time.sleep(0.2)
        default_ui.finish_ticker()
        return None

    @staticmethod
    def _tail(path: Path, n: int = _STDERR_TAIL) -> str:
        try:
            data = Path(path).read_bytes()
        except FileNotFoundError:
            return ""
        return data[-n:].decode("utf-8", "replace")

    @staticmethod
    def _finalize_stream(out_path: Path) -> None:
        """Move the raw JSONL stream to <stem>.raw and rewrite <stem>.out as
        plain text: every string payload of each JSON line (document order),
        non-JSON lines passed through. Fenced gauntlet-* blocks travel inside
        escaped JSON strings (e.g. cmd's final `result.finalText`), so this
        makes them literal and extractable by verdicts.py."""
        import json
        raw_path = out_path.with_suffix(".raw")
        try:
            out_path.rename(raw_path)
        except FileNotFoundError:
            return

        def strings(node, sink):
            if isinstance(node, str):
                sink.append(node)
            elif isinstance(node, dict):
                for value in node.values():
                    strings(value, sink)
            elif isinstance(node, list):
                for item in node:
                    strings(item, sink)

        collected: list[str] = []
        with open(raw_path, encoding="utf-8", errors="replace") as fh:
            for line in fh:
                stripped = line.strip()
                if not stripped:
                    continue
                try:
                    node = json.loads(stripped)
                except json.JSONDecodeError:
                    collected.append(line.rstrip("\n"))
                else:
                    strings(node, collected)
        out_path.write_text("\n".join(collected) + "\n", encoding="utf-8")

    def _classify(self, stderr_tail: str):
        for kind, patterns in self._compiled.items():
            for pat in patterns:
                if pat.search(stderr_tail):
                    return kind
        return None
