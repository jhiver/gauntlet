"""Per-role capsule rendering.

Every capsule embeds the safety rules (DESIGN.md "Safety"), the immutable
root contract, and the exact fenced output block the role must produce.
Lane capsules additionally carry machine-readable `lane-id:` / `lane-owns:` /
`lane-tests:` / `wave:` lines (used by the echo harness).
"""
from __future__ import annotations

import json

SAFETY = """## Safety (mandatory, non-negotiable)

- Never read, display, copy, or process `.env` files or any secrets.
- No network writes to external services or systems.
- No git mutations: the orchestrator owns all git operations.
- Do not launch other agents.
- No production actions, no destructive actions.
- Write only inside the paths your lane owns.
"""

_REPORT_FORMAT = """\
End your report with exactly one fenced block:
```gauntlet-report
{"files_changed": ["..."], "tests_run": ["..."], "tests_passed": true,
 "partial": false, "notes": ""}
```
Set "partial": true if you could not complete the lane."""

_VERDICT_FORMAT = """\
End your output with exactly one fenced block:
```gauntlet-verdict
{"groups": [{"root_cause": "...", "claims": ["..."],
 "contract_ids": ["AC-1"], "verdict": "FIX", "fix": "...",
 "owns": "src/path.py"}]}
```
verdict is one of FIX | REDESIGN | REPORT_ONLY | DISMISS.
An empty review is {"groups": []}. Only use contract IDs listed above."""

_PLAN_FORMAT = """\
End your output with exactly one fenced block:
```gauntlet-plan
{"lanes": [{"id": "F1", "owns": ["src/auth/**"], "forbidden": [],
 "tests": ["..."], "brief": "...", "addresses": ["<root_cause>"]}]}
```
Lane owns globs must be pairwise non-overlapping and non-empty."""


def _contract_section(mission) -> str:
    ids = "\n".join(f"- {cid}" for cid in sorted(mission.contract_ids)) or "(none)"
    return (
        "## Root contract (immutable)\n\n"
        + mission.body.strip()
        + "\n\n## Contract IDs (valid for verdict contract_ids)\n\n" + ids + "\n")


def implementer(mission, lane, *, wave: int, run_id: str,
                role: str = "implementer", fix_groups=None) -> str:
    parts = [
        f"# Gauntlet capsule — role: {role} — run: {run_id} — wave: {wave}",
        "",
        SAFETY,
        _contract_section(mission),
        "## Your lane",
        "",
        f"lane-id: {lane.id}",
        f"lane-owns: {json.dumps(lane.owns)}",
        f"lane-forbidden: {json.dumps(lane.forbidden)}",
        f"lane-tests: {json.dumps(lane.tests)}",
        f"wave: {wave}",
        "",
        f"brief: {lane.brief}",
    ]
    if lane.addresses:
        parts += ["", "addresses: " + json.dumps(lane.addresses)]
    if fix_groups:
        parts += ["", "## Accepted findings to fix", ""]
        for group in fix_groups:
            parts.append(f"- root cause: {group.root_cause}")
            if group.fix:
                parts.append(f"  fix: {group.fix}")
    parts += [
        "",
        "## Instructions",
        "",
        "Implement the lane brief inside the current worktree. Touch only the",
        "paths your lane owns; never touch a forbidden path. Run the lane",
        "tests. Do not run git.",
        "",
        "## Expected output",
        "",
        _REPORT_FORMAT,
    ]
    return "\n".join(parts) + "\n"


def reviewer(mission, *, wave: int, run_id: str,
             diff_path: str | None = None) -> str:
    instructions = [
        "Review the integrated changes in this worktree against the contract.",
    ]
    if diff_path:
        instructions.append(
            f"The full base-to-candidate diff is at {diff_path} — read it "
            "first; consult worktree files for surrounding context.")
    instructions += [
        "You run READ-ONLY: do not modify any file. One global review of the",
        "whole diff; report only defects backed by evidence, each mapped to a",
        "contract ID where applicable. Style nitpicks and out-of-mission",
        "findings are not FIX claims.",
    ]
    return "\n".join([
        f"# Gauntlet capsule — role: reviewer — run: {run_id} — wave: {wave}",
        "",
        SAFETY,
        _contract_section(mission),
        "## Instructions",
        "",
        *instructions,
        "",
        "## Expected output",
        "",
        _VERDICT_FORMAT,
    ]) + "\n"


def judge(mission, *, wave: int, run_id: str, review_json: str) -> str:
    return "\n".join([
        f"# Gauntlet capsule — role: judge — run: {run_id} — wave: {wave}",
        "",
        SAFETY,
        _contract_section(mission),
        "## Reviewer claims to judge",
        "",
        "```json",
        review_json.strip(),
        "```",
        "",
        "## Instructions",
        "",
        "You run READ-ONLY. Judge all reviewer claims together against the",
        "contract: deduplicate them by root cause, dismiss style nitpicks and",
        "out-of-mission findings, and emit the final grouped verdict.",
        "",
        "## Expected output",
        "",
        _VERDICT_FORMAT,
    ]) + "\n"


def planner(mission, *, run_id: str, groups=None, complaint: str | None = None) -> str:
    parts = [
        f"# Gauntlet capsule — role: planner — run: {run_id}",
        "",
        SAFETY,
        _contract_section(mission),
    ]
    if groups:
        parts += [
            "## Accepted findings to address (fix-wave recut)", ""]
        for group in groups:
            parts.append(f"- {group.root_cause} (verdict {group.verdict})")
        parts += [
            "",
            "Cut fix lanes addressing these root causes. Each lane's",
            "'addresses' lists the root causes it fixes.",
            "",
        ]
    else:
        parts += [
            "## Instructions",
            "",
            "Cut the mission into orthogonal implementation lanes: disjoint",
            "owns globs, no shared invariants, any integration order, each",
            "independently revertable.",
            "",
        ]
    if complaint:
        parts += [
            "## Previous attempt rejected",
            "",
            complaint,
            "",
        ]
    parts += ["## Expected output", "", _PLAN_FORMAT]
    return "\n".join(parts) + "\n"


def checkpoint(name: str, context: str) -> str:
    return "\n".join([
        f"# Gauntlet director checkpoint: {name}",
        "",
        "You are the mission director. Review the summary below, then reply",
        "with 'approve' or 'reject' on its own line.",
        "",
        context,
    ]) + "\n"
