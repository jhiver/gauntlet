+++
slug = "worktree-agnostic-symlinks"

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
owns = ["src/config.rs", "tests/test_config.rs"]
forbidden = ["src/worktrees.rs", "src/orchestrator.rs", "tests/test_worktrees.rs"]
tests = ["cargo test --test test_config"]
brief = "Define WorktreeConfig struct with declarative symlinks in Config and add TOML validation and tests."

[[lanes]]
id = "L2"
owns = ["src/worktrees.rs", "src/orchestrator.rs", "tests/test_worktrees.rs"]
forbidden = ["src/config.rs", "tests/test_config.rs"]
tests = ["cargo test --test test_worktrees", "cargo test --test test_loop"]
brief = "Remove hardcoded node_modules symlinking and wire declarative config.worktree.symlinks across all worktree lifecycle phases."
+++

# Mission Objective: Language-Agnostic Declarative Worktree Symlinks

Eliminate hardcoded technology-specific assumptions (such as `"node_modules"`) from Gauntlet's core engine, making worktree directory sharing completely language-agnostic and declaratively configured via `[worktree].symlinks` in `gauntlet.toml`.

## Context & Motivation

Gauntlet operates as a universal autonomous engineering engine capable of running across any technology stack (Rust, Python, Node.js, Go, C++, etc.). Previously, `create_worktree` and `_phase_deliver` contained hardcoded references to `"node_modules"`. This violates the core design principle of technology agnosticism. Worktree dependencies (such as `.venv`, `node_modules`, `vendor/`, or custom build caches) must be declared explicitly in configuration.

## Acceptance Criteria (AC)

- **AC-1 (Declarative Configuration Schema)**:
  - Add `WorktreeConfig` to `src/config.rs` with `pub symlinks: Vec<String>` (defaulting to empty `vec![]`).
  - Add `pub worktree: WorktreeConfig` to `Config` with `#[serde(default)]`.
  - Support parsing and round-tripping `[worktree]` in `gauntlet.toml`, `--config`, and TOML tables.

- **AC-2 (Agnostic Worktree Lifecycle)**:
  - Update `create_worktree` in `src/worktrees.rs` to accept a slice of relative paths `symlinks: &[String]` instead of hardcoding `"node_modules"`.
  - For each path in `symlinks`, if the path exists in the root repository and does not yet exist in the worktree, create a relative symlink.
  - Remove all hardcoded string literals `"node_modules"` from `src/worktrees.rs` and `src/orchestrator.rs`.

- **AC-3 (Orchestrator Integration)**:
  - In `_phase_init`, `_phase_implement`, and `_phase_deliver`, pass `self.config.worktree.symlinks` to worktree creation and preparation routines.
  - Ensure all worktrees (integration worktrees, lane worktrees, and fix worktrees) receive configured symlinks deterministically.

- **AC-4 (Comprehensive Automated Tests)**:
  - Unit tests in `tests/test_config.rs` verifying `[worktree]` parsing, defaults, and serialization.
  - Integration tests in `tests/test_worktrees.rs` verifying generic symlink creation for multi-language directory structures (e.g. `.venv`, `node_modules`, `custom_cache`).

## Invariants (INV) - Inviolable Rules

- **INV-1 (Zero Language Hardcoding)**: No core engine file in `src/` may contain hardcoded framework/language dependency directory names.
- **INV-2 (Fail-Safe Symlinking)**: If a declared symlink source does not exist in the source repository, the engine logs or skips it safely without crashing or failing worktree creation.
- **INV-3 (Safety & Code Quality)**: Zero `unwrap()`, `expect()`, or `panic!` in non-test code. 0 Clippy warnings (`cargo clippy --all-targets -- -D warnings`).
- **INV-4 (Full Test Suite Regression-Free)**: All existing Gauntlet tests + new unit and integration tests must pass cleanly (`cargo test`).

## Non-Goals (NG)

- **NG-1**: Automatically guessing package managers or running package installation commands during worktree creation (dependency setup remains declarative).
- **NG-2**: Modifying VCS ignore semantics (`.gitignore` rules remain standard).
