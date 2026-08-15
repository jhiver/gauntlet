+++
slug = "rust-port"

[[repos]]
path = "/Users/jhiver/aios/projects/gauntlet"
target_branch = "rust-port"
gates = [
  "cargo check --manifest-path rust/Cargo.toml",
  "cargo test --manifest-path rust/Cargo.toml",
  "cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings"
]

[[lanes]]
id = "L1"
owns = [
  "rust/Cargo.toml",
  "rust/src/lib.rs",
  "rust/src/config.rs",
  "rust/src/mission.rs",
  "rust/src/statemachine.rs",
  "rust/src/verdicts.rs",
  "rust/src/report.rs"
]
forbidden = ["src/**", "tests/**", "DESIGN.md"]
tests = ["cargo test --manifest-path rust/Cargo.toml --lib"]
brief = "Implement core data models, TOML/Markdown parsing, block extraction, atomic state machine & convergence logic in Rust."

[[lanes]]
id = "L2"
owns = [
  "rust/src/worktrees.rs",
  "rust/src/gates.rs",
  "rust/src/fallback.rs"
]
forbidden = ["src/**", "tests/**", "DESIGN.md"]
tests = ["cargo test --manifest-path rust/Cargo.toml --lib"]
brief = "Implement Git worktrees management, mechanical containment/drift/overlap checks, gate runners, and fallback chain executor with circuit breakers in Rust."

[[lanes]]
id = "L3"
owns = [
  "rust/src/capsules.rs",
  "rust/src/adapters/**"
]
forbidden = ["src/**", "tests/**", "DESIGN.md"]
tests = ["cargo test --manifest-path rust/Cargo.toml --lib"]
brief = "Implement prompt capsule rendering (all roles + history) and CLI subprocess adapters (agy, codex, cmd, kimi, reasonix, human, echo) with JSONL event unwrapping in Rust."

[[lanes]]
id = "L4"
owns = [
  "rust/src/orchestrator.rs",
  "rust/src/autoroute.rs",
  "rust/src/ui.rs",
  "rust/src/cli.rs",
  "rust/src/main.rs",
  "rust/tests/**"
]
forbidden = ["src/**", "tests/**", "DESIGN.md"]
tests = ["cargo test --manifest-path rust/Cargo.toml"]
brief = "Implement CLI entry point, ANSI terminal UI with live tickers, Pareto autorouting, main multi-threaded orchestrator loop, and end-to-end integration tests in Rust."
+++

# Objective

Port the complete Gauntlet deterministic multi-agent orchestrator (`src/` and `DESIGN.md`) into a clean, safe, idiomatic, high-performance Rust crate located under `rust/`.

The Rust port must be a complete 1:1 functional equivalent of the Python reference implementation, targeting the dedicated branch `rust-port` without modifying or regressing existing Python files.

---

## Architectural Requirements

1. **Deterministic State Machine**:
   - Zero LLM in the control loop. All phases (`INIT`, `PLAN`, `IMPLEMENT`, `INSPECT`, `INTEGRATE`, `GATES`, `REVIEW`, `JUDGE`, `PLAN_FIX`, `POLISH`, `DELIVER`, and all `BLOCKED_*` terminals) must behave identically to the reference implementation.
   - Atomic persistence of `state.json` via temporary file replacement.
   - Exact mathematical convergence logic: each fix wave must have strictly fewer blocking defect groups than the historical minimum (`min(history)`), otherwise classify as `STALLED`.

2. **Git Worktree & Containment Subsystem**:
   - Orchestrator alone runs Git. Workers never execute Git commands.
   - Dynamic Git worktree provisioning for integration and parallel lanes (`<repo>-worktree-gauntlet-<run>-<lane>`).
   - Mechanical checks in code: glob translation to regex, pairwise `owns` overlap validation, lane diff validation vs `owns`/`forbidden`, claimed files vs real diff, and root repository drift detection.
   - Worktree cleanup, rebase, and fast-forward delivery onto target branch `rust-port`.

3. **Structured I/O & Defect Protocol**:
   - Strict extraction of the last matching fenced block: `gauntlet-report`, `gauntlet-verdict`, `gauntlet-plan`, `gauntlet-stages`.
   - Complete defect classification (`code_defect`, `doc_drift`, `evidence_gap`) and verdict enforcement (`FIX`, `REDESIGN`, `REPORT_ONLY`, `DISMISS`).
   - Only `code_defect` with `FIX` and any `REDESIGN` block delivery; `doc_drift` and `evidence_gap` are routed exclusively to the non-blocking `POLISH` pass.

