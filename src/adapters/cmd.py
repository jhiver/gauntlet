"""cmd harness: commandcode.ai CLI.

See DESIGN.md "Concrete harness commands". Do NOT use `cmd -w/--worktree`:
the orchestrator owns worktrees.
"""
from pathlib import Path

from src.adapters.base import SubprocessAdapter


class CmdAdapter(SubprocessAdapter):
    jsonl_output = True  # --output-format json emits an NDJSON event stream

    def build_argv(self, *, capsule: Path, worktree: Path, write: bool,
                   model: str | None, effort: str | None) -> list[str]:
        argv = ["cmd", "-p",
                f"Execute the mission file at {capsule} and follow it exactly.",
                "--no-session", "--skip-onboarding", "--no-auto-update",
                "--output-format", "json"]
        chosen = model or self.default_model
        if chosen:
            argv += ["--model", chosen]
        if effort:
            argv += ["--effort", effort]
        # In -p mode, tool use (file writes, shell) is gated unless --yolo;
        # --auto-accept is NOT sufficient. Reviewers/judges stay read-only
        # via plan mode.
        argv += ["--yolo"] if write else ["--permission-mode", "plan"]
        return argv
