# Gauntlet — Design & implementation contract

Executable implementation of the canonical Gauntlet protocol
(`~/aios/.agents/skills/gauntlet-loop/SKILL.md`), runnable outside Codex:

```
./gauntlet [--config FILE] [--auto] [--resume RUN_DIR] [--dry-run] MISSION.md
```

The orchestrator is a **deterministic state machine** (Python 3.11+ stdlib
only — `tomllib`, `subprocess`, `threading`, `json`; zero third-party deps).
No LLM sits in the control loop. LLMs fill bounded roles configured in TOML;
every role chain ends with an implicit `human` link.

## Layout

```
gauntlet                  # executable wrapper: python3 -m src.cli "$@"
gauntlet.toml             # default harness/role configuration
DESIGN.md                 # this file
README.md
examples/mission-example.md
src/
  cli.py            # argparse, --config/--auto/--resume/--dry-run
  config.py         # TOML load + merge + chain validation
  mission.py        # mission file parsing (TOML frontmatter + body)
  statemachine.py   # phases, transitions, state.json persistence
  orchestrator.py   # drives the loop (threads for parallel lanes)
  worktrees.py      # git worktree lifecycle (orchestrator-owned)
  capsules.py       # per-role capsule rendering
  verdicts.py       # gauntlet-verdict / gauntlet-report block parsing+validation
  gates.py          # run gate commands in integration worktree
  fallback.py       # chain executor: retry policy + run-level circuit breaker
  report.py         # compact report.md maintenance
  adapters/
    base.py         # HarnessAdapter ABC, RunHandle, RunResult, FailureKind
    agy.py          # wraps ~/aios/.reasonix/skills/antigravity-delegation/scripts/agy-delegate
    cmd.py          # commandcode.ai CLI
    kimi.py         # kimi CLI
    reasonix.py     # reasonix CLI
    human.py        # interactive checkpoint (terminal)
    echo.py         # deterministic fake harness for tests and --dry-run
tests/              # unittest (stdlib), no network, no real harness calls
```

## Mission file format

Markdown contract with a TOML frontmatter delimited by `+++`:

```markdown
+++
slug = "auth-refactor"                     # optional; derived from filename otherwise
[[repos]]
path = "/Users/jhiver/le-bureau-core"
target_branch = "main"
gates = ["cargo test --workspace", "cargo clippy --all-targets -- -D warnings"]

[[lanes]]                                  # optional; planner role generates if absent
id = "L1"
owns = ["src/auth/**"]
forbidden = ["src/api/**"]
tests = ["cargo test -p auth"]
brief = "Extract session handling into src/auth/."
+++

# Objective
...
## AC
- AC-1: ...
## INV
- INV-1: ...
## NG
- NG-1: ...
```

The body is the immutable root contract. AC/INV/NG entries carry stable IDs;
the orchestrator extracts them with a simple regex (`^- (AC|INV|NG)-\w+:`)
to inject into capsules and to validate verdict `contract_ids`.

## Config resolution & schema

Resolution order (later overrides): built-in defaults →
`<tool>/gauntlet.toml` → `<mission-dir>/gauntlet.toml` → `--config FILE`.

```toml
[harnesses.cmd]
adapter = "cmd"                 # adapter module name
supports_write = true
default_model = "gpt-5.6-luna"
[harnesses.cmd.errors]          # failure classification regexes (stderr tail)
quota = ["insufficient[_ ]quota", "402"]
auth  = ["unauthorized", "401"]

[roles.reviewer]
chain = [
  { harness = "cmd", model = "gpt-5.6-sol", effort = "medium" },
  { harness = "reasonix", model = "deepseek-v4-pro" },
  # "human" is the implicit terminal link of every chain
]

[policy]
checkpoints = ["plan", "deliver"]   # director (human) approval points
max_fix_waves = 2
idle_timeout_s = 900                # reviewer/judge: no stream activity
hard_timeout_s = 2700               # reviewer/judge: wall clock cap
lane_timeout_s = 5400               # implementer/fixer wall clock cap

[fallback]
on_quota = "next_and_break"         # + run-level circuit breaker
on_auth = "break"                   # circuit breaker + alert, no chain drain
on_rate_limit = "backoff_retry_then_next"   # backoff 30s, 1 retry
on_timeout = "retry_once_then_next"
on_crash = "retry_once_then_next"
on_invalid_output = "retry_once_then_next"
max_attempts_per_task = 3           # beyond: human checkpoint
```

