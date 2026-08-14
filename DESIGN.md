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
max_total_waves = 5                 # absolute safety cap on fix waves
on_wave_cap = "checkpoint"          # when convergence stalls: ask the
                                    # director for one more wave ("block"
                                    # stops immediately instead)
idle_timeout_s = 900                # reviewer/judge: no stream activity
hard_timeout_s = 2700               # reviewer/judge: wall clock cap
lane_timeout_s = 5400               # implementer/fixer wall clock cap

[fallback]
on_quota = "next_and_break"         # + run-level circuit breaker
on_auth = "break"                   # circuit breaker + alert, no chain drain
on_rate_limit = "backoff_retry_then_next"   # backoff 30s, 1 retry
on_model_unavailable = "next"       # model-scoped (e.g. not in plan): no
                                    # retry, no breaker — other models on the
                                    # same harness stay usable
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
  (initial cut) and after each judgment with blocking groups (fix-wave
  recut). Its output is a `gauntlet-plan` JSON block validated like verdicts.
- `director` is consulted at configured checkpoints; default harness `human`.
  With `--auto`, checkpoints auto-approve — except the wave-cap consult,
  which auto-*rejects*: an unattended run must not grant itself extra waves.
- Before each REVIEW, the orchestrator writes the full base-to-candidate
  diff to `reviews/diff-w<N>.patch` and references it in the reviewer
  capsule, so reviewers do not need shell access to inspect the candidate.
- The reviewer capsule of a fix wave carries the root causes of every
  previous round in three buckets — fixed, deferred to the polish pass, and
  dismissed: a fresh reviewer verifies the previous fixes instead of
  re-sampling the defect distribution from zero. The judge gets the deferred
  and dismissed lists too, so a re-opened claim is closed at the layer that
  decides. Both capsules state the review discipline: claim only against
  a contract clause, never against a behavior the contract explicitly allows
  or a non-goal excludes, and keep rare races and crash windows
  `REPORT_ONLY` unless the mission targets recovery or concurrency.
- The `fixer` chain also runs the pre-delivery polish pass (see below).

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

Harnesses emitting a JSONL event stream (cmd `--output-format json`, kimi
`stream-json`, reasonix `--events-jsonl`) set `jsonl_output = True`: after
exit, the raw stream is kept in `<stem>.raw` and `<stem>.out` is rewritten
as plain text (every string payload of each JSON line, in order). Fenced
`gauntlet-*` blocks travel inside escaped JSON strings (e.g. cmd's final
`result.finalText`); this rewrite makes them literal and extractable.

### Concrete harness commands

Short prompt + capsule path, never an inline capsule (long inline prompts
fail silently on agy). cwd is always the lane worktree.

- **agy**: `bash ~/aios/.reasonix/skills/antigravity-delegation/scripts/agy-delegate
  --kind {implement|review} --complexity {low|medium|high} --mission {capsule}
  --cwd {worktree} [--write] [--model M] [--timeout T]`.
  The capsule is first staged at `{worktree}/.gauntlet/capsule.md` and removed
  after the run: the launcher passes `--add-dir <capsule-dir>` to agy, and a
  capsule living in the main checkout made agy anchor on (and write into) the
  main checkout instead of the lane worktree.
- **cmd**: `cmd -p "Execute the mission file at {capsule} and follow it exactly."
  --model M --effort E --no-session --skip-onboarding --no-auto-update
  --output-format json` plus `--permission-mode plan` (read-only) or
  `--yolo` (write — in `-p` mode tool use is permission-gated and
  `--auto-accept` is NOT sufficient). Do NOT use `cmd -w/--worktree` — the
  orchestrator owns worktrees.
- **kimi**: `kimi -p "..." --add-dir {worktree} [-y | --auto]
  [--output-format stream-json] [-m M]`
- **reasonix**: `reasonix -p "..." --output-format stream-json [--model M]
  [--effort E]`; read-only roles add `--allowed-tools
  deny:write,deny:bash,deny:git` (the reviewer reads the diff file and
  worktree files; it needs no shell). Do NOT use `reasonix run
  --events-jsonl` for output capture: that stream is redacted (kind markers
  only, no content).
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
 "verdict": "FIX", "class": "code_defect", "fix": "...",
 "owns": "src/auth/session.rs"}]}
```
verdict ∈ FIX | REDESIGN | REPORT_ONLY | DISMISS ; empty review = `{"groups": []}`.
class ∈ code_defect | doc_drift | evidence_gap (default `code_defect`).

```gauntlet-plan
{"lanes": [{"id": "F1", "owns": ["src/auth/**"], "forbidden": [],
 "tests": ["..."], "brief": "...", "addresses": ["<root_cause>"]}]}
