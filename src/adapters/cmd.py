"""cmd harness: commandcode.ai CLI.

See DESIGN.md "Concrete harness commands". Do NOT use `cmd -w/--worktree`:
the orchestrator owns worktrees.
"""
from pathlib import Path

from src.adapters.base import SubprocessAdapter


class CmdAdapter(SubprocessAdapter):
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
        # Reviewers/judges run read-only; only write roles get --auto-accept.
        argv += ["--auto-accept"] if write else ["--permission-mode", "plan"]
        return argv
