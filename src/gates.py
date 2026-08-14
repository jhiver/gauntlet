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
    results: list[GateResult] = []
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    for i, command in enumerate(commands):
        if dry_run:
            log(f"DRY-RUN: (cd {cwd} && {command})")
            results.append(GateResult(command, True, None, None))
            continue
        log_path = out_dir / f"gate-{i}.log"
        try:
            proc = subprocess.run(
                ["bash", "-c", command], cwd=str(cwd),
                capture_output=True, text=True, timeout=timeout_s)
            log_path.write_text(proc.stdout + proc.stderr, encoding="utf-8")
            results.append(GateResult(
                command, proc.returncode == 0, proc.returncode, log_path,
                detail=proc.stderr.strip().splitlines()[-1]
                if proc.returncode != 0 and proc.stderr.strip() else ""))
        except subprocess.TimeoutExpired:
            log_path.write_text(f"gate timed out after {timeout_s}s\n",
                                encoding="utf-8")
            results.append(GateResult(command, False, None, log_path,
                                      detail=f"timeout after {timeout_s}s"))
    return results


def all_ok(results: list[GateResult]) -> bool:
    return all(r.ok for r in results)