Validation at load: every link of a write-role chain (implementer, fixer)
must have `supports_write = true`; unknown adapter/harness names are config
errors, not runtime surprises.

## Roles

`implementer`, `fixer`, `reviewer`, `judge`, `planner`, `director`.

- `planner` is invoked only when the mission has no pre-written `[[lanes]]`
  (initial cut) and after each judgment with actionable groups (fix-wave
  recut). Its output is a `gauntlet-plan` JSON block validated like verdicts.
- `director` is consulted at configured checkpoints; default harness `human`.
  With `--auto`, checkpoints auto-approve.

## Adapter interface (adapters/base.py)

```python
class FailureKind(Enum):
    NONE = "none"
    QUOTA_EXHAUSTED = "quota"
    RATE_LIMITED = "rate_limit"
    AUTH_EXPIRED = "auth"
    TIMEOUT_IDLE = "timeout_idle"
    TIMEOUT_HARD = "timeout_hard"
    CRASH = "crash"
    PARTIAL_DELIVERY = "partial"
    OUTPUT_INVALID = "invalid_output"

@dataclass
class RunResult:
    failure: FailureKind
    exit_code: int | None
    output_path: Path        # captured stdout (final report lives here)
    detail: str              # stderr tail / classification reason

class HarnessAdapter(ABC):
    name: str
    supports_write: bool
    @abstractmethod
    def run(self, *, capsule: Path, worktree: Path, write: bool,
            model: str | None, effort: str | None,
            hard_timeout_s: int, idle_timeout_s: int | None,
            out_dir: Path) -> RunResult: ...
```

`run()` is blocking; the orchestrator runs lanes in threads (one per lane).
Idle detection: the adapter tees the harness event stream (or stdout) to a
file under `out_dir`; the orchestrator watches mtime — no mtime change for
`idle_timeout_s` → kill → `TIMEOUT_IDLE`. Harnesses without an event stream
use stdout mtime. Classification: adapter applies its configured error
regexes to the stderr tail, then falls back to exit-code heuristics.

### Concrete harness commands

Short prompt + capsule path, never an inline capsule (long inline prompts
fail silently on agy). cwd is always the lane worktree.

- **agy**: `bash ~/aios/.reasonix/skills/antigravity-delegation/scripts/agy-delegate
  --kind {implement|review} --complexity {low|medium|high} --mission {capsule}
  --cwd {worktree} [--write] [--model M] [--timeout T]`
- **cmd**: `cmd -p "Execute the mission file at {capsule} and follow it exactly."
  --model M --effort E --no-session --skip-onboarding --no-auto-update
  --output-format json` plus `--permission-mode plan` (read-only) or
  `--auto-accept` (write). Do NOT use `cmd -w/--worktree` — the orchestrator
  owns worktrees.
- **kimi**: `kimi -p "..." --add-dir {worktree} [-y | --auto]
  [--output-format stream-json] [-m M]`
- **reasonix**: `reasonix run [--model M] [--effort E] --events-jsonl` with
  the short prompt; read-only enforced via `--allowed-tools` deny rules.
- **human**: prints the capsule path + what is expected, waits on stdin for
  a decision (`approve`/`reject`, or a pasted verdict JSON block).
- **echo**: copies the capsule into the output file and emits a canned
  valid `gauntlet-report` / `gauntlet-verdict` (`NO_CLAIMS`). Used by tests
  and `--dry-run`.

## Structured I/O protocol

Workers end their report with a fenced block; reviewers/judges/planners
likewise. Missing/invalid block → `OUTPUT_INVALID` (retry per fallback
policy). Workers also self-declare `PARTIAL_DELIVERY` in the block.

````
```gauntlet-report
{"files_changed": ["src/a.rs"], "tests_run": ["cargo test"], "tests_passed": true,
 "partial": false, "notes": ""}
```

