"""Terminal UI & Live visibility engine for Gauntlet.

Provides modern, informative, styled terminal output with ANSI colors,
live execution timers/spinners, progress bars, and structured tables.
"""
from __future__ import annotations

import os
import sys
import time
from typing import Any


class UI:
    # ANSI Color Tokens
    RESET = "\033[0m"
    BOLD = "\033[1m"
    DIM = "\033[2m"
    ITALIC = "\033[3m"
    UNDERLINE = "\033[4m"

    # Foreground colors
    BLACK = "\033[30m"
    RED = "\033[31m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    BLUE = "\033[34m"
    MAGENTA = "\033[35m"
    CYAN = "\033[36m"
    WHITE = "\033[37m"

    # Bright foreground colors
    BRIGHT_RED = "\033[91m"
    BRIGHT_GREEN = "\033[92m"
    BRIGHT_YELLOW = "\033[93m"
    BRIGHT_BLUE = "\033[94m"
    BRIGHT_MAGENTA = "\033[95m"
    BRIGHT_CYAN = "\033[96m"
    BRIGHT_WHITE = "\033[97m"

    # Background colors
    BG_DARK = "\033[48;5;235m"
    BG_BLUE = "\033[44m"
    BG_GREEN = "\033[42m"
    BG_YELLOW = "\033[43m"
    BG_RED = "\033[41m"

    SPINNER_FRAMES = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]

    def __init__(self, stream=None, enable_color: bool | None = None):
        self.stream = stream or sys.stdout
        if enable_color is None:
            # Check NO_COLOR or non-tty
            no_color = os.environ.get("NO_COLOR") is not None
            is_tty = getattr(self.stream, "isatty", lambda: False)()
            self.enable_color = is_tty and not no_color
        else:
            self.enable_color = enable_color
        self._last_ticker_len = 0
        self._spinner_idx = 0
        self._last_non_ticker_time = time.monotonic()

    def _c(self, text: str, *codes: str) -> str:
        if not self.enable_color:
            return text
        return "".join(codes) + str(text) + self.RESET

    def print(self, *args, **kwargs):
        self._clear_ticker()
        print(*args, file=self.stream, **kwargs)
        self.stream.flush()

    def _clear_ticker(self):
        if self._last_ticker_len > 0 and self.enable_color:
            self.stream.write("\r" + " " * self._last_ticker_len + "\r")
            self.stream.flush()
            self._last_ticker_len = 0

    # ----------------------------------------------------------- Cards & Boxes

    def banner(self, title: str, subtitle: str | None = None,
               meta: dict[str, str] | None = None, width: int = 76):
        self._clear_ticker()
        c_border = self.CYAN
        c_title = self.BOLD + self.BRIGHT_WHITE
        c_sub = self.DIM + self.WHITE
        c_key = self.CYAN
        c_val = self.BRIGHT_CYAN

        top = self._c("╭" + "─" * (width - 2) + "╮", c_border)
        bot = self._c("╰" + "─" * (width - 2) + "╯", c_border)
        v = self._c("│", c_border)

        lines = [top]
        lines.append(f"{v} {self._c(title, c_title):<{width + len(self._c('', c_title)) - 4}} {v}")
        if subtitle:
            lines.append(f"{v} {self._c(subtitle, c_sub):<{width + len(self._c('', c_sub)) - 4}} {v}")
        if meta:
            lines.append(f"{v} " + " " * (width - 4) + f" {v}")
            for k, val in meta.items():
                entry = f"  • {self._c(k, c_key)}: {self._c(val, c_val)}"
                # Raw text length without ANSI codes for padding calculation
                raw_len = len(f"  • {k}: {val}")
                padding = max(0, width - 4 - raw_len)
                lines.append(f"{v} {entry}" + " " * padding + f" {v}")
        lines.append(bot)
        self.print("\n".join(lines))

    def phase_card(self, phase: str, wave: int = 0, detail: str | None = None,
                   width: int = 76):
        self._clear_ticker()
        phase_colors = {
            "INIT": self.BRIGHT_BLUE,
            "PLAN": self.BRIGHT_MAGENTA,
            "PLAN_CHECKPOINT": self.YELLOW,
            "STAGES": self.BRIGHT_BLUE,
            "IMPLEMENT": self.BRIGHT_CYAN,
            "INSPECT": self.CYAN,
            "INTEGRATE": self.BLUE,
            "GATES": self.YELLOW,
            "REVIEW": self.MAGENTA,
            "JUDGE": self.BRIGHT_MAGENTA,
            "PLAN_FIX": self.BRIGHT_YELLOW,
            "POLISH": self.GREEN,
            "DELIVER_CHECKPOINT": self.YELLOW,
            "DELIVER": self.BRIGHT_GREEN,
            "READY": self.BRIGHT_GREEN,
            "READY_NO_CHANGE": self.BRIGHT_GREEN,
            "BLOCKED": self.BRIGHT_RED,
        }
        color = phase_colors.get(phase, self.WHITE)
        wave_str = f" [Wave {wave}]" if wave > 0 or phase in ("IMPLEMENT", "PLAN_FIX", "REVIEW", "JUDGE") else ""
        icon_map = {
            "INIT": "⚙️ ", "PLAN": "🗺️ ", "PLAN_CHECKPOINT": "⏸️ ", "STAGES": "📦",
            "IMPLEMENT": "⚡", "INSPECT": "🔍", "INTEGRATE": "🔄",
            "GATES": "🛡️ ", "REVIEW": "🧐", "JUDGE": "⚖️ ",
            "PLAN_FIX": "🔧", "POLISH": "✨", "DELIVER_CHECKPOINT": "⏸️ ",
            "DELIVER": "🚀", "READY": "🎉", "READY_NO_CHANGE": "🎉",
        }
        icon = icon_map.get(phase, "◈")

        title = f"{icon}  PHASE: {phase}{wave_str}"
        c_line = self._c("─" * (width - len(title) - 4), self.DIM + self.WHITE)
        self.print(f"\n{self._c('╭──', color)} {self._c(title, self.BOLD, color)} {c_line}{self._c('╮', color)}")
        if detail:
            self.print(f"{self._c('│', color)}  {self._c(detail, self.DIM + self.WHITE)}")
            self.print(f"{self._c('╰', color)}{self._c('─' * (width - 2), color)}{self._c('╯', color)}")

    def stage_header(self, index: int, total: int, slug: str, brief: str = "",
                     width: int = 76):
        self._clear_ticker()
        badge = f"📦 STAGE [{index}/{total}]: {slug}"
        c_line = self._c("─" * max(2, width - len(badge) - 6), self.DIM + self.BRIGHT_BLUE)
        self.print(f"\n{self._c('╭──', self.BRIGHT_BLUE)} {self._c(badge, self.BOLD, self.BRIGHT_CYAN)} {c_line}{self._c('╮', self.BRIGHT_BLUE)}")
        if brief:
            self.print(f"{self._c('│', self.BRIGHT_BLUE)}  {self._c(brief, self.WHITE)}")
        self.print(f"{self._c('╰', self.BRIGHT_BLUE)}{self._c('─' * (width - 2), self.BRIGHT_BLUE)}{self._c('╯', self.BRIGHT_BLUE)}")

    # --------------------------------------------------------- Progress & Logs

    def step(self, label: str, message: str, detail: str = ""):
        self._clear_ticker()
        badge = self._c(f"[{label}]", self.BOLD, self.CYAN)
        det = f" {self._c(f'({detail})', self.DIM)}" if detail else ""
        self.print(f" {badge} {message}{det}")

    def success(self, message: str, detail: str = ""):
        self._clear_ticker()
        icon = self._c("✔", self.BOLD, self.BRIGHT_GREEN)
        det = f" {self._c(f'({detail})', self.DIM)}" if detail else ""
        self.print(f"  {icon} {message}{det}")

    def warning(self, message: str, detail: str = ""):
        self._clear_ticker()
        icon = self._c("⚠", self.BOLD, self.BRIGHT_YELLOW)
        det = f" {self._c(f'({detail})', self.DIM)}" if detail else ""
        self.print(f"  {icon} {self._c(message, self.YELLOW)}{det}")

    def error(self, message: str, detail: str = ""):
        self._clear_ticker()
        icon = self._c("✖", self.BOLD, self.BRIGHT_RED)
        det = f"\n    {self._c(detail, self.RED)}" if detail else ""
        self.print(f"  {icon} {self._c(message, self.BRIGHT_RED)}{det}")

    def gate_result(self, index: int, total: int, command: str, ok: bool,
                    duration_s: float, detail: str = ""):
        self._clear_ticker()
        idx_str = self._c(f"[{index}/{total}]", self.DIM)
        if ok:
            status = self._c("✔ PASS", self.BOLD, self.BRIGHT_GREEN)
            dur = self._c(f"({duration_s:.1f}s)", self.DIM)
            self.print(f"  {idx_str} {command:<48} {status} {dur}")
        else:
            status = self._c("✖ FAIL", self.BOLD, self.BRIGHT_RED)
            dur = self._c(f"({duration_s:.1f}s)", self.DIM)
            self.print(f"  {idx_str} {command:<48} {status} {dur}")
            if detail:
                self.print(f"      {self._c('↳ ' + detail, self.RED)}")

    # ------------------------------------------------------------- Live Ticker

    def ticker(self, role: str, harness: str, model: str | None,
               lane_id: str | None, elapsed_s: float, bytes_count: int = 0,
               idle_s: float | None = None, status_text: str = ""):
        if not self.enable_color:
            return
        frame = self.SPINNER_FRAMES[self._spinner_idx % len(self.SPINNER_FRAMES)]
        self._spinner_idx += 1

        mins, secs = divmod(int(elapsed_s), 60)
        time_str = f"{mins:02d}:{secs:02d}"

        harness_label = harness
        if model:
            harness_label += f":{model}"
        lane_str = f" [{lane_id}]" if lane_id else ""

        size_kb = bytes_count / 1024.0
        size_str = f"{size_kb:.1f} KB out" if size_kb > 0 else "starting"
        idle_str = f", active {int(idle_s)}s ago" if idle_s is not None and idle_s > 0 else ""

        line = (f"\r {self._c(frame, self.BOLD, self.BRIGHT_CYAN)} "
                f"{self._c(f'[{role.upper()}{lane_str}]', self.BOLD, self.CYAN)} "
                f"{self._c(harness_label, self.BRIGHT_WHITE)} • "
                f"{self._c(time_str, self.YELLOW)} • "
                f"{self._c(size_str + idle_str, self.DIM)}")
        if status_text:
            line += f" • {self._c(status_text, self.DIM)}"

        # Strip ANSI to measure visual length
        # Using fixed terminal width buffer
        vis_len = len(f" [{role.upper()}{lane_str}] {harness_label} • {time_str} • {size_str + idle_str}")
        padding = max(0, self._last_ticker_len - vis_len)
        self.stream.write(line + " " * padding)
        self.stream.flush()
        self._last_ticker_len = vis_len

    def finish_ticker(self, message: str = ""):
        if self._last_ticker_len > 0:
            self._clear_ticker()
            if message:
                self.success(message)

    # ---------------------------------------------------------- Verdicts Table

    def verdicts_table(self, groups: list[Any], width: int = 76):
        self._clear_ticker()
        if not groups:
            self.success("No claims or defects found in review (NO_CLAIMS). Candidate is clean.")
            return

        self.print(f"\n{self._c('╭─ ⚖️  JUDGMENT VERDICTS (' + str(len(groups)) + ' group(s)) ' + '─' * (width - 32) + '╮', self.MAGENTA)}")
        for idx, g in enumerate(groups, 1):
            v = getattr(g, "verdict", "UNKNOWN")
            v_badge_map = {
                "FIX": self._c(" FIX ", self.BOLD, self.WHITE, self.BG_RED),
                "REDESIGN": self._c(" REDESIGN ", self.BOLD, self.WHITE, self.BG_RED),
                "REPORT_ONLY": self._c(" REPORT_ONLY ", self.BOLD, self.WHITE, self.BG_BLUE),
                "DISMISS": self._c(" DISMISS ", self.DIM),
            }
            v_badge = v_badge_map.get(v, self._c(f" {v} ", self.BOLD))
            root = getattr(g, "root_cause", "")
            owns = getattr(g, "owns", "")
            cls_name = getattr(g, "class_", "code_defect")
            c_ids = ", ".join(getattr(g, "contract_ids", [])) or "None"

            self.print(f"{self._c('│', self.MAGENTA)} {self._c(f'#{idx}', self.BOLD)} {v_badge} {self._c(root, self.BOLD, self.WHITE)}")
            self.print(f"{self._c('│', self.MAGENTA)}    • Class: {self._c(cls_name, self.CYAN)} | Owner: {self._c(owns or 'N/A', self.YELLOW)} | Contract: {self._c(c_ids, self.DIM)}")
            fix = getattr(g, "fix", "")
            if fix:
                self.print(f"{self._c('│', self.MAGENTA)}    • Proposed fix: {self._c(fix, self.DIM)}")
            claims = getattr(g, "claims", [])
            for c in claims[:2]:
                self.print(f"{self._c('│', self.MAGENTA)}      - {self._c(c, self.DIM)}")
            if len(claims) > 2:
                self.print(f"{self._c('│', self.MAGENTA)}      - {self._c(f'... +{len(claims)-2} more claims', self.DIM)}")
        self.print(f"{self._c('╰' + '─' * (width - 2) + '╯', self.MAGENTA)}\n")


# Global default instance
default_ui = UI()
