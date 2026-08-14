# Gauntlet

Deterministic orchestrator for structured engineering missions: contract →
parallel implementation lanes in git worktrees via pluggable LLM CLI
harnesses (agy, cmd, kimi, reasonix) → adversarial review → batch judgment →
bounded fix waves → delivery. No LLM sits in the control loop; LLMs fill
bounded roles configured in TOML.

## Usage

```
./gauntlet [--config FILE] [--auto] [--resume RUN_DIR] [--dry-run] MISSION.md
```

- `MISSION.md` — the mission contract (TOML frontmatter + markdown body).
  Format: see `DESIGN.md` "Mission file format"; a working template is
  `examples/mission-example.md`.
- `--config FILE` — extra config, highest precedence. Resolution order:
  built-in defaults → `<tool>/gauntlet.toml` → `<mission-dir>/gauntlet.toml`
  → `--config`. See `gauntlet.toml` for the schema (harnesses, role chains,
  policy, fallback).
- `--auto` — auto-approve all director checkpoints.
- `--resume RUN_DIR` — re-enter the state machine from a run directory
  (`<repo>/.missions/<YYYYMMDD>-<slug>/`).
- `--dry-run` — print every git command and the harness command that would
  run instead of executing; harnesses run as the deterministic `echo`
  harness.

Exit code: `0` on `READY` / `READY_NO_CHANGE`, `2` on `BLOCKED`, `1` on
usage/config errors.

## Layout & protocol

Everything — adapter API, state machine, fallback policy, structured I/O
protocol, safety rules, testing requirements — is specified in `DESIGN.md`.

## Tests

```
python3 -m unittest discover -s tests -v
```

Stdlib-only unittest; no network, no real harness calls (echo adapter and
scripted fakes only), git mutations confined to test tempdirs.
