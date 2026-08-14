//! Per-role capsule rendering.
//!
//! Every capsule embeds the safety rules (DESIGN.md "Safety"), the immutable
//! root contract, and the exact fenced output block the role must produce.
//! Lane capsules additionally carry machine-readable `lane-id:` / `lane-owns:` /
//! `lane-tests:` / `wave:` lines (used by the echo harness).

use std::collections::BTreeSet;

/// Mandatory safety rules included in every capsule.
pub const SAFETY: &str = r#"## Safety (mandatory, non-negotiable)

- Never read, display, copy, or process `.env` files or any secrets.
- No network writes to external services or systems.
- No git mutations: the orchestrator owns all git operations.
- Do not launch other agents.
- No production actions, no destructive actions.
- Write only inside the paths your lane owns.
"#;

pub const REPORT_FORMAT: &str = r#"End your report with exactly one fenced block:
```gauntlet-report
{"files_changed": ["..."], "tests_run": ["..."], "tests_passed": true,
 "partial": false, "notes": ""}
```
Set "partial": true if you could not complete the lane."#;

pub const VERDICT_FORMAT: &str = r#"End your output with exactly one fenced block:
```gauntlet-verdict
{"groups": [{"root_cause": "...", "claims": ["..."],
 "contract_ids": ["AC-1"], "verdict": "FIX", "class": "code_defect",
 "fix": "...", "owns": "src/path.py"}]}
```
verdict is one of FIX | REDESIGN | REPORT_ONLY | DISMISS.
class is one of code_defect | doc_drift | evidence_gap (default code_defect).
`owns` is the path or glob the correction belongs to.
An empty review is {"groups": []}. Only use contract IDs listed above."#;

