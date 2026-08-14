"""reasonix harness: reasonix CLI.

Uses `reasonix -p --output-format stream-json`: the stream carries the
actual content (`kind:"text"` events + a final `type:"result"` with a
`result` field). Do NOT use `reasonix run --events-jsonl` for output
capture — that stream is REDACTED (kind markers only, no content).

Read-only roles get --allowed-tools deny rules: the reviewer reads the diff
file and worktree files, it needs no shell, write, or git tool.
"""
from pathlib import Path

from src.adapters.base import SubprocessAdapter


class ReasonixAdapter(SubprocessAdapter):
    jsonl_output = True  # stream-json emits JSONL

    def build_argv(self, *, capsule: Path, worktree: Path, write: bool,
                   model: str | None, effort: str | None) -> list[str]:
        argv = ["reasonix", "-p",
                f"Execute the mission file at {capsule} and follow it exactly.",
                "--output-format", "stream-json"]
        chosen = model or self.default_model
        if chosen:
            argv += ["--model", chosen]
        if effort:
            argv += ["--effort", effort]
        if not write:
            argv += ["--allowed-tools", "deny:write,deny:bash,deny:git"]
        return argv
