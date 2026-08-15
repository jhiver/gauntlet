//! Phases, transitions, state.json persistence.
//!
//! state.json is rewritten after every phase transition and every lane status
//! change (atomic write). --resume reloads it and re-enters at the recorded
//! phase; phase handlers are written to be re-entrant.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const PHASES: &[&str] = &[
    "INIT",
    "PLAN",
    "PLAN_CHECKPOINT",
    "STAGES",
    "IMPLEMENT",
    "INSPECT",
    "INTEGRATE",
    "GATES",
    "REVIEW",
    "JUDGE",
    "PLAN_FIX",
    "POLISH",
    "DELIVER_CHECKPOINT",
    "DELIVER",
    "READY",
    "READY_NO_CHANGE",
    "BLOCKED",
    "BLOCKED_CONVERGENCE",
    "BLOCKED_ARCHITECTURE",
    "BLOCKED_GATE",
    "BLOCKED_HARNESS",
];

pub const BLOCKED_TERMINALS: &[&str] = &[
    "BLOCKED",
    "BLOCKED_CONVERGENCE",
    "BLOCKED_ARCHITECTURE",
    "BLOCKED_GATE",
    "BLOCKED_HARNESS",
];

pub const TERMINALS: &[&str] = &[
    "READY",
    "READY_NO_CHANGE",
    "BLOCKED",
    "BLOCKED_CONVERGENCE",
    "BLOCKED_ARCHITECTURE",
    "BLOCKED_GATE",
    "BLOCKED_HARNESS",
];

pub const LANE_ACTIVE: &[&str] = &["pending", "failed"];

pub const CONVERGING: &str = "converging";
pub const STALLED: &str = "stalled";
pub const CAPPED: &str = "capped";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum StatemachineError {
    #[error("{0}")]
    Message(String),
}

impl From<String> for StatemachineError {
    fn from(s: String) -> Self {
        StatemachineError::Message(s)
    }
}

impl From<&str> for StatemachineError {
    fn from(s: &str) -> Self {
        StatemachineError::Message(s.to_string())
    }
}

/// Decide whether another fix wave is justified.
///
/// `history` holds the blocking-group count of every previous judgment,
/// `count` the current one. A wave is granted while the mission converges —
/// each round must beat the best round so far, so an oscillation
/// (7 -> 4 -> 5) counts as stalled, not as progress.
pub fn convergence_state(
    history: &[usize],
    count: usize,
    wave: usize,
    max_total_waves: usize,
) -> &'static str {
    if wave >= max_total_waves {
        return CAPPED;
    }
    if let Some(&min_h) = history.iter().min() {
        if count >= min_h {
            return STALLED;
        }
    }
    CONVERGING
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaneState {
    pub id: String,
    pub owns: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub addresses: Vec<String>,
    #[serde(default = "default_lane_status")]
    pub status: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub changed: Vec<String>,
    #[serde(default)]
    pub claimed: Vec<String>,
}

fn default_lane_status() -> String {
    "pending".to_string()
}