pub const PLAN_FORMAT: &str = r#"End your output with exactly ONE fenced block:

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
```"#;

pub const REVIEWER_STANCE: &str = r#"You are a senior dev doing the code review before these changes get committed to git and you HATE what you are seeing... What would you criticize? What edge cases am I missing?

Remember Antoine de Saint-Exupéry “Perfection is achieved, not when there is nothing more to add, but when there is nothing left to take away.”.

Your proposed solutions need to bring robustness through simplification and elegance, not over-engineered bloat."#;

pub const REVIEW_DISCIPLINE: &str = r#"## Review discipline

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
  concern over adding a compensating layer."#;

pub const JUDGE_RULE: &str = r#"For every root-cause group, evaluate: (1) justified — the defect exists on a
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
delivery, so classify honestly rather than to force or avoid a fix wave."#;

// -----------------------------------------------------------------------------
// Traits and Data Structures for Flexible Interoperability
// -----------------------------------------------------------------------------

pub trait MissionLike {
    fn body(&self) -> &str;
    fn contract_ids(&self) -> Box<dyn Iterator<Item = &str> + '_>;
}

pub trait LaneLike {
    fn id(&self) -> &str;
    fn owns(&self) -> &[String];
    fn forbidden(&self) -> &[String];
    fn tests(&self) -> &[String];
    fn brief(&self) -> &str;
    fn addresses(&self) -> &[String];
}

pub trait ClaimGroupLike {
    fn root_cause(&self) -> &str;
    fn fix(&self) -> &str;
    fn verdict(&self) -> &str;
    fn owns(&self) -> &str;
    fn defect_class(&self) -> &str;
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MissionInfo {
    pub body: String,
    pub contract_ids: Vec<String>,
}

impl MissionLike for MissionInfo {
    fn body(&self) -> &str {
        &self.body
    }
    fn contract_ids(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        Box::new(self.contract_ids.iter().map(|s| s.as_str()))
    }
}

impl<I> MissionLike for (&str, I)
where
    I: Clone + IntoIterator,
    I::Item: AsRef<str>,
{
    fn body(&self) -> &str {
        self.0
    }
    fn contract_ids(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        let items: Vec<String> = self.1.clone().into_iter().map(|s| s.as_ref().to_string()).collect();
        // Return a boxed iterator using the collected strings
        Box::new(Box::leak(items.into_boxed_slice()).iter().map(|s| s.as_str()))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaneInfo {
    pub id: String,
    pub owns: Vec<String>,
    pub forbidden: Vec<String>,
    pub tests: Vec<String>,
    pub brief: String,
    pub addresses: Vec<String>,
}

impl LaneLike for LaneInfo {
    fn id(&self) -> &str {
        &self.id
    }
    fn owns(&self) -> &[String] {
        &self.owns
    }
    fn forbidden(&self) -> &[String] {
        &self.forbidden
    }
    fn tests(&self) -> &[String] {
        &self.tests
    }
    fn brief(&self) -> &str {
        &self.brief
    }
    fn addresses(&self) -> &[String] {
        &self.addresses
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClaimGroupInfo {
    pub root_cause: String,
    pub claims: Vec<String>,
    pub contract_ids: Vec<String>,
    pub verdict: String,
    pub fix: String,
    pub owns: String,
    pub defect_class: String,
}

impl ClaimGroupLike for ClaimGroupInfo {
    fn root_cause(&self) -> &str {
        &self.root_cause
    }
    fn fix(&self) -> &str {
        &self.fix
    }
    fn verdict(&self) -> &str {
        &self.verdict
    }
    fn owns(&self) -> &str {
        &self.owns
    }
    fn defect_class(&self) -> &str {
        &self.defect_class
    }
}

// -----------------------------------------------------------------------------
// Rendering Functions
// -----------------------------------------------------------------------------

pub fn contract_section(mission: &impl MissionLike) -> String {
    let ids: BTreeSet<String> = mission.contract_ids().map(|s| s.to_string()).collect();
    let ids_str = if ids.is_empty() {
        "(none)".to_string()
    } else {
        ids.iter().map(|cid| format!("- {}", cid)).collect::<Vec<_>>().join("\n")
    };
    format!(
        "## Root contract (immutable)\n\n{}\n\n## Contract IDs (valid for verdict contract_ids)\n\n{}\n",
        mission.body().trim(),
        ids_str
    )
}

pub fn implementer(
    mission: &impl MissionLike,
    lane: &impl LaneLike,
    wave: u32,
    run_id: &str,
    role: Option<&str>,
    fix_groups: Option<&[impl ClaimGroupLike]>,
) -> String {
    let role = role.unwrap_or("implementer");
    let mut parts = vec![
        format!("# Gauntlet capsule — role: {} — run: {} — wave: {}", role, run_id, wave),
        "".to_string(),
        SAFETY.to_string(),
        contract_section(mission),
        "## Your lane".to_string(),
        "".to_string(),
        format!("lane-id: {}", lane.id()),
        format!("lane-owns: {}", serde_json::to_string(lane.owns()).unwrap_or_else(|_| "[]".to_string())),
        format!("lane-forbidden: {}", serde_json::to_string(lane.forbidden()).unwrap_or_else(|_| "[]".to_string())),
        format!("lane-tests: {}", serde_json::to_string(lane.tests()).unwrap_or_else(|_| "[]".to_string())),
        format!("wave: {}", wave),
        "".to_string(),
        format!("brief: {}", lane.brief()),
    ];
    let addresses = lane.addresses();
    if !addresses.is_empty() {
        parts.push("".to_string());
        parts.push(format!("addresses: {}", serde_json::to_string(addresses).unwrap_or_else(|_| "[]".to_string())));
    }
    if let Some(groups) = fix_groups {
        if !groups.is_empty() {
            parts.push("".to_string());
            parts.push("## Accepted findings to fix".to_string());
            parts.push("".to_string());
            for group in groups {
                parts.push(format!("- root cause: {}", group.root_cause()));
                let fix = group.fix();
                if !fix.is_empty() {
                    parts.push(format!("  fix: {}", fix));
                }
            }
        }
    }
    parts.push("".to_string());
    parts.push("## Instructions".to_string());
    parts.push("".to_string());
    parts.push("Implement the lane brief inside the current worktree. Touch only the".to_string());
    parts.push("paths your lane owns; never touch a forbidden path. Run the lane".to_string());
    parts.push("tests. Do not run git.".to_string());
    parts.push("".to_string());
    parts.push("## Expected output".to_string());
    parts.push("".to_string());
    parts.push(REPORT_FORMAT.to_string());

    parts.join("\n") + "\n"
}

fn history_section(
    fixed: Option<&[String]>,
    deferred: Option<&[String]>,
    dismissed: Option<&[String]>,
) -> Vec<String> {
    let mut parts = Vec::new();
    let sections = [
        ("Findings already accepted and fixed (verify, do not re-litigate)", fixed),
        ("Findings already accepted and deferred to the final polish pass (do not raise them again)", deferred),
        ("Findings already dismissed (do not re-open without NEW evidence)", dismissed),
    ];
    for (title, entries) in sections {
        if let Some(entries) = entries {
            if !entries.is_empty() {
                parts.push(format!("## {}", title));
                parts.push("".to_string());
                for entry in entries {
                    parts.push(format!("- {}", entry));
                }
                parts.push("".to_string());
            }
        }
    }
    parts
}

#[allow(clippy::too_many_arguments)]
pub fn reviewer(
    mission: &impl MissionLike,
    wave: u32,
    run_id: &str,
    diff_path: Option<&str>,
    fixed: Option<&[String]>,
    deferred: Option<&[String]>,
    dismissed: Option<&[String]>,
) -> String {
    let mut instructions = vec![
        "Review the integrated changes in this worktree against the contract.".to_string(),
    ];
    if let Some(diff_path) = diff_path {
        instructions.push(format!(
            "The full base-to-candidate diff is at {} — read it first; consult worktree files for surrounding context.",
            diff_path
        ));
    }
    instructions.push("You run READ-ONLY: do not modify any file. One global review of the".to_string());
    instructions.push("whole diff; report only defects backed by evidence, each mapped to a".to_string());
    instructions.push("contract ID where applicable. Style nitpicks and out-of-mission".to_string());
    instructions.push("findings are not FIX claims.".to_string());
    if wave > 0 {
        instructions.push(format!(
            "This is fix wave {}: your first job is to verify that the findings listed below were actually fixed, and only then to look for defects the previous rounds missed.",
            wave
        ));
    }

    let mut parts = vec![
        format!("# Gauntlet capsule — role: reviewer — run: {} — wave: {}", run_id, wave),
        "".to_string(),
        REVIEWER_STANCE.to_string(),
        "".to_string(),
        SAFETY.to_string(),
        contract_section(mission),
    ];
    parts.extend(history_section(fixed, deferred, dismissed));
    parts.push(REVIEW_DISCIPLINE.to_string());
    parts.push("".to_string());
    parts.push("## Instructions".to_string());
    parts.push("".to_string());
    parts.extend(instructions);
    parts.push("".to_string());
    parts.push("## Expected output".to_string());
    parts.push("".to_string());
    parts.push(VERDICT_FORMAT.to_string());

    parts.join("\n") + "\n"
}

pub fn judge(
    mission: &impl MissionLike,
    wave: u32,
    run_id: &str,
    review_json: &str,
    deferred: Option<&[String]>,
    dismissed: Option<&[String]>,
) -> String {
    let mut parts = vec![
        format!("# Gauntlet capsule — role: judge — run: {} — wave: {}", run_id, wave),
        "".to_string(),
        SAFETY.to_string(),
        contract_section(mission),
    ];
    parts.extend(history_section(None, deferred, dismissed));
    parts.push("## Reviewer claims to judge".to_string());
    parts.push("".to_string());
    parts.push("```json".to_string());
    parts.push(review_json.trim().to_string());
    parts.push("```".to_string());
    parts.push("".to_string());
    parts.push("## Instructions".to_string());
    parts.push("".to_string());
    parts.push("You run READ-ONLY. Judge all reviewer claims together against the".to_string());
    parts.push("contract: deduplicate them by root cause, dismiss style nitpicks and".to_string());
    parts.push("out-of-mission findings, and emit the final grouped verdict.".to_string());
    parts.push("".to_string());
    parts.push(JUDGE_RULE.to_string());
    parts.push("".to_string());
    parts.push("## Expected output".to_string());
    parts.push("".to_string());
    parts.push(VERDICT_FORMAT.to_string());

    parts.join("\n") + "\n"
}

