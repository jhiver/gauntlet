"""Phases, transitions, state.json persistence.

state.json is rewritten after every phase transition and every lane status
change (atomic write). --resume reloads it and re-enters at the recorded
phase; phase handlers are written to be re-entrant.
"""
from __future__ import annotations

import json
import os
from dataclasses import asdict, dataclass, field
from pathlib import Path

PHASES = [
    "INIT", "PLAN", "PLAN_CHECKPOINT", "STAGES", "IMPLEMENT", "INSPECT", "INTEGRATE",
    "GATES", "REVIEW", "JUDGE", "PLAN_FIX", "POLISH", "DELIVER_CHECKPOINT",
    "DELIVER",
    "READY", "READY_NO_CHANGE",
    "BLOCKED", "BLOCKED_CONVERGENCE", "BLOCKED_ARCHITECTURE", "BLOCKED_GATE",
    "BLOCKED_HARNESS",
]
# Every blocked terminal means the same thing: a human decision is required,
# and the diagnosis says which one. They are distinguished because the recovery
# playbook differs (re-scope vs redesign vs fix the gate vs fix harness access).
BLOCKED_TERMINALS = {"BLOCKED", "BLOCKED_CONVERGENCE", "BLOCKED_ARCHITECTURE",
                     "BLOCKED_GATE", "BLOCKED_HARNESS"}
TERMINALS = {"READY", "READY_NO_CHANGE"} | BLOCKED_TERMINALS

# Lane status: pending -> done -> integrated; side states: failed, rejected.
LANE_ACTIVE = {"pending", "failed"}

# Convergence verdicts for the fix-wave gate.
CONVERGING = "converging"   # strictly fewer blocking groups than ever before
STALLED = "stalled"         # count did not beat the historical minimum
CAPPED = "capped"           # absolute safety cap on fix waves reached


def convergence_state(history: list[int], count: int, *, wave: int,
                      max_total_waves: int) -> str:
    """Decide whether another fix wave is justified.

    `history` holds the blocking-group count of every previous judgment,
    `count` the current one. A wave is granted while the mission converges —
    each round must beat the best round so far, so an oscillation
    (7 -> 4 -> 5) counts as stalled, not as progress.
    """
    if wave >= max_total_waves:
        return CAPPED
    if history and count >= min(history):
        return STALLED
    return CONVERGING


@dataclass
class LaneState:
    id: str
    owns: list[str]
    forbidden: list[str] = field(default_factory=list)
    tests: list[str] = field(default_factory=list)
    brief: str = ""
    addresses: list[str] = field(default_factory=list)
    status: str = "pending"
    detail: str = ""
    changed: list[str] = field(default_factory=list)
    claimed: list[str] = field(default_factory=list)


@dataclass
class State:
    run_id: str = ""
    slug: str = ""
    phase: str = "INIT"
    wave: int = 0
    repo: str = ""
    target_branch: str = "main"
    gates: list[str] = field(default_factory=list)
    base_commit: str = ""
    run_dir: str | None = None
    lanes: list[LaneState] = field(default_factory=list)
    stages: list[dict] = field(default_factory=list)
    harness_health: dict = field(default_factory=dict)
    reviews: list[str] = field(default_factory=list)
    judgments: list[str] = field(default_factory=list)
    worktrees: list[str] = field(default_factory=list)
    branches: list[str] = field(default_factory=list)
    integrated_changes: bool = False
    blocking_history: list[int] = field(default_factory=list)
    polish_done: bool = False
    polish_detail: str = ""
    blocked_reason: str | None = None
    blocked_kind: str | None = None
    blocked_phase: str | None = None
    auto: bool = False
    dry_run: bool = False

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> "State":
        data = dict(data)
        data["lanes"] = [LaneState(**lane) for lane in data.get("lanes", [])]
        known = {f for f in cls.__dataclass_fields__}
        return cls(**{k: v for k, v in data.items() if k in known})


def save(state: State) -> Path:
    """Atomic rewrite of state.json inside the run directory."""
    if not state.run_dir:
        raise ValueError("state.run_dir is not set")
    path = Path(state.run_dir) / "state.json"
    tmp = path.with_suffix(".json.tmp")
    tmp.write_text(json.dumps(state.to_dict(), indent=2) + "\n",
                   encoding="utf-8")
    os.replace(tmp, path)
    return path


def load(run_dir) -> State:
    path = Path(run_dir) / "state.json"
    data = json.loads(path.read_text(encoding="utf-8"))
    state = State.from_dict(data)
    state.run_dir = str(run_dir)
    return state