4. **Harness Adapters & Subprocess Protocol**:
   - Subprocess adapters for `agy`, `codex`, `cmd`, `kimi`, `reasonix`, `human`, and `echo`.
   - JSONL/NDJSON stream finalization unwrapping nested string payloads into literal plain text (`<stem>.raw` preserved, `<stem>.out` rewritten).
   - Idle timeouts (monitoring stdout file mtime) and hard deadlines with process termination.
   - Staging of capsule inside worktree (`.gauntlet/capsule.md`) for `agy` to prevent checkout anchoring.

5. **Super-Auto Pareto Frontier Routing & Fallback**:
   - Static risk analysis of contracts (security keywords, scope size, gate complexity, clause counts) generating tiered routing profiles (`high-risk`, `standard`, `fast`).
   - Resilient fallback executor with thread-safe run-level circuit breakers (`HarnessHealth`), retry/backoff policies, and error classifications (quota, auth, rate limit, model unavailable).

6. **Terminal UI & Visibility**:
   - ANSI color tokens, phase cards, live tickers/spinners with elapsed times and stream sizes, gate summaries, and verdict tables.

---

## AC (Acceptance Criteria)

- AC-1: The Rust crate is fully configured in `rust/Cargo.toml` with `src/lib.rs` and binary `src/main.rs`, building cleanly via `cargo check` and `cargo clippy`.
- AC-2: `statemachine.rs` implements all phases, transitions, atomic `state.json` persistence, and the mathematical convergence rule (`CONVERGING`, `STALLED`, `CAPPED`).
- AC-3: `mission.rs` and `config.rs` accurately parse TOML frontmatter + markdown body with `AC-*`/`INV-*`/`NG-*` clause extraction and hierarchical TOML config resolution.
- AC-4: `worktrees.rs` implements git worktree management, glob matching, pairwise overlap checks, lane diff inspection, claimed-vs-diff validation, and checkout drift detection.
- AC-5: `verdicts.rs` extracts and validates `gauntlet-report`, `gauntlet-verdict`, `gauntlet-plan`, and `gauntlet-stages` fenced blocks with defect classification and contract ID validation.
- AC-6: `capsules.rs` renders all prompt capsules (`implementer`, `reviewer`, `judge`, `planner`, `polish`, `checkpoint`) with safety rules, root contracts, and prior findings history.
- AC-7: `adapters/` implements all harness adapters (`agy`, `codex`, `cmd`, `kimi`, `reasonix`, `human`, `echo`), JSONL stream unwrapping, stderr classification, and idle/hard timeouts.
- AC-8: `fallback.rs` executes role chains with thread-safe circuit breakers (`HarnessHealth`), retry/backoff, and quota/auth error handling.
- AC-9: `autoroute.rs` scores contracts and maps roles to optimal model tiers (`high-risk`, `standard`, `fast`).
- AC-10: `ui.rs` and `report.rs` provide ANSI cards, spinners, tickers, and human-readable `report.md` generation.
- AC-11: `orchestrator.rs` and `cli.rs` execute the full state machine across threads with CLI arguments matching the Python reference (`--config`, `--auto`, `--interactive`, `--resume`, `--dry-run`, `--replan`, `--profile`, `--no-color`).
- AC-12: Comprehensive unit and integration test suite in `rust/tests/` passes with zero failures via `cargo test`.

---

## INV (Invariants)

- INV-1: Existing Python codebase in `src/`, `tests/`, `gauntlet`, etc. must remain untouched and fully operational.
- INV-2: The mission target branch is `rust-port`; no commits or merges may touch `main`.
- INV-3: The Rust orchestrator control loop must remain completely deterministic with zero LLM in the state machine.
- INV-4: Workers must never execute git commands; all git mutations are orchestrator-owned.
- INV-5: Protocol compatibility: all `state.json` files, capsule formats, and `gauntlet-*` blocks must be 100% interoperable with the Python version.

---

## NG (Non-Goals)

- NG-1: No Web or GUI frontend (CLI and library only).
- NG-2: No network daemons, external database backends, or background telemetry.
- NG-3: No automatic deletion or replacement of the Python implementation on the `main` branch.