pub fn polish(
    mission: &impl MissionLike,
    groups: &[impl ClaimGroupLike],
    wave: u32,
    run_id: &str,
    owns: Option<&[String]>,
) -> String {
    let owns_list: Vec<String> = owns.map(|o| o.to_vec()).unwrap_or_default();
    let mut parts = vec![
        format!("# Gauntlet capsule — role: polish — run: {} — wave: {}", run_id, wave),
        "".to_string(),
        SAFETY.to_string(),
        contract_section(mission),
        "## Non-blocking findings to clear".to_string(),
        "".to_string(),
    ];
    for group in groups {
        parts.push(format!("- [{}] {}", group.defect_class(), group.root_cause()));
        let fix = group.fix();
        if !fix.is_empty() {
            parts.push(format!("  fix: {}", fix));
        }
        let owns_str = group.owns();
        if !owns_str.is_empty() {
            parts.push(format!("  owns: {}", owns_str));
        }
    }
    if !owns_list.is_empty() {
        parts.push("".to_string());
        parts.push("lane-id: polish".to_string());
        parts.push(format!("lane-owns: {}", serde_json::to_string(&owns_list).unwrap_or_else(|_| "[]".to_string())));
        parts.push("lane-tests: []".to_string());
        parts.push(format!("wave: {}", wave));
    }
    parts.push("".to_string());
    parts.push("## Instructions".to_string());
    parts.push("".to_string());
    parts.push("Correct exactly these findings in the current worktree — nothing".to_string());
    parts.push("else. They are documentation drift and evidence gaps, not behavior:".to_string());
    parts.push("do not change program behavior, do not refactor, do not add".to_string());
    parts.push("machinery. Touch only the paths listed above. Do not run git.".to_string());
    parts.push("The repository gates run again on your result; if they fail, your".to_string());
    parts.push("whole pass is discarded and the candidate ships without it.".to_string());
    parts.push("".to_string());
    parts.push("## Expected output".to_string());
    parts.push("".to_string());
    parts.push(REPORT_FORMAT.to_string());

    parts.join("\n") + "\n"
}

