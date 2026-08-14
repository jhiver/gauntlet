"""codex harness: OpenAI Codex CLI.

See DESIGN.md "Concrete harness commands".
"""
from pathlib import Path

from src.adapters.base import SubprocessAdapter


class CodexAdapter(SubprocessAdapter):
    jsonl_output = True  # --json emits a JSONL event stream

    def build_argv(self, *, capsule: Path, worktree: Path, write: bool,
                   model: str | None, effort: str | None) -> list[str]:
        argv = ["codex", "exec",
                f"Execute the mission file at {capsule} and follow it exactly.",
                "-C", str(worktree),
                "--json"]
        chosen = model or self.default_model
        if chosen:
            argv += ["-m", chosen]
        if effort:
            argv += ["-c", f"model_reasoning_effort={effort}"]
        if write:
            argv += ["--dangerously-bypass-approvals-and-sandbox"]
        else:
            argv += ["-s", "read-only"]
        return argv
