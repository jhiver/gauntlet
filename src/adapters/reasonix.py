"""reasonix harness: reasonix CLI.

See DESIGN.md "Concrete harness commands". Read-only is enforced via
--allowed-tools deny rules for non-write roles.
"""
from pathlib import Path

from src.adapters.base import SubprocessAdapter


class ReasonixAdapter(SubprocessAdapter):
    def build_argv(self, *, capsule: Path, worktree: Path, write: bool,
                   model: str | None, effort: str | None) -> list[str]:
        argv = ["reasonix", "run", "--events-jsonl"]
        chosen = model or self.default_model
        if chosen:
            argv += ["--model", chosen]
        if effort:
            argv += ["--effort", effort]
        if not write:
            argv += ["--allowed-tools", "deny:write,deny:bash,deny:git"]
        argv.append(f"Execute the mission file at {capsule} and follow it exactly.")
        return argv