pub fn planner(
    mission: &impl MissionLike,
    run_id: &str,
    groups: Option<&[impl ClaimGroupLike]>,
    complaint: Option<&str>,
) -> String {
    let mut parts = vec![
        format!("# Gauntlet capsule — role: planner — run: {}", run_id),
        "".to_string(),
        SAFETY.to_string(),
        contract_section(mission),
    ];
    if let Some(groups) = groups {
        if !groups.is_empty() {
            parts.push("## Accepted findings to address (fix-wave recut)".to_string());
            parts.push("".to_string());
            for group in groups {
                parts.push(format!("- {} (verdict {})", group.root_cause(), group.verdict()));
            }
            parts.push("".to_string());
            parts.push("Cut fix lanes addressing these root causes. Each lane's".to_string());
            parts.push("'addresses' lists the root causes it fixes.".to_string());
            parts.push("".to_string());
        } else {
            parts.push("## Instructions & Decision Framework".to_string());
            parts.push("".to_string());
            parts.push("1. **PRIORITY 1: Orthogonal Parallel Lanes (`gauntlet-plan`)**".to_string());
            parts.push("   - If subtasks can be decoupled with disjoint file sets (`owns`), output >= 2 parallel lanes (`gauntlet-plan`).".to_string());
            parts.push("   - Parallel lanes execute concurrently in isolated worktrees.".to_string());
            parts.push("".to_string());
            parts.push("2. **PRIORITY 2: Sequential Stage Pipeline (`gauntlet-stages`)**".to_string());
            parts.push("   - If subtasks have shared files, overlapping state, causal dependencies, or sequential steps (e.g. Step 1 -> Step 2 -> ...), DO NOT lump everything into a single massive lane.".to_string());
            parts.push("   - Instead, decompose into sequential stages (`gauntlet-stages`) so each phase executes incrementally through the full Gauntlet loop on top of verified previous output.".to_string());
            parts.push("   - A single monolithic lane containing an entire multi-step project is strictly forbidden when sequential stages can be created.".to_string());
            parts.push("".to_string());
        }
    } else {
        parts.push("## Instructions & Decision Framework".to_string());
        parts.push("".to_string());
        parts.push("1. **PRIORITY 1: Orthogonal Parallel Lanes (`gauntlet-plan`)**".to_string());
        parts.push("   - If subtasks can be decoupled with disjoint file sets (`owns`), output >= 2 parallel lanes (`gauntlet-plan`).".to_string());
        parts.push("   - Parallel lanes execute concurrently in isolated worktrees.".to_string());
        parts.push("".to_string());
        parts.push("2. **PRIORITY 2: Sequential Stage Pipeline (`gauntlet-stages`)**".to_string());
        parts.push("   - If subtasks have shared files, overlapping state, causal dependencies, or sequential steps (e.g. Step 1 -> Step 2 -> ...), DO NOT lump everything into a single massive lane.".to_string());
        parts.push("   - Instead, decompose into sequential stages (`gauntlet-stages`) so each phase executes incrementally through the full Gauntlet loop on top of verified previous output.".to_string());
        parts.push("   - A single monolithic lane containing an entire multi-step project is strictly forbidden when sequential stages can be created.".to_string());
        parts.push("".to_string());
    }

    if let Some(complaint) = complaint {
        if !complaint.is_empty() {
            parts.push("## Previous attempt rejected".to_string());
            parts.push("".to_string());
            parts.push(complaint.to_string());
            parts.push("".to_string());
        }
    }

    parts.push("## Expected output".to_string());
    parts.push("".to_string());
    parts.push(PLAN_FORMAT.to_string());

    parts.join("\n") + "\n"
}

