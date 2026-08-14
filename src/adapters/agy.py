"""agy harness: wraps the antigravity-delegation launcher script.

See DESIGN.md "Concrete harness commands". Short prompt + capsule path,
never an inline capsule (long inline prompts fail silently on agy).

The capsule is staged INSIDE the lane worktree before invocation: the
launcher passes `--add-dir <capsule-dir>` to agy, and a capsule living in
the main checkout made agy anchor on (and write into) the main checkout
instead of the lane worktree (smoke test 2026-08-14). The staged copy is
removed after the run, before INSPECT diffs the worktree.
"""
import os
import shutil
from pathlib import Path

from src.adapters.base import RunResult, SubprocessAdapter

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

    def run(self, *, capsule: Path, worktree: Path, write: bool,
            model: str | None, effort: str | None,
            hard_timeout_s: int, idle_timeout_s: int | None,
            out_dir: Path) -> RunResult:
        worktree = Path(worktree)
        staged = worktree / ".gauntlet" / "capsule.md"
        try:
            staged.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(capsule, staged)
            return super().run(capsule=staged, worktree=worktree, write=write,
                               model=model, effort=effort,
                               hard_timeout_s=hard_timeout_s,
                               idle_timeout_s=idle_timeout_s, out_dir=out_dir)
        finally:
            shutil.rmtree(staged.parent, ignore_errors=True)

    def describe(self, *, capsule: Path, worktree: Path, write: bool,
                 model: str | None, effort: str | None) -> str:
        staged = Path(worktree) / ".gauntlet" / "capsule.md"
        argv = self.build_argv(capsule=staged, worktree=worktree, write=write,
                               model=model, effort=effort)
        import shlex
        return (f"stage capsule at {staged}; "
                f"(cd {worktree} && {shlex.join(argv)}); "
                f"remove {staged.parent}")
