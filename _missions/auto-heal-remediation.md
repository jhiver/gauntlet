+++
slug = "auto-heal-remediation"

[[repos]]
path = "."
target_branch = "main"
gates = [
  "cargo check",
  "cargo clippy --all-targets -- -D warnings",
  "cargo test"
]

[[lanes]]
id = "L1"
owns = ["src/config.rs", "src/fallback.rs", "src/statemachine.rs"]
forbidden = ["src/orchestrator.rs", "src/capsules.rs", "tests/**"]
tests = ["cargo test --test test_checks", "cargo test --test test_state"]
brief = "Add on_blocked policy enum, auto_heal_budget, and state machine transitions for auto-remediation."

[[lanes]]
id = "L2"
owns = ["src/orchestrator.rs", "src/capsules.rs", "tests/test_auto_heal.rs"]
forbidden = ["src/config.rs", "src/fallback.rs", "src/statemachine.rs"]
tests = ["cargo test --test test_auto_heal", "cargo test --test test_loop"]
brief = "Implement gate failure auto-fix synthesis, unowned file pruning, director AI triage fallback, and auto-heal orchestrator loop."
+++

# Mission Objective: Autonomous Auto-Heal & Blocker Self-Remediation

Transform Gauntlet's mechanical terminal blocked states (`BLOCKED_GATE`, `BLOCKED_SAFETY`, `BLOCKED_CONVERGENCE`, `BLOCKED_ARCHITECTURE`) into autonomous self-healing recovery loops bounded by a strict attempt budget.

## Acceptance Criteria (AC)

- **AC-1 (Configuration & Policy)**:
  - `PolicyConfig` supports `on_blocked: String` (`"halt"`, `"auto_heal"`, `"director"` - default: `"auto_heal"`) and `auto_heal_budget: usize` (default: `2`).
  - Validation ensures `on_blocked` rejects unknown policy strings.

- **AC-2 (Gate Failure Auto-Fix)**:
  - When mechanical gates fail under `on_blocked = "auto_heal"`, the orchestrator synthesizes a fix finding group (`contract_id = "GATES"`, `class = "CODE_DEFECT"`, `verdict = "FIX"`, `root_cause = "<compiler/test error output>"`) and triggers an automatic fix wave without human intervention, up to `auto_heal_budget` attempts.

- **AC-3 (Safety Drift Auto-Prune)**:
  - When a safety inspection detects unowned modified or created files outside lane `owns`, under `auto_heal`, the orchestrator mechanically prunes/reverts unowned paths (`git checkout -- <unowned>` / `rm -f <untracked>`) and re-runs containment checks.

- **AC-4 (Director AI / Supervisor Fallback Triage)**:
  - When `on_blocked = "director"`, or when `auto_heal` exhausts its budget, the orchestrator queries `roles.director` (which can be backed by an AI model like `gpt-5.6-sol` / `claude-sonnet-5` or human fallback) with a diagnostic capsule before terminating.

- **AC-5 (State Machine & History Tracking)**:
  - `state.json` tracks `auto_heal_attempts: usize` and records remediation transitions clearly in `report.md`.

## Invariants (INV) - Inviolable Rules

- **INV-1**: Zero `unwrap()`, `expect()`, or `panic!` in non-test Rust code. All errors use typed `Result` and `?`.
- **INV-2**: Mutex locks recover from poisoned states cleanly (`.unwrap_or_else(|p| p.into_inner())`).
- **INV-3**: Strict convergence and loop bounding: auto-healing cannot exceed `auto_heal_budget` to mathematically prevent infinite retry loops.
- **INV-4**: All 136 existing tests + new auto-heal unit and integration tests must pass cleanly.
- **INV-5**: 0 Clippy warnings (`cargo clippy --all-targets -- -D warnings`).

## Non-Goals (NG)

- **NG-1**: Arbitrary unrestricted retries (the budget must strictly govern all auto-healing actions).
- **NG-2**: Bypassing gate validation (the final delivered branch must still satisfy 100% of all mechanical gates).