```gauntlet-verdict
{"groups": [{"root_cause": "...", "claims": ["..."], "contract_ids": ["AC-2"],
 "verdict": "FIX", "fix": "...", "owns": "src/auth/session.rs"}]}
```
verdict ∈ FIX | REDESIGN | REPORT_ONLY | DISMISS ; empty review = `{"groups": []}`.

```gauntlet-plan
{"lanes": [{"id": "F1", "owns": ["src/auth/**"], "forbidden": [],
 "tests": ["..."], "brief": "...", "addresses": ["<root_cause>"]}]}
```
````

`verdicts.py` extracts the LAST matching fenced block, JSON-parses, and
schema-validates (verdict enum, contract IDs must exist in the contract,
lane ownership globs non-empty).

## State machine

```
INIT → PLAN → [checkpoint: plan] → IMPLEMENT(wave=0) → INSPECT → INTEGRATE
     → GATES → REVIEW → JUDGE
     → if actionable: PLAN_FIX → IMPLEMENT(wave+=1) → INSPECT → INTEGRATE → GATES → REVIEW → JUDGE
     → if no actionable: [checkpoint: deliver] → DELIVER → READY
Terminals: READY | READY_NO_CHANGE | BLOCKED (architecture not converging,
           exhausted chain, failed required gate, safety violation)
```

Mechanical checks in code (never delegated to a model):
- PLAN: pairwise intersection of `owns` globs across lanes must be empty
  (translate globs, test overlap); violation → config error, back to planner
  once, then human.
- INSPECT: diff of each lane vs base must touch only `owns` paths and no
  `forbidden` path → automatic lane rejection, no debate.
- GATES: run the repo's gate commands once per integrated candidate.
- Wave counter from `state.json`; exceeding `max_fix_waves` → BLOCKED.

## Worktrees & git (orchestrator-owned)

For repo `/path/repo`, run `RUN=20260813-slug`:
- integration worktree: `/path/repo-worktree-gauntlet-RUN` on branch
  `gauntlet/RUN/integration` created from the target branch HEAD;
- lane worktrees: `/path/repo-worktree-gauntlet-RUN-L1` on branch
  `gauntlet/RUN/L1`, all from the SAME base commit.
Workers never run git. Integration = merge lane branch into integration
branch inside the integration worktree. DELIVER: rebase integration branch
onto target, rerun gates, then (checkpoint) fast-forward merge into target
in the main checkout, cleanup worktrees + branches. `--dry-run` prints every
git command instead of executing. Preserve unrelated user changes: refuse
INIT if the main checkout has staged changes on the target branch.

## Run directory & resume

`<main-checkout>/.missions/<YYYYMMDD>-<slug>/` — created by INIT:

```
mission.md      # resolved contract (copy of input)
config.toml     # effective merged config
state.json      # phase, wave, base_commit, lane states, harness health, verdicts
report.md       # compact human-readable mission report
capsules/       # ephemeral; deleted per lane after successful integration
outputs/        # one file per harness run (lane id + attempt)
verdicts/       # parsed verdict JSON per review round
```

`state.json` is rewritten after every phase transition and every lane status
change. `--resume RUN_DIR` reloads and re-enters the state machine at the
recorded phase; harness circuit breakers persist and re-open on first
success after resume.

## Safety

Every capsule embeds: no `.env`/secrets access, no network writes, no git
mutations, no agent launches, no production or destructive actions, writes
only inside owned paths. Reviewers/judges run read-only
(`--permission-mode plan` / `--allowed-tools` / no `--write`). The
orchestrator alone runs git and gate commands.

## Testing

`tests/` with stdlib `unittest`, the `echo` adapter, and a tmp git repo
fixture: mission parsing, config merge + chain validation, verdict/plan
extraction + schema rejection, glob-overlap check, forbidden-path diff
rejection, fallback policy matrix (quota breaker, timeout retry, chain
exhaustion → human), state round-trip + resume, full loop dry-run
(INIT→READY with echo harnesses and a real tmp git repo).
`python3 -m unittest discover -s tests -v` must pass with zero warnings.
