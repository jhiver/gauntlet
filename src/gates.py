"""Run gate commands in the integration worktree.

The orchestrator alone runs gate commands. Each command runs via bash with
output captured under out_dir; with dry_run=True commands are printed instead
of executed.
"""
from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path


@dataclass
class GateResult:
    command: str
    ok: bool
    returncode: int | None
    log_path: Path | None
    detail: str = ""


def run_gates(commands: list[str], *, cwd, out_dir, dry_run: bool = False,
              log=print, timeout_s: int = 600) -> list[GateResult]:
    import time
    from src.ui import default_ui
    results: list[GateResult] = []
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    total = len(commands)
    for i, command in enumerate(commands, 1):
        if dry_run:
            log(f"DRY-RUN: (cd {cwd} && {command})")
            results.append(GateResult(command, True, None, None))
            continue
        log_path = out_dir / f"gate-{i-1}.log"
        t0 = time.monotonic()
        try:
            proc = subprocess.run(
                ["bash", "-c", command], cwd=str(cwd),
                capture_output=True, text=True, timeout=timeout_s)
            dur = time.monotonic() - t0
            log_path.write_text(proc.stdout + proc.stderr, encoding="utf-8")
            ok = proc.returncode == 0
            detail = (proc.stderr.strip().splitlines()[-1]
                      if not ok and proc.stderr.strip() else "")
            default_ui.gate_result(i, total, command, ok, dur, detail)
            results.append(GateResult(
                command, ok, proc.returncode, log_path, detail=detail))
        except subprocess.TimeoutExpired:
            dur = time.monotonic() - t0
            log_path.write_text(f"gate timed out after {timeout_s}s\n",
                                encoding="utf-8")
            detail = f"timeout after {timeout_s}s"
            default_ui.gate_result(i, total, command, False, dur, detail)
            results.append(GateResult(command, False, None, log_path, detail=detail))
    return results


def all_ok(results: list[GateResult]) -> bool:
    return all(r.ok for r in results)
