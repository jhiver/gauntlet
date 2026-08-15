<div align="center">

# 🛡️ Gauntlet Engine

**The Autonomous Multi-Agent Engineering Engine with Mechanical Containment & Mathematical Convergence**

[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/Tests-277%20passed-brightgreen.svg)]()
[![Safety](https://img.shields.io/badge/Zero--Unwrap-100%25%20Crash--Resilient-blue.svg)]()
[![Dual-Engine](https://img.shields.io/badge/Engine-Rust%20%7C%20Python-blueviolet.svg)]()
[![License](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue.svg)]()

*Eliminate hallucination, workspace drift, and oscillatory regression loops in AI software engineering.*

[Quickstart](#-quickstart) •
[Core Architecture](#-core-architecture) •
[Mission Contracts](#-mission-contracts) •
[Skills Integration](#-skills-integration) •
[Configuration](#-configuration--profiles)

</div>

---

## ⚡ Why Gauntlet Engine?

Naive multi-agent loops and agentic coding tools frequently suffer from four critical failure modes:
1. **Workspace Drift & Collisions**: Parallel agents overwrite each other's files, touch unowned code, or pollute the main working directory.
2. **Unchecked Scope Creep**: Agents hallucinate speculative features, ignoring boundary invariants and non-goals.
3. **Cosmetic Review Bias**: Reviewers either rubber-stamp broken diffs or generate noisy, trivial nitpicks that distract from critical bugs.
4. **Oscillatory Regression Loops**: Fixes introduce new bugs in an endless circular loop without guaranteed termination.

**Gauntlet Engine** solves these problems through **deterministic mechanical containment** and **formal mathematical convergence**. No LLM sits in the orchestration loop; AI harnesses serve strictly bounded, isolated worker roles governed by a deterministic finite state machine.

---

## 🏛️ Core Architecture

```
                                 ┌────────────────────────────────────────────────────────┐
                                 │                 MISSION CONTRACT                       │
                                 │    AC-* (Acceptance)  INV-* (Invariants)  NG-* (Goals) │
                                 └───────────────────────────┬────────────────────────────┘
                                                             │
                                                             ▼
                                                    ┌─────────────────┐
                                                    │   PLAN STAGE    │
                                                    │ (Auto / Sliced) │
                                                    └────────┬────────┘
                                                             │
                                     ┌───────────────────────┴───────────────────────┐
                                     ▼                                               ▼
                         ┌───────────────────────┐                       ┌───────────────────────┐
                         │   LANE 1 (Worktree)   │                       │   LANE 2 (Worktree)   │
                         │   owns: ["src/core/*"]│                       │   owns: ["src/api/*"] │
                         └───────────┬───────────┘                       └───────────┬───────────┘
                                     │                                               │
                                     └───────────────────────┬───────────────────────┘
                                                             ▼
                                                    ┌─────────────────┐
                                                    │ MECHANICAL INSP │ (Drift & Glob Containment)
                                                    └────────┬────────┘
                                                             ▼
                                                    ┌─────────────────┐
                                                    │   INTEGRATION   │ ➔ [ MECHANICAL GATES ]
                                                    └────────┬────────┘
                                                             ▼
                                                    ┌─────────────────┐
                                                    │ ADVERSARIAL REV │ (Saint-Exupéry subtraction law)
                                                    └────────┬────────┘
                                                             ▼
                                                    ┌─────────────────┐
                                                    │  BATCH JUDGMENT │
                                                    └────────┬────────┘
                                                             │
                                  ┌──────────────────────────┴──────────────────────────┐
                                  │                                                     │
                             (No Claims)                                         (Defects Found)
                                  ▼                                                     ▼
                         ┌─────────────────┐                                   ┌─────────────────┐
                         │   POLISH PASS   │                                   │    PLAN FIX     │
                         └────────┬────────┘                                   │  (Wave N + 1)   │
                                  ▼                                            └────────┬────────┘
                         ┌─────────────────┐                                            │ (Strictly Decreasing:
                         │ DELIVER / READY │                                            ▼  E_n < min(E_0...E_{n-1}))
                         └─────────────────┘                                    [ IMPLEMENT WAVE ]
```

### 1. Contract-Bound Slicing (`AC-*`, `INV-*`, `NG-*`)
Missions are specified with immutable Acceptance Criteria (`AC-*`), inviolable Anti-Drift Invariants (`INV-*`), and strict Non-Goals (`NG-*`). Agents are legally forbidden from modifying unassigned paths or expanding scope.

### 2. Orthogonal Worktree Containment
Parallel lanes run in completely isolated, sibling Git worktrees. The engine mechanically verifies that:
- Every file changed matches the lane's `owns` globs (`check_lane_diff`).
- No file claimed by the worker is missing from the diff (`check_claimed_vs_diff`).
- The main repository checkout suffered **zero drift** during execution (`checkout_drift`).

### 3. Adversarial Contradictory Triad
- **Implementer**: Concurrently develops code in its sandboxed worktree.
- **Reviewer**: Strictly read-only auditor instructed to actively find edge cases, omissions, and over-engineering (*"Perfection is achieved not when there is nothing left to add, but when there is nothing left to take away"*).
- **Judge**: Batch arbitrator that deduplicates root causes and applies formal propositional logic:
  $$\text{FIX} = \text{justified} \land \text{aligned} \land \big((\text{simplifying} \land \text{equivalent}) \lor (\text{critical} \land \text{proportionate} \land \text{local})\big)$$

### 4. Bounded Mathematical Convergence
Fix waves are permitted if and only if the number of blocking defect groups strictly beats the best historical round:
$$\text{Defects}_{N} < \min(\text{Defects}_0, \dots, \text{Defects}_{N-1})$$
If defect counts oscillate or stall, Gauntlet halts immediately with `BLOCKED_CONVERGENCE` to prevent token burning and architectural drift.

### 5. Multi-Harness Circuit Breakers & Fallback Chains
Role chains seamlessly cascade across multiple LLM providers (AGY/Gemini, Codex/GPT-5, CMD, Kimi, Reasonix) with automated backoff, rate-limit retries, and quota circuit breakers.

---

## 🚀 Quickstart

### 1. Build and Install (Rust)

Gauntlet is available as a blazing-fast, zero-unwrap, crash-resilient native Rust binary:

```bash
# Build release binary
cargo build --release --manifest-path rust/Cargo.toml

# Install locally
cargo install --path rust
```

*(Alternatively, run directly with Python 3.11+: `./gauntlet <mission.md>`)*

### 2. Write a Mission Contract (`missions/my-feature.md`)

```markdown
+++
slug = "my-feature"

[[repos]]
path = "."
target_branch = "main"
gates = [
  "cargo test",
  "cargo clippy --all-targets -- -D warnings"
]

[[lanes]]
id = "L1"
owns = ["src/core/**", "tests/test_core.rs"]
forbidden = ["src/api/**"]
tests = ["cargo test --test test_core"]
brief = "Implement core data structures and boundary validation."
+++

# Objective
Implement high-throughput transaction cache with bounded memory eviction.

## Acceptance Criteria (AC)
- AC-1: Cache supports O(1) reads and writes up to 100k items.
- AC-2: Exceeding capacity triggers LRU eviction without deadlocks.

## Invariants (INV)
- INV-1: Zero unsafe blocks or unwrap() in production code.
- INV-2: Full thread-safety under high concurrency.

## Non-Goals (NG)
- NG-1: Network clustering or distributed persistence.
```

### 3. Run the Gauntlet

```bash
# 1. Validate syntax and lane orthogonality without executing
gauntlet --dry-run missions/my-feature.md

# 2. Run with Super-Auto Pareto optimization
gauntlet --profile auto missions/my-feature.md

# 3. Or run with pure AGY / Gemini 3.7 Flash High
gauntlet --config gauntlet.agy.toml missions/my-feature.md
```

---

## 🛠️ CLI Options

| Flag | Description |
| :--- | :--- |
| `--profile {auto,fast,standard,high-risk}` | Auto-tunes reasoning depth and model selection based on contract risk. |
| `--config FILE` | Overrides configuration with custom TOML routing (e.g. `gauntlet.agy.toml`). |
| `--auto` *(default)* | Runs unattended, auto-approving director checkpoints. |
| `--interactive` | Pauses for human confirmation at `PLAN` and `DELIVERY` checkpoints. |
| `--resume RUN_DIR` | Resumes an interrupted run directly from its `.missions/<run>/state.json`. |
| `--dry-run` | Prints all planned worktrees, git operations, and harness capsules without mutating git. |
| `--no-color` | Disables ANSI colors and interactive spinners (CI/CD friendly). |

---

## 🤖 Skills Integration

Gauntlet Engine includes two first-class agent skills for Antigravity, Claude Code, Cursor, and Reasonix:

| Skill | Directory | Purpose |
| :--- | :--- | :--- |
| **`gauntlet-architect`** | `skills/gauntlet-architect/` | Explores the codebase, analyzes blast radius, drafts invariant contracts, and partitions orthogonal lanes. |
| **`gauntlet-pilot`** | `skills/gauntlet-pilot/` | Supervises autonomous execution, manages checkpoints, monitors convergence, and handles triage. |

### Installing Skills
To install in your local agent environment:
```bash
mkdir -p ~/.agents/skills
cp -r skills/gauntlet-architect ~/.agents/skills/
cp -r skills/gauntlet-pilot ~/.agents/skills/
```

---

## ⚙️ Configuration & Profiles

Gauntlet resolves configuration via deep recursive merge:
1. **Built-in Defaults** (embedded in the binary)
2. **Project Config**: `./gauntlet.toml`
3. **Mission Config**: `<mission_dir>/gauntlet.toml`
4. **CLI Flag**: `--config <custom.toml>`

### Example: Pure AGY High-Throughput Routing (`gauntlet.agy.toml`)
```toml
[harnesses.agy]
adapter = "agy"
supports_write = true
default_model = "gemini-3.7-flash-high"
launcher = "agy-delegate"

[roles.implementer]
chain = [{ harness = "agy", model = "gemini-3.7-flash-high", effort = "high" }]

[roles.reviewer]
chain = [{ harness = "agy", model = "gemini-3.7-flash-high", effort = "high" }]

[roles.judge]
chain = [{ harness = "agy", model = "gemini-3.7-flash-high", effort = "high" }]

[policy]
max_total_waves = 5
on_wave_cap = "checkpoint"
```

---

## 🧪 Rigorous Verification & Safety

Gauntlet is engineered to high-assurance standards:
- **Zero-Unwrap Codebase**: 100% of production Rust code (`rust/src/`) is free from `unwrap()`, `expect()`, `panic!`, and unsafe string/array slicing.
- **Poison-Resilient Mutexes**: Seamless recovery from concurrent thread panics.
- **Dual Test Suite**:
  - **136 Rust Unit & Integration Tests**: State machine, worktree isolation, glob engines, circuit-breakers.
  - **141 Python Unit Tests**: Cross-validation of protocol invariants.
  - **0 Clippy Warnings** (`--all-targets -- -D warnings`).

Run the full verification suite:
```bash
# Test Rust Engine
cargo test --manifest-path rust/Cargo.toml
cargo clippy --manifest-path rust/Cargo.toml --all-targets -- -D warnings

# Test Python Engine
python3 -m unittest discover -s tests -v
```

---

## 📄 License

Dual-licensed under either of:
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