impl Default for LaneState {
    fn default() -> Self {
        Self {
            id: String::new(),
            owns: Vec::new(),
            forbidden: Vec::new(),
            tests: Vec::new(),
            brief: String::new(),
            addresses: Vec::new(),
            status: default_lane_status(),
            detail: String::new(),
            changed: Vec::new(),
            claimed: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct State {
    #[serde(default)]
    pub run_id: String,
    #[serde(default)]
    pub slug: String,
    #[serde(default = "default_phase")]
    pub phase: String,
    #[serde(default)]
    pub wave: usize,
    #[serde(default)]
    pub repo: String,
    #[serde(default = "default_target_branch")]
    pub target_branch: String,
    #[serde(default)]
    pub gates: Vec<String>,
    #[serde(default)]
    pub base_commit: String,
    #[serde(default)]
    pub run_dir: Option<String>,
    #[serde(default)]
    pub lanes: Vec<LaneState>,
    #[serde(default)]
    pub stages: Vec<serde_json::Value>,
    #[serde(default)]
    pub harness_health: HashMap<String, String>,
    #[serde(default)]
    pub reviews: Vec<String>,
    #[serde(default)]
    pub judgments: Vec<String>,
    #[serde(default)]
    pub worktrees: Vec<String>,
    #[serde(default)]
    pub branches: Vec<String>,
    #[serde(default)]
    pub integrated_changes: bool,
    #[serde(default)]
    pub blocking_history: Vec<usize>,
    #[serde(default)]
    pub polish_done: bool,
    #[serde(default)]
    pub polish_detail: String,
    #[serde(default)]
    pub blocked_reason: Option<String>,
    #[serde(default)]
    pub blocked_kind: Option<String>,
    #[serde(default)]
    pub blocked_phase: Option<String>,
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub auto_heal_attempts: usize,
    #[serde(default)]
    pub gate_auto_heal_attempts: usize,
    #[serde(default)]
    pub safety_pruned_files: Vec<String>,
}

fn default_phase() -> String {
    "INIT".to_string()
}
fn default_target_branch() -> String {
    "main".to_string()
}

impl Default for State {
    fn default() -> Self {
        Self {
            run_id: String::new(),
            slug: String::new(),
            phase: default_phase(),
            wave: 0,
            repo: String::new(),
            target_branch: default_target_branch(),
            gates: Vec::new(),
            base_commit: String::new(),
            run_dir: None,
            lanes: Vec::new(),
            stages: Vec::new(),
            harness_health: HashMap::new(),
            reviews: Vec::new(),
            judgments: Vec::new(),
            worktrees: Vec::new(),
            branches: Vec::new(),
            integrated_changes: false,
            blocking_history: Vec::new(),
            polish_done: false,
            polish_detail: String::new(),
            blocked_reason: None,
            blocked_kind: None,
            blocked_phase: None,
            auto: false,
            dry_run: false,
            auto_heal_attempts: 0,
            gate_auto_heal_attempts: 0,
            safety_pruned_files: Vec::new(),
        }
    }
}

impl State {
    pub fn from_dict(val: serde_json::Value) -> Result<Self, StatemachineError> {
        serde_json::from_value(val)
            .map_err(|e| StatemachineError::Message(format!("invalid state dict: {e}")))
    }

    pub fn to_dict(&self) -> Result<serde_json::Value, StatemachineError> {
        serde_json::to_value(self)
            .map_err(|e| StatemachineError::Message(format!("cannot serialize state: {e}")))
    }
}

/// Atomic rewrite of state.json inside the run directory.
pub fn save(state: &State) -> Result<PathBuf, StatemachineError> {
    let run_dir_str = match &state.run_dir {
        Some(d) if !d.is_empty() => d,
        _ => return Err(StatemachineError::Message("state.run_dir is not set".to_string())),
    };

    let path = Path::new(run_dir_str).join("state.json");
    let tmp = Path::new(run_dir_str).join("state.json.tmp");

    let json_text = serde_json::to_string_pretty(state)
        .map_err(|e| StatemachineError::Message(format!("cannot serialize state: {e}")))?;

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    std::fs::write(&tmp, format!("{json_text}\n"))
        .map_err(|e| StatemachineError::Message(format!("cannot write state file {}: {e}", tmp.display())))?;

    std::fs::rename(&tmp, &path)
        .map_err(|e| StatemachineError::Message(format!("cannot rename temp state file {}: {e}", path.display())))?;

    Ok(path)
}

pub fn load(run_dir: &Path) -> Result<State, StatemachineError> {
    let path = run_dir.join("state.json");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| StatemachineError::Message(format!("cannot read state file {}: {e}", path.display())))?;

    let mut state: State = serde_json::from_str(&text)
        .map_err(|e| StatemachineError::Message(format!("invalid JSON in {}: {e}", path.display())))?;

    state.run_dir = Some(run_dir.to_string_lossy().to_string());
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_save_load_round_trip() {
        let dir = tempdir().unwrap();
        let state = State {
            run_id: "20260814-demo".to_string(),
            slug: "demo".to_string(),
            phase: "JUDGE".to_string(),
            wave: 1,
            repo: "/tmp/repo".to_string(),
            target_branch: "main".to_string(),
            gates: vec!["true".to_string()],
            base_commit: "abc123".to_string(),
            run_dir: Some(dir.path().to_str().unwrap().to_string()),
            lanes: vec![LaneState {
                id: "L1".to_string(),
                owns: vec!["src/**".to_string()],
                status: "integrated".to_string(),
                changed: vec!["src/a.md".to_string()],
                ..Default::default()
            }],
            harness_health: [("agy".to_string(), "open".to_string())]
                .into_iter()
                .collect(),
            reviews: vec!["verdicts/review-w0.json".to_string()],
            worktrees: vec!["/tmp/repo-worktree-gauntlet-20260814-demo".to_string()],
            branches: vec!["gauntlet/20260814-demo/integration".to_string()],
            integrated_changes: true,
            auto: true,
            dry_run: false,
            ..Default::default()
        };

        save(&state).unwrap();
        let loaded = load(dir.path()).unwrap();
        assert_eq!(loaded.run_id, state.run_id);
        assert_eq!(loaded.phase, state.phase);
        assert_eq!(loaded.wave, state.wave);
        assert_eq!(loaded.lanes.len(), 1);
        assert_eq!(loaded.lanes[0].id, "L1");
        assert_eq!(loaded.lanes[0].status, "integrated");
        assert_eq!(loaded.harness_health.get("agy"), Some(&"open".to_string()));
    }

    #[test]
    fn test_from_dict_ignores_unknown_keys() {
        let json_val = serde_json::json!({
            "run_id": "x",
            "future_field": 1
        });
        let state = State::from_dict(json_val).unwrap();
        assert_eq!(state.run_id, "x");
        assert_eq!(state.phase, "INIT");
    }

    #[test]
    fn test_save_requires_run_dir() {
        let state = State::default();
        assert!(save(&state).is_err());
    }

    #[test]
    fn test_convergence_states() {
        assert_eq!(convergence_state(&[], 7, 0, 5), CONVERGING);
        assert_eq!(convergence_state(&[7], 4, 1, 5), CONVERGING);
        assert_eq!(convergence_state(&[7, 4], 1, 2, 5), CONVERGING);
        assert_eq!(convergence_state(&[4], 4, 1, 5), STALLED);
        assert_eq!(convergence_state(&[7, 4], 5, 2, 5), STALLED);
        assert_eq!(convergence_state(&[7, 4, 5], 4, 3, 5), STALLED);
        assert_eq!(convergence_state(&[7, 4], 1, 2, 2), CAPPED);
        assert_eq!(convergence_state(&[], 3, 0, 0), CAPPED);
    }
}
