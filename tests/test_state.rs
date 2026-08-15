use tempfile::tempdir;

use gauntlet::statemachine::{load, save, LaneState, State};

#[test]
fn test_state_round_trip() {
    let tmp = tempdir().unwrap();
    let run_dir = tmp.path().join(".missions/20260814-test");
    std::fs::create_dir_all(&run_dir).unwrap();

    let state = State {
        slug: "test".to_string(),
        repo: "/path/to/repo".to_string(),
        target_branch: "main".to_string(),
        base_commit: "abc1234".to_string(),
        run_id: "20260814-test".to_string(),
        run_dir: Some(run_dir.to_string_lossy().to_string()),
        wave: 2,
        phase: "IMPLEMENT".to_string(),
        lanes: vec![LaneState {
            id: "L1".to_string(),
            owns: vec!["src/**".to_string()],
            forbidden: vec!["src/secret.rs".to_string()],
            tests: vec!["cargo test".to_string()],
            brief: "test brief".to_string(),
            addresses: vec!["AC-1".to_string()],
            status: "done".to_string(),
            detail: "detail".to_string(),
            changed: vec!["src/main.rs".to_string()],
            claimed: vec!["src/main.rs".to_string()],
        }],
        stages: vec![],
        gates: vec!["cargo test".to_string()],
        auto: true,
        dry_run: false,
        reviews: vec!["verdicts/review-w0.json".to_string()],
        judgments: vec!["verdicts/judgment-w0.json".to_string()],
        blocking_history: vec![3, 1],
        polish_done: false,
        polish_detail: "".to_string(),
        integrated_changes: true,
        worktrees: vec!["/path/to/wt".to_string()],
        branches: vec!["gauntlet/wt".to_string()],
        blocked_reason: None,
        blocked_kind: None,
        blocked_phase: None,
        harness_health: Default::default(),
        auto_heal_attempts: 0,
        gate_auto_heal_attempts: 0,
        safety_pruned_files: Vec::new(),
    };

    save(&state).unwrap();
    let loaded = load(&run_dir).unwrap();

    assert_eq!(loaded.slug, "test");
    assert_eq!(loaded.wave, 2);
    assert_eq!(loaded.phase, "IMPLEMENT");
    assert_eq!(loaded.lanes.len(), 1);
    assert_eq!(loaded.lanes[0].id, "L1");
    assert_eq!(loaded.blocking_history, vec![3, 1]);
}
