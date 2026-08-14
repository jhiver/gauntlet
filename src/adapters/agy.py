"""agy harness: wraps the antigravity-delegation launcher script.

See DESIGN.md "Concrete harness commands". Short prompt + capsule path,
never an inline capsule (long inline prompts fail silently on agy).
"""
import os
from pathlib import Path

from src.adapters.base import SubprocessAdapter

_DEFAULT_LAUNCHER = ("~/aios/.reasonix/skills/antigravity-delegation"
                     "/scripts/agy-delegate")
_COMPLEXITY = {"low": "low", "medium": "medium", "high": "high", "max": "high"}


class AgyAdapter(SubprocessAdapter):
    def __init__(self, name: str, cfg: dict | None = None):
        super().__init__(name, cfg)
        cfg = cfg or {}
        self.launcher = os.path.expanduser(cfg.get("launcher", _DEFAULT_LAUNCHER))

    def build_argv(self, *, capsule: Path, worktree: Path, write: bool,
                   model: str | None, effort: str | None) -> list[str]:
        argv = ["bash", self.launcher,
                "--kind", "implement" if write else "review",
                "--complexity", _COMPLEXITY.get(effort or "", "medium"),
                "--mission", str(capsule),
                "--cwd", str(worktree)]
        if write:
            argv.append("--write")
        chosen = model or self.default_model
        if chosen:
            argv += ["--model", chosen]
        return argv
