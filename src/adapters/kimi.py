"""kimi harness: kimi CLI.

See DESIGN.md "Concrete harness commands". `-y` (auto-approve) only for
write roles; read-only roles get `--auto`.
"""
from pathlib import Path

from src.adapters.base import SubprocessAdapter


class KimiAdapter(SubprocessAdapter):
    jsonl_output = True  # --output-format stream-json emits JSONL

    def build_argv(self, *, capsule: Path, worktree: Path, write: bool,
                   model: str | None, effort: str | None) -> list[str]:
        argv = ["kimi", "-p",
                f"Execute the mission file at {capsule} and follow it exactly.",
                "--add-dir", str(worktree),
                "--output-format", "stream-json"]
        chosen = model or self.default_model
        if chosen:
            argv += ["-m", chosen]
        return argv
