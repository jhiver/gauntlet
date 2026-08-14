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
 "contract_ids": ["AC-1"], "verdict": "FIX", "class": "code_defect",
 "fix": "...", "owns": "src/path.py"}]}
```
verdict is one of FIX | REDESIGN | REPORT_ONLY | DISMISS.
class is one of code_defect | doc_drift | evidence_gap (default code_defect).
`owns` is the path or glob the correction belongs to.
An empty review is {"groups": []}. Only use contract IDs listed above."""

_PLAN_FORMAT = """\
End your output with exactly ONE fenced block:

### PRIORITY 1: Orthogonal Parallel Lanes (preferred for speed and concurrency)
Use this whenever tasks can be isolated into non-overlapping file sets (`owns`):
```gauntlet-plan
{"lanes": [
  {"id": "L1", "owns": ["src/auth/**"], "forbidden": [], "tests": ["npm test"], "brief": "Implement authentication", "addresses": ["AC-1"]},
  {"id": "L2", "owns": ["src/ui/**"], "forbidden": [], "tests": ["npm test"], "brief": "Implement UI components", "addresses": ["AC-2"]}
]}
```
(Lane owns globs must be pairwise non-overlapping and non-empty.)

### PRIORITY 2: Sequential Stage Pipeline (for multi-step or non-parallelizable missions)
Use this whenever tasks have shared files, overlapping state, causal dependencies, or sequential steps (do NOT lump everything into 1 single monolithic lane):
```gauntlet-stages
{"stages": [
  {"slug": "01-core-types", "brief": "Define schemas and types", "owns": ["src/types/**"], "contract_ids": ["AC-1"]},
  {"slug": "02-engine", "brief": "Implement engine logic using the new types", "owns": ["src/engine/**"], "contract_ids": ["AC-2"]}
]}
```"""

# Protocol section 3: the reviewer prompt starts with this text verbatim.
_REVIEWER_STANCE = """\
You are a senior dev doing the code review before these changes get committed to git and you HATE what you are seeing... What would you criticize? What edge cases am I missing?

Remember Antoine de Saint-Exupéry “Perfection is achieved, not when there is nothing more to add, but when there is nothing left to take away.”.

Your proposed solutions need to bring robustness through simplification and elegance, not over-engineered bloat."""

# Protocol section 3bis: what makes a claim admissible. The criticality filter
# lives here as well as in the judge rule — a reviewer that never raises an
# out-of-scope claim costs no round-trip to dismiss it.
_REVIEW_DISCIPLINE = """\
## Review discipline

- Scope: a claim is admissible only if the flagged behavior violates a clause
  of the contract above. That contract also grants ALLOWANCES — explicit
  permissions, reuse grants, non-goals. Never claim against a pattern the
  contract explicitly allows or a non-goal excludes; such claims are dismissed
  on citation alone.
- Criticality: crash windows, fault injection, rare races, and double
  failures are `REPORT_ONLY` unless the contract explicitly targets recovery,
  fault tolerance, or concurrency. A real defect outside the mission is
  `REPORT_ONLY`, never `FIX`.
- Class: tag every group. `code_defect` is behavior that is wrong.
  `doc_drift` is documentation left inconsistent with the code. `evidence_gap`
  is a missing or unreproducible proof artifact. The last two never block
  delivery on their own — they are collected for a single polish pass — so do
  not inflate them into code defects to force attention.
- Evidence: every claim cites the `file:line` you actually read and the
  contract ID it violates. No citation, no claim.
- Proportion: prefer deleting or reverting the machinery that causes a
  concern over adding a compensating layer."""

# Protocol section 4: batch judgment rule and boundaries.
_JUDGE_RULE = """\
For every root-cause group, evaluate: (1) justified — the defect exists on a
concrete supported path; (2) aligned — it affects an AC, an INV, the central
objective, or an ordinary-path regression introduced by the candidate;
(3) critical — delivery would otherwise cause concrete security/safety
failure, irreversible data loss, production outage, or central goal failure;
(4) simplifying — the smallest correction removes net code, state, branches,
dependencies, or concepts; (5) equivalent — it preserves supported behavior;
(6) proportionate — risk reduction clearly outweighs every new concept and
failure mode; (7) local — the correction stays inside the owning abstraction.

Action rule:
FIX = justified AND aligned AND (
  (simplifying AND equivalent)
  OR (critical AND proportionate AND local)
)
REDESIGN: justified, aligned, critical defect whose smallest additive patch
is not proportionate or local. REPORT_ONLY: real but non-actionable concern.
DISMISS: invalid, stale, duplicate, or already-covered claim.

Boundaries: broad words like "robust" or "safe" do not expand the mission;
crash windows, fault injection, rare races, and double failures are
non-critical unless the mission targets recovery, fault tolerance, or
concurrency; a critical defect outside the mission stays report-only; new
locks, queues, retries, timers, durable state, protocol phases, or
cross-component coordination are presumed REDESIGN unless they replace more
complexity than they add; when review-created machinery causes a concern,
delete, revert, or replace its parent — never add a compensating layer.

Two powers are yours alone:
- Dismissal on citation: if the contract clause cited by a claim in fact
  allows the flagged behavior, or a non-goal excludes it, the group is
  DISMISS — no further debate, whatever its wording.
- Demotion: a real defect whose class is out of the mission's scope, or whose
  criticality does not justify a fix now, is demoted FIX -> REPORT_ONLY.
Set each group's class (code_defect | doc_drift | evidence_gap) yourself; the
reviewer's tag is a proposal. Only code_defect and REDESIGN groups hold up
delivery, so classify honestly rather than to force or avoid a fix wave."""


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


def _history_section(fixed=None, deferred=None, dismissed=None) -> list[str]:
    """Rounds already fought. Without it every fresh reviewer re-samples the
    defect distribution from zero instead of verifying the previous fixes."""
    parts: list[str] = []
    for title, entries in (
        ("Findings already accepted and fixed (verify, do not re-litigate)",
         fixed),
        ("Findings already accepted and deferred to the final polish pass "
         "(do not raise them again)", deferred),
        ("Findings already dismissed (do not re-open without NEW evidence)",
         dismissed),
    ):
        if entries:
            parts += [f"## {title}", "",
                      *[f"- {entry}" for entry in entries], ""]
    return parts


def reviewer(mission, *, wave: int, run_id: str, diff_path: str | None = None,
             fixed=None, deferred=None, dismissed=None) -> str:
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
    if wave:
        instructions.append(
            f"This is fix wave {wave}: your first job is to verify that the "
            "findings listed below were actually fixed, and only then to look "
            "for defects the previous rounds missed.")
    return "\n".join([
        f"# Gauntlet capsule — role: reviewer — run: {run_id} — wave: {wave}",
        "",
        _REVIEWER_STANCE,
        "",
        SAFETY,
        _contract_section(mission),
        *_history_section(fixed, deferred, dismissed),
        _REVIEW_DISCIPLINE,
        "",
        "## Instructions",
        "",
        *instructions,
        "",
        "## Expected output",
        "",
        _VERDICT_FORMAT,
    ]) + "\n"


def judge(mission, *, wave: int, run_id: str, review_json: str,
          deferred=None, dismissed=None) -> str:
    return "\n".join([
        f"# Gauntlet capsule — role: judge — run: {run_id} — wave: {wave}",
        "",
        SAFETY,
        _contract_section(mission),
        *_history_section(deferred=deferred, dismissed=dismissed),
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
        _JUDGE_RULE,
        "",
        "## Expected output",
        "",
        _VERDICT_FORMAT,
    ]) + "\n"


def polish(mission, groups, *, wave: int, run_id: str, owns=None) -> str:
    """Single pre-delivery pass over the non-blocking findings.

    It runs in the integration worktree, after the gates went green and after
    the last judgment found nothing blocking: documentation drift and evidence
    gaps get corrected without costing a fix wave or a review round.
    """
    owns = list(owns or [])
    parts = [
        f"# Gauntlet capsule — role: polish — run: {run_id} — wave: {wave}",
        "",
        SAFETY,
        _contract_section(mission),
        "## Non-blocking findings to clear",
        "",
    ]
    for group in groups:
        parts.append(f"- [{group.defect_class}] {group.root_cause}")
        if group.fix:
            parts.append(f"  fix: {group.fix}")
        if group.owns:
            parts.append(f"  owns: {group.owns}")
    if owns:
        parts += [
            "",
            "lane-id: polish",
            f"lane-owns: {json.dumps(owns)}",
            "lane-tests: []",
            f"wave: {wave}",
        ]
    parts += [
        "",
        "## Instructions",
        "",
        "Correct exactly these findings in the current worktree — nothing",
        "else. They are documentation drift and evidence gaps, not behavior:",
        "do not change program behavior, do not refactor, do not add",
        "machinery. Touch only the paths listed above. Do not run git.",
        "The repository gates run again on your result; if they fail, your",
        "whole pass is discarded and the candidate ships without it.",
        "",
        "## Expected output",
        "",
        _REPORT_FORMAT,
    ]
    return "\n".join(parts) + "\n"


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
            "## Instructions & Decision Framework",
            "",
            "1. **PRIORITY 1: Orthogonal Parallel Lanes (`gauntlet-plan`)**",
            "   - If subtasks can be decoupled with disjoint file sets (`owns`), output >= 2 parallel lanes (`gauntlet-plan`).",
            "   - Parallel lanes execute concurrently in isolated worktrees.",
            "",
            "2. **PRIORITY 2: Sequential Stage Pipeline (`gauntlet-stages`)**",
            "   - If subtasks have shared files, overlapping state, causal dependencies, or sequential steps (e.g. Step 1 -> Step 2 -> ...), DO NOT lump everything into a single massive lane.",
            "   - Instead, decompose into sequential stages (`gauntlet-stages`) so each phase executes incrementally through the full Gauntlet loop on top of verified previous output.",
            "   - A single monolithic lane containing an entire multi-step project is strictly forbidden when sequential stages can be created.",
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
