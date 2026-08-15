<div align="center">

# 🛡️ Gauntlet Engine

### *Autonomous Multi-Agent Software Engineering with Mechanical Containment & Mathematical Convergence*

[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg?logo=rust)](https://www.rust-lang.org)
[![Tests](https://img.shields.io/badge/Tests-277%20passed-brightgreen.svg)]()
[![Safety](https://img.shields.io/badge/Zero--Unwrap-100%25%20Crash--Resilient-blue.svg)]()
[![Dual-Engine](https://img.shields.io/badge/Engine-Rust%20%7C%20Python-blueviolet.svg)]()
[![License](https://img.shields.io/badge/License-MIT%2FApache--2.0-blue.svg)]()

[The Vision](#-the-vision) •
[The 5 Core Roles](#-the-5-core-roles) •
[Multi-Model Synergy](#-multi-model--multi-harness-synergy) •
[State Machine Workflow](#-state-machine-lifecycle) •
[Quickstart](#-quickstart) •
[Skills](#-skills-integration)

</div>

---

## 🌟 The Vision

Giving an AI agent free rein over a complex codebase usually leads to three catastrophic failure modes:
1. **Workspace Drift & Collisions**: Parallel agents edit files they don't own, hallucinate unrequested refactors, or conflict with each other.
2. **Review Blindspots & Sycophancy**: The same AI that wrote the code reviews its own PR with an inherent bias, missing edge cases and introducing architectural bloat.
3. **Infinite Regression Loops**: Fixing bug A introduces bug B, leading to infinite retries, burned tokens, and degraded code quality.

**Gauntlet Engine** solves this by treating autonomous engineering not as a loose chat prompt, but as a **deterministic, invariant-bound state machine**. 

No LLM sits in the control loop. Instead, specialized AI models fill strictly bounded, sandboxed roles governed by mechanical Git isolation and formal mathematical convergence.

---

## 🎭 The 5 Core Roles

In Gauntlet, no single agent does everything. Tasks are strictly segregated into 5 specialized roles to ensure checks, balances, and zero sycophancy:

```mermaid
flowchart TD
    subgraph Planning & Execution
        A["<b>1. Planner</b><br><i>Slices problem into orthogonal lanes</i>"] -->|Disjoint Worktrees| B["<b>2. Implementer</b><br><i>Builds features in parallel</i>"]
    end

    subgraph Adversarial Audit & Judgment
        B -->|Merged Candidate| C["<b>3. Reviewer</b><br><i>Adversarial Auditor: HATES what they see</i>"]
        C -->|Unfiltered Finding Groups| D["<b>4. Judge</b><br><i>Batch Arbitrator: Formal Propositional Logic</i>"]
    end

    subgraph Targeted Convergence
        D -->|Valid Blocking Defects| E["<b>5. Fixer</b><br><i>Targeted surgical fix wave</i>"]
        E -->|Candidate Wave N+1| C
        D -->|Zero Blocking Defects| F["<b>✨ Polish & Deliver</b>"]
    end

    style A fill:#0f3854,stroke:#38bdf8,stroke-width:2px,color:#ffffff
    style B fill:#064e3b,stroke:#34d399,stroke-width:2px,color:#ffffff
    style C fill:#78350f,stroke:#fbbf24,stroke-width:2px,color:#ffffff
    style D fill:#581c87,stroke:#c084fc,stroke-width:2px,color:#ffffff
    style E fill:#7f1d1d,stroke:#f87171,stroke-width:2px,color:#ffffff
    style F fill:#134e4a,stroke:#2dd4bf,stroke-width:2px,color:#ffffff
```

| Role | Responsibility | Philosophy |
| :--- | :--- | :--- |
| **`planner`** | Analyzes the contract and partitions the work into mathematically disjoint `[[lanes]]` (`owns` vs `forbidden` file globs). | *Never invent fake parallelism; slice work cleanly.* |
| **`implementer`** | Writes code and local tests inside an isolated, ephemeral Git worktree. | *Fast, focused, confined strictly to owned files.* |
| **`reviewer`** | A read-only adversarial auditor that inspects the complete integrated diff. | *Antoine de Saint-Exupéry: "Perfection is achieved not when there is nothing left to add, but when there is nothing left to take away."* |
| **`judge`** | An independent batch arbitrator that deduplicates root causes and applies formal criteria (`FIX`, `REDESIGN`, `REPORT_ONLY`, `DISMISS`). | *Zero emotion, purely propositional logic.* |
| **`fixer`** | Performs focused surgical repairs on validated root causes in a fresh, orthogonal fix wave. | *Minimal blast radius, net reduction in complexity.* |

---

## ⚡ Multi-Model & Multi-Harness Synergy

Using a single model for both coding and reviewing creates an echo chamber. Gauntlet Engine natively orchestrates **heterogeneous model ensembles** with self-healing fallback chains and circuit breakers:

```mermaid
flowchart LR
    subgraph Role Execution
        R["<b>Role Request</b>"] --> H1["<b>Link 1: Primary Harness</b><br>Gemini 3.7 Flash High"]
    end

    subgraph Self-Healing Fallback Chain
        H1 -->|Rate Limit / 429| B1["<b>Backoff & Retry</b>"]
        H1 -->|Quota Exceeded| CB1["<b>Open Circuit Breaker</b>"]
        CB1 --> H2["<b>Link 2: Secondary Harness</b><br>Codex GPT-5.6 Sol (xhigh)"]
        H2 -->|Unavailable / Crash| H3["<b>Link 3: Tertiary Harness</b><br>Kimi K3 / DeepSeek"]
        H3 -->|Chain Exhausted| H4["<b>Terminal: Human Director</b>"]
    end

    style R fill:#1e293b,stroke:#94a3b8,stroke-width:2px,color:#ffffff
    style H1 fill:#0f3854,stroke:#38bdf8,stroke-width:2px,color:#ffffff
    style B1 fill:#78350f,stroke:#fbbf24,stroke-width:2px,color:#ffffff
    style CB1 fill:#7f1d1d,stroke:#f87171,stroke-width:2px,color:#ffffff
    style H2 fill:#581c87,stroke:#c084fc,stroke-width:2px,color:#ffffff
    style H3 fill:#064e3b,stroke:#34d399,stroke-width:2px,color:#ffffff
    style H4 fill:#7c2d12,stroke:#fb923c,stroke-width:2px,color:#ffffff
```

- **Speed vs Reasoning Pareto Frontier**: Fast, high-context models (like *Gemini 3.7 Flash*) handle high-throughput parallel implementations, while deep reasoning models (*GPT-5.6 Sol / Kimi K3*) perform adversarial audits and judgments.
- **Circuit Breaker Resilience**: If an API provider suffers outages, rate limits, or auth expiration, Gauntlet trips a per-run circuit breaker and seamlessly cascades to the next configured provider without losing state.

---

## 🔄 State Machine Lifecycle

Gauntlet Engine runs a deterministic finite state machine where every transition is recorded in `state.json` and `report.md`:

```mermaid
stateDiagram-v2
    [*] --> INIT
    INIT --> PLAN: Load Contract & Check Out Base
    
    PLAN --> IMPLEMENT: Orthogonal Lanes Defined
    
    state IMPLEMENT {
        [*] --> Worktree_Lane_1
        [*] --> Worktree_Lane_2
        Worktree_Lane_1 --> Merge_Lanes
        Worktree_Lane_2 --> Merge_Lanes
    }
    
    IMPLEMENT --> INSPECT: Parallel Workers Complete
    INSPECT --> INTEGRATE: Containment & Drift Checks Pass
    INSPECT --> BLOCKED_SAFETY: Path Violation / Drift Detected
    
    INTEGRATE --> GATES: Merge into Integration Branch
    GATES --> REVIEW: Deterministic Gates Pass (Tests/Lints)
    GATES --> BLOCKED_GATE: Mechanical Gate Failure
    
    REVIEW --> JUDGE: Adversarial Audit Done
    
    state JUDGE_DECISION <<choice>>
    JUDGE --> JUDGE_DECISION
    
    JUDGE_DECISION --> POLISH: No Blocking Claims (Clean Candidate)
    JUDGE_DECISION --> PLAN_FIX: Blocking Defects Present (Wave N+1)
    JUDGE_DECISION --> BLOCKED_CONVERGENCE: Defect Count Stalled / Oscillating
    JUDGE_DECISION --> BLOCKED_ARCHITECTURE: REDESIGN Verdict Issued
    
    PLAN_FIX --> IMPLEMENT: Coalesce Overlaps & Re-slice Lanes
    
    POLISH --> DELIVER: Final Verification
    DELIVER --> READY: Rebase on Target Branch & Clean Worktrees
    READY --> [*]
```

### The Iron Law of Convergence: $E_n < \min(E_0 \dots E_{n-1})$
To prevent endless loops, fix waves are allowed **only if the number of blocking defects strictly decreases compared to the best historical round**. If an agent oscillates ($3 \to 1 \to 2$ defects), the engine immediately halts with `BLOCKED_CONVERGENCE` for human triage.

---

## 🚀 Quickstart

### 1. Build and Install (Rust)

Gauntlet is built in production-grade, zero-panic Rust with 100% typed error handling:

```bash
# Build release binary
cargo build --release --manifest-path rust/Cargo.toml

# Install to your PATH
cargo install --path rust
```

*(Alternatively, run the Python 3.11+ engine directly: `./gauntlet <mission.md>`)*

---

### 2. Write a Mission Contract (`missions/my-feature.md`)

A Gauntlet mission combines TOML frontmatter (mechanical rules & lanes) and Markdown (human contracts):

```markdown
+++
slug = "cache-service"

[[repos]]
path = "."
target_branch = "main"
gates = [
  "cargo test",
  "cargo clippy --all-targets -- -D warnings"
]

[[lanes]]
id = "L1"
owns = ["src/cache/**", "tests/test_cache.rs"]
forbidden = ["src/api/**"]
tests = ["cargo test --test test_cache"]
brief = "Implement bounded LRU cache with eviction logic."

[[lanes]]
id = "L2"
owns = ["src/api/**", "tests/test_api.rs"]
forbidden = ["src/cache/**"]
tests = ["cargo test --test test_api"]
brief = "Expose cache endpoints over HTTP."
+++

# Mission Objective
Implement the cache service and REST endpoints with zero regressions.

## Acceptance Criteria (AC)
- AC-1: Cache supports O(1) concurrent get/put operations.
- AC-2: Eviction policy triggers deterministically when capacity is reached.

## Invariants (INV) - Inviolable Rules
- INV-1: Zero unwrap(), expect(), or panic! in production code.
- INV-2: Full thread safety under high concurrency.

## Non-Goals (NG)
- NG-1: Distributed Redis/Memcached clustering.
```

---

### 3. Run the Gauntlet

```bash
# 1. Dry run: Verify lane orthogonality and worktree allocation without modifying git
gauntlet --dry-run missions/my-feature.md

# 2. Super-Auto mode: Intelligent Pareto routing based on mission risk
gauntlet --profile auto missions/my-feature.md

# 3. Pure AGY mode: Run all roles via Gemini 3.7 Flash High
gauntlet --config gauntlet.agy.toml missions/my-feature.md

# 4. Interactive mode: Pauses for human confirmation at key milestones
gauntlet --interactive missions/my-feature.md
```

---

## 🤖 Skills Integration

Gauntlet Engine includes two dedicated skills for agentic IDEs (**Antigravity, Claude Code, Cursor, Reasonix**):

```
.agents/skills/
├── gauntlet-architect/    # Slices contracts, defines blast radius, partitions orthogonal lanes
└── gauntlet-pilot/        # Supervises execution, manages checkpoints, handles triage & delivery
```

### Install in your Agent environment
```bash
mkdir -p ~/.agents/skills
cp -r skills/gauntlet-architect ~/.agents/skills/
cp -r skills/gauntlet-pilot ~/.agents/skills/
```

- **Invoke `gauntlet-architect`**: When you have an idea, PRD, or feature request and want a formal, orthogonal mission contract created.
- **Invoke `gauntlet-pilot`**: When you want the agent to execute, supervise, monitor, and deliver a mission into your repository.

---

## ⚙️ Configuration & Profiles

Gauntlet merges configuration in the following order:
1. **Built-in Defaults** (embedded in the engine)
2. **Project Config**: `./gauntlet.toml`
3. **Mission Config**: `<mission_dir>/gauntlet.toml`
4. **CLI Flag**: `--config <custom.toml>`

### Ready-to-use Configurations:
- **`gauntlet.toml`**: Multi-harness setup combining AGY, Codex GPT-5.6, CMD, and Kimi K3.
- **`gauntlet.agy.toml`**: High-throughput configuration running 100% on AGY (Gemini 3.7 Flash High).

---

## 🧪 Safety & Test Suite

Gauntlet Engine is tested to the highest assurance standards:
- **0 `unwrap()` / `expect()` / `panic!`** in all non-test Rust modules.
- **Poison-Resilient Mutexes**: Thread locks automatically recover from poisoned states.
- **277 Tests Passed (0 Failures)**:
  - 136 Rust unit and integration tests.
  - 141 Python unit tests.
  - Clean Clippy check (`--all-targets -- -D warnings`).

---

## 🙏 Acknowledgments & Credits

**Gauntlet Engine** builds upon, formalizes, and extends the foundational concepts introduced by [robonuggets/gauntlet-loop](https://github.com/robonuggets/gauntlet-loop). We expand the original loop into a production-grade, dual-engine (Rust + Python) autonomous orchestrator with mechanical worktree isolation, adversarial multi-role auditing, and bounded mathematical convergence.

---

## 📄 License

Dual-licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT) at your option.
