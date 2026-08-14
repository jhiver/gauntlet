"""echo harness: deterministic fake for tests and --dry-run.

Copies the capsule into the output file and emits a canned valid
gauntlet-report / gauntlet-verdict (NO_CLAIMS) / gauntlet-plan block, so the
extraction path for any role finds a valid block (extractors take the LAST
matching block of the requested kind, so emitting all three is safe).

When invoked with write=True on a lane capsule (which carries machine-readable
`lane-id:` / `lane-owns:` / `lane-tests:` / `wave:` lines), echo also creates
one small file inside the lane's first owned path. This makes the full loop
(INSPECT diff, commit, merge, deliver) exercisable end-to-end without a real
LLM. If the worktree does not exist (e.g. --dry-run), no file is written.
"""
import json
import re
from pathlib import Path

from src.adapters.base import FailureKind, HarnessAdapter, RunResult
from src.worktrees import static_prefix

_LANE_ID_RE = re.compile(r"^lane-id:\s*(\S+)\s*$", re.M)
_LANE_OWNS_RE = re.compile(r"^lane-owns:\s*(\[.*\])\s*$", re.M)
_LANE_TESTS_RE = re.compile(r"^lane-tests:\s*(\[.*\])\s*$", re.M)
_WAVE_RE = re.compile(r"^wave:\s*(\d+)\s*$", re.M)


def _json_list(pattern: re.Pattern, text: str) -> list[str]:
    m = pattern.search(text)
    if not m:
        return []
    try:
        value = json.loads(m.group(1))
    except json.JSONDecodeError:
        return []
    return [str(v) for v in value] if isinstance(value, list) else []


class EchoAdapter(HarnessAdapter):
    supports_write = True

    def __init__(self, name: str = "echo", cfg: dict | None = None):
        super().__init__(name, cfg)
        self._counter = 0

    def run(self, *, capsule: Path, worktree: Path, write: bool,
            model: str | None, effort: str | None,
            hard_timeout_s: int, idle_timeout_s: int | None,
            out_dir: Path, **kwargs) -> RunResult:
        text = Path(capsule).read_text(encoding="utf-8")
        changed: list[str] = []
        if write and Path(worktree).is_dir():
            owns = _json_list(_LANE_OWNS_RE, text)
            if owns:
                lane_m = _LANE_ID_RE.search(text)
                wave_m = _WAVE_RE.search(text)
                lane_id = lane_m.group(1) if lane_m else "lane"
                wave = wave_m.group(1) if wave_m else "0"
                rel_dir = static_prefix(owns[0])
                rel = f"{rel_dir + '/' if rel_dir else ''}echo-{lane_id}-w{wave}.md"
                target = Path(worktree) / rel
                target.parent.mkdir(parents=True, exist_ok=True)
                target.write_text(
                    f"echo harness deterministic write: lane {lane_id}, "
                    f"wave {wave}\n", encoding="utf-8")
                changed.append(rel)
        tests = _json_list(_LANE_TESTS_RE, text)
        report = {
            "files_changed": changed,
            "tests_run": tests,
            "tests_passed": True,
            "partial": False,
            "notes": "echo harness: no real work performed",
        }
        verdict = {"groups": []}
        plan = {"lanes": [{
            "id": "E1",
            "owns": ["**"],
            "forbidden": [],
            "tests": ["true"],
            "brief": "echo harness canned plan lane",
            "addresses": [],
        }]}
        out_dir = Path(out_dir)
        out_dir.mkdir(parents=True, exist_ok=True)
        self._counter += 1
        out_path = out_dir / f"{Path(capsule).stem}-echo-{self._counter}.out"
        out_path.write_text(
            text
            + "\n\n```gauntlet-report\n" + json.dumps(report) + "\n```\n"
            + "\n```gauntlet-verdict\n" + json.dumps(verdict) + "\n```\n"
            + "\n```gauntlet-plan\n" + json.dumps(plan) + "\n```\n",
            encoding="utf-8")
        return RunResult(FailureKind.NONE, 0, out_path, "echo harness ok")

    def describe(self, *, capsule: Path, worktree: Path, write: bool,
                 model: str | None, effort: str | None) -> str:
        return f"echo harness (capsule={capsule}, worktree={worktree})"