pub fn checkpoint(name: &str, context: &str) -> String {
    format!(
        "# Gauntlet director checkpoint: {}\n\nYou are the mission director. Review the summary below, then reply\nwith 'approve' or 'reject' on its own line.\n\n{}\n",
        name, context
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_mission() -> MissionInfo {
        MissionInfo {
            body: "# Obj\n\n- AC-1: x\n".to_string(),
            contract_ids: vec!["AC-1".to_string()],
        }
    }

    fn test_lane() -> LaneInfo {
        LaneInfo {
            id: "L1".to_string(),
            owns: vec!["src/example/**".to_string()],
            forbidden: vec![],
            tests: vec!["true".to_string()],
            brief: "b".to_string(),
            addresses: vec![],
        }
    }

    #[test]
    fn test_implementer_capsule() {
        let m = test_mission();
        let lane = test_lane();
        let text = implementer(&m, &lane, 0, "RUN", None, None::<&[ClaimGroupInfo]>);
        assert!(text.contains("# Gauntlet capsule — role: implementer — run: RUN — wave: 0"));
        assert!(text.contains("lane-id: L1"));
        assert!(text.contains("lane-owns: [\"src/example/**\"]"));
        assert!(text.contains("lane-tests: [\"true\"]"));
        assert!(text.contains("brief: b"));
        assert!(text.contains("```gauntlet-report"));
    }

    #[test]
    fn test_reviewer_capsule() {
        let m = test_mission();
        let text = reviewer(
            &m,
            0,
            "RUN",
            Some("/run/reviews/diff-w0.patch"),
            None,
            None,
            None,
        );
        assert!(text.contains("/run/reviews/diff-w0.patch"));
        assert!(text.contains("you HATE what you are seeing"));
        assert!(text.contains("Antoine de Saint-Exupéry"));
        let stance_idx = text.find("You are a senior dev").unwrap();
        let safety_idx = text.find("## Safety").unwrap();
        assert!(stance_idx < safety_idx);
    }

    #[test]
    fn test_judge_capsule() {
        let m = test_mission();
        let text = judge(&m, 0, "RUN", "{\"groups\": []}", None, None);
        assert!(text.contains("FIX = justified AND aligned AND ("));
        assert!(text.contains("REDESIGN"));
        assert!(text.contains("REPORT_ONLY"));
        assert!(text.contains("never add a compensating layer"));
    }

    #[test]
    fn test_polish_capsule() {
        let m = test_mission();
        let groups = vec![ClaimGroupInfo {
            root_cause: "doc mismatch".to_string(),
            claims: vec![],
            contract_ids: vec!["AC-1".to_string()],
            verdict: "FIX".to_string(),
            fix: "update docs".to_string(),
            owns: "README.md".to_string(),
            defect_class: "doc_drift".to_string(),
        }];
        let text = polish(&m, &groups, 0, "RUN", Some(&["README.md".to_string()]));
        assert!(text.contains("- [doc_drift] doc mismatch"));
        assert!(text.contains("fix: update docs"));
        assert!(text.contains("owns: README.md"));
        assert!(text.contains("lane-id: polish"));
        assert!(text.contains("lane-owns: [\"README.md\"]"));
    }

    #[test]
    fn test_planner_capsule() {
        let m = test_mission();
        let text = planner(&m, "RUN", None::<&[ClaimGroupInfo]>, None);
        assert!(text.contains("PRIORITY 1: Orthogonal Parallel Lanes"));
        assert!(text.contains("PRIORITY 2: Sequential Stage Pipeline"));
    }

    #[test]
    fn test_checkpoint_capsule() {
        let text = checkpoint("plan", "summary details");
        assert!(text.contains("# Gauntlet director checkpoint: plan"));
        assert!(text.contains("summary details"));
    }
}
