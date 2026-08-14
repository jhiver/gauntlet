"""Shared fixtures for Gauntlet tests: tmp git repos, missions, configs.

Git mutations happen only inside per-test tempdirs, never in a real repo.
"""
import json
import subprocess
import sys
from pathlib import Path

TOOL_DIR = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(TOOL_DIR))


def git(repo: Path, *args: str) -> str:
    proc = subprocess.run(["git", *args], cwd=repo, capture_output=True,
                          text=True)
    assert proc.returncode == 0, f"git {args}: {proc.stderr}"
    return proc.stdout


def make_git_repo(path: Path) -> Path:
    path.mkdir(parents=True, exist_ok=True)
    git(path, "init", "-b", "main")
    git(path, "config", "user.email", "test@example.com")
    git(path, "config", "user.name", "Test")
    (path / "README.md").write_text("# fixture repo\n", encoding="utf-8")
    git(path, "add", "-A")
    git(path, "commit", "-m", "init")
    return path


MISSION_TEMPLATE = """+++
slug = "{slug}"

[[repos]]
path = "{repo}"
target_branch = "main"
gates = {gates}

{lanes}+++

# Objective

Test mission.

## AC

- AC-1: The example lane exists and its tests pass.

## INV

- INV-1: No file outside the lane owns is modified.

## NG

- NG-1: No public API change.
"""

LANE_TEMPLATE = """[[lanes]]
id = "{lid}"
owns = [{owns}]
forbidden = [{forbidden}]
tests = ["true"]
brief = "Lane {lid} brief."

"""


def write_mission(path: Path, repo: Path, *, slug: str = "example",
                  lanes: list[dict] | None = None,
                  gates: list[str] | None = None) -> Path:
    if lanes is None:
        lanes = [{"lid": "L1", "owns": '"src/example/**"', "forbidden": ""}]
    lanes_toml = "".join(
        LANE_TEMPLATE.format(lid=l["lid"], owns=l["owns"],
                             forbidden=l.get("forbidden", ""))
        for l in lanes)
    gates_toml = "[" + ", ".join(
        json.dumps(g) for g in (gates if gates is not None else ["true"])) + "]"
    path.write_text(
        MISSION_TEMPLATE.format(slug=slug, repo=repo, lanes=lanes_toml,
                                gates=gates_toml),
        encoding="utf-8")
    return path


ECHO_CONFIG = """# Test config: every role resolved by the echo harness.
[roles.implementer]
chain = [ { harness = "echo" } ]
[roles.fixer]
chain = [ { harness = "echo" } ]
[roles.reviewer]
chain = [ { harness = "echo" } ]
[roles.judge]
chain = [ { harness = "echo" } ]
[roles.planner]
chain = [ { harness = "echo" } ]
[roles.director]
chain = [ { harness = "human" } ]

[policy]
checkpoints = []
max_total_waves = 5
on_wave_cap = "checkpoint"
idle_timeout_s = 5
hard_timeout_s = 30
lane_timeout_s = 30

[fallback]
on_quota = "next_and_break"
on_auth = "break"
on_rate_limit = "backoff_retry_then_next"
on_timeout = "retry_once_then_next"
on_crash = "retry_once_then_next"
on_invalid_output = "retry_once_then_next"
max_attempts_per_task = 3
backoff_s = 0
"""


def write_echo_config(path: Path) -> Path:
    path.write_text(ECHO_CONFIG, encoding="utf-8")
    return path
