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
    "INIT", "PLAN", "PLAN_CHECKPOINT", "IMPLEMENT", "INSPECT", "INTEGRATE",
    "GATES", "REVIEW", "JUDGE", "PLAN_FIX", "DELIVER_CHECKPOINT", "DELIVER",
    "READY", "READY_NO_CHANGE", "BLOCKED",
]
TERMINALS = {"READY", "READY_NO_CHANGE", "BLOCKED"}

# Lane status: pending -> done -> integrated; side states: failed, rejected.
LANE_ACTIVE = {"pending", "failed"}


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
    harness_health: dict = field(default_factory=dict)
    reviews: list[str] = field(default_factory=list)
    judgments: list[str] = field(default_factory=list)
    worktrees: list[str] = field(default_factory=list)
    branches: list[str] = field(default_factory=list)
    integrated_changes: bool = False
    blocked_reason: str | None = None
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