```
````

`verdicts.py` extracts the LAST matching fenced block, JSON-parses, and
schema-validates (verdict enum, class enum, contract IDs must exist in the
contract, lane ownership globs non-empty).

Only two kinds of group hold up delivery: a `code_defect` with verdict `FIX`,
and any `REDESIGN` (only a code defect can make the smallest additive patch
disproportionate). `doc_drift` and `evidence_gap` are real work but they are
collected — across every round — for the single polish pass below, so a stale
doc or an unreproducible proof never costs a fix wave and never blocks a
candidate whose gates are green.

## State machine

```
INIT → PLAN → [checkpoint: plan] → IMPLEMENT(wave=0) → INSPECT → INTEGRATE
     → GATES → REVIEW → JUDGE
     → if blocking: PLAN_FIX → IMPLEMENT(wave+=1) → INSPECT → INTEGRATE → GATES → REVIEW → JUDGE
     → if none blocking: POLISH → [checkpoint: deliver] → DELIVER → READY
Terminals: READY | READY_NO_CHANGE
         | BLOCKED_CONVERGENCE   (fix waves stopped reducing the count)
         | BLOCKED_ARCHITECTURE  (an accepted REDESIGN group)
         | BLOCKED_GATE          (a required gate failed)
         | BLOCKED_HARNESS       (no harness in a chain could deliver)
         | BLOCKED               (safety violation, config error, merge conflict)
```

A blocked terminal always means the same thing — **a human decision is
required, here is the diagnosis** — never "delivered with leftovers". Its kind
selects the recovery playbook, and `_blocked()` writes a diagnosis section into
`report.md`: phase at failure, wave, blocking-group trajectory, remaining
groups with class and owner, gates, harness health, and the preserved
candidate branch. Nothing else is needed to resume or re-scope.

### Fix waves and convergence

A wave is granted while the mission converges: each judgment must have
**strictly fewer blocking groups than the best round so far**, so an
oscillation (7 → 4 → 5) counts as stalled, not as progress. Counting judged
groups (not raw claims) is what makes the metric honest — the judge has
already deduplicated by root cause. When the count stalls, `on_wave_cap`
decides: `checkpoint` asks the director for one more wave (auto-rejected under
`--auto`), `block` stops immediately. `max_total_waves` is the absolute cap
that bounds the worst case whatever the trajectory.

### Polish pass

One `fixer` run in the integration worktree, after the last judgment, over the
accumulated non-blocking findings. It cannot block delivery by construction: a
failed pass, a write outside the findings' declared `owns`, or a gate that
breaks on its result discards the pass and the candidate ships as judged —
each outcome recorded in `report.md` and in the deliver checkpoint. Gates run
on the dirty worktree *before* the polish is committed, so discarding is a
`reset --hard`, never a revert of history.

Mechanical checks in code (never delegated to a model):
- PLAN: pairwise intersection of `owns` globs across lanes must be empty
  (translate globs, test overlap); violation → config error, back to planner
  once, then human.
- INSPECT: diff of each lane vs base must touch only `owns` paths and no
  `forbidden` path → automatic lane rejection, no debate. Two containment
  checks join it: every file the worker's report claims must appear in the
  lane diff (a miss means the write landed elsewhere), and the main checkout
  is diffed against its pre-lanes snapshot — any new path outside
  `.missions/` is a containment breach and blocks the run (`SAFETY`).
- GATES: run the repo's gate commands once per integrated candidate.
- JUDGE: blocking-group count per wave is appended to
  `state.json:blocking_history`; the convergence rule and the absolute cap are
  computed from it in code, never asked of a model.
- POLISH: the pass is contained by the union of the findings' `owns` globs
  (same check as INSPECT) whenever all of them declare one.

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
state.json      # phase, wave, base_commit, lane states, harness health,
                # verdicts, blocking_history, polish + blocked diagnosis
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
extraction + schema rejection, defect-class semantics (blocking vs polish),
glob-overlap check, forbidden-path diff rejection, fallback policy matrix
(quota breaker, timeout retry, chain exhaustion → human), state round-trip +
resume, full loop dry-run (INIT→READY with echo harnesses and a real tmp git
repo). `tests/test_waves.py` scripts reviewer/judge verdicts per wave to
exercise the convergence rule (decrease, equality, oscillation, cap,
director grant), the blocked-terminal kinds, the polish pass and its three
discard paths, and the reviewer's cross-round memory.
`python3 -m unittest discover -s tests -v` must pass with zero warnings.
