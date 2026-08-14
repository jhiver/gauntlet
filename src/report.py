"""Compact report.md maintenance: one short section per phase."""
from __future__ import annotations

import time
from pathlib import Path


class Report:
    def __init__(self, path, *, title: str = "Gauntlet run report"):
        self.path = Path(path)
        if not self.path.exists():
            self.path.write_text(f"# {title}\n", encoding="utf-8")

    def _append(self, text: str) -> None:
        with open(self.path, "a", encoding="utf-8") as fh:
            fh.write(text)

    def section(self, title: str, body: str = "") -> None:
        stamp = time.strftime("%H:%M:%S")
        self._append(f"\n## {title} ({stamp})\n\n{body.rstrip()}\n")

    def line(self, text: str) -> None:
        self._append(f"- {text}\n")
