"""human harness: interactive checkpoint on the terminal.

Prints the capsule path + what is expected, then waits on stdin for a
decision (`approve` / `reject` on their own line) or a pasted fenced
gauntlet-* block (kept reading until the closing fence). EOF without any
decision is a CRASH, so the adapter is non-interactive-safe: with piped or
closed stdin it returns immediately instead of blocking forever.
"""
import re
import sys
from pathlib import Path

from src.adapters.base import FailureKind, HarnessAdapter, RunResult

BLOCK_START_RE = re.compile(r"^```gauntlet-\w+\s*$", re.M)


def read_decision(output_path: Path) -> str | None:
    """Classify captured human input: 'approve', 'reject', 'output' (a pasted
    block), or None (nothing usable)."""
    try:
        text = Path(output_path).read_text(encoding="utf-8", errors="replace")
    except FileNotFoundError:
        return None
    for line in text.splitlines():
        stripped = line.strip()
        if stripped == "reject":
            return "reject"
        if stripped == "approve":
            return "approve"
    if BLOCK_START_RE.search(text):
        return "output"
    return None


class HumanAdapter(HarnessAdapter):
    supports_write = True

    def __init__(self, name: str = "human", cfg: dict | None = None):
        super().__init__(name, cfg)
        self._counter = 0

    def run(self, *, capsule: Path, worktree: Path, write: bool,
            model: str | None, effort: str | None,
            hard_timeout_s: int, idle_timeout_s: int | None,
            out_dir: Path) -> RunResult:
        if sys.stdin.isatty():
            print("=" * 72)
            print("HUMAN CHECKPOINT")
            print(f"  capsule : {capsule}")
            print(f"  worktree: {worktree}")
            print("  Read the capsule, perform or review the task, then")
            print("  either paste the required fenced gauntlet-* block, or")
            print("  type 'approve' / 'reject' on its own line.")
            print("  EOF (Ctrl-D) aborts this task.")
            print("=" * 72)
        collected: list[str] = []
        mode: str | None = None  # None | "block" | "decision"
        for line in sys.stdin:
            collected.append(line)
            stripped = line.strip()
            if mode == "block":
                if stripped == "```":
                    break
            elif BLOCK_START_RE.match(stripped):
                mode = "block"
            elif stripped in ("approve", "reject"):
                mode = "decision"
                break
        # Falling out of the loop means EOF on stdin.
        out_dir = Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        self._counter += 1
        out_path = out_dir / f"{Path(capsule).stem}-human-{self._counter}.out"
        out_path.write_text("".join(collected), encoding="utf-8")
        if mode is None:
            return RunResult(FailureKind.CRASH, None, out_path,
                             "no decision on stdin (EOF)")
        return RunResult(FailureKind.NONE, 0, out_path,
                         f"human input captured ({mode})")

    def describe(self, *, capsule: Path, worktree: Path, write: bool,
                 model: str | None, effort: str | None) -> str:
        return f"human checkpoint (capsule={capsule})"
