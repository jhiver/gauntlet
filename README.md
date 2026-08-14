# Gauntlet

Deterministic orchestrator for structured engineering missions: contract →
parallel implementation lanes in git worktrees via pluggable LLM CLI
harnesses (agy, cmd, kimi, reasonix) → adversarial review → batch judgment →
fix waves for as long as the defect count keeps falling → polish pass →
delivery. No LLM sits in the control loop; LLMs fill bounded roles configured
in TOML.

## Usage

```
./gauntlet [--config FILE] [--profile {auto,fast,standard,high-risk}] [--interactive] [--resume RUN_DIR] [--dry-run] [--no-color] MISSION.md
```

- `MISSION.md` — the mission contract (TOML frontmatter + markdown body).
  Format: see `DESIGN.md` "Mission file format"; a working template is
  `examples/mission-example.md`.
- `--profile {auto,fast,standard,high-risk}` — Pareto frontier intelligent auto-routing:
  automatically analyzes contract invariants, security/auth keywords, and owned paths
  to calibrate model reasoning depth and fallback chains (Gemini 3.7 Flash High for fast
  execution, Codex GPT-5.6 Sol `xhigh` and Kimi K3 for high-assurance review/judgment).
- `--config FILE` — extra config, highest precedence. Resolution order:
  built-in defaults → `<tool>/gauntlet.toml` → `<mission-dir>/gauntlet.toml`
  → `--config`. See `gauntlet.toml` for the schema (harnesses, role chains,
  policy, fallback).
- `--auto` (default: enabled) — auto-approve all director checkpoints.
- `--interactive` / `--no-auto` — pause for manual director approval at checkpoints.
- `--resume RUN_DIR` — re-enter the state machine from a run directory
  (`<repo>/.missions/<YYYYMMDD>-<slug>/`).
- `--dry-run` — print every git command and the harness command that would
  run instead of executing; harnesses run as the deterministic `echo`
  harness.
- `--no-color` — disable ANSI rich formatting and live progress tickers.

Exit code: `0` on `READY` / `READY_NO_CHANGE`, `2` on any blocked terminal
(`BLOCKED_CONVERGENCE`, `BLOCKED_ARCHITECTURE`, `BLOCKED_GATE`,
`BLOCKED_HARNESS`, `BLOCKED`), `1` on usage/config errors. A blocked run
always means a human decision is required and writes the diagnosis into
`report.md`.

## Layout & protocol

Everything — adapter API, state machine, fallback policy, structured I/O
protocol, safety rules, testing requirements — is specified in `DESIGN.md`.

## Tests

```
python3 -m unittest discover -s tests -v
```

Stdlib-only unittest; no network, no real harness calls (echo adapter and
scripted fakes only), git mutations confined to test tempdirs.
