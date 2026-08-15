use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

use gauntlet::orchestrator::Orchestrator;
use gauntlet::statemachine::load;

mod helpers;
use helpers::{git, make_git_repo, write_echo_config, write_mission, LaneSpec};

#[test]
fn test_full_loop_echo_reaches_ready() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    let config = write_echo_config(&tmp.path().join("echo.toml"));
    let mission = write_mission(&tmp.path().join("m.md"), &repo, "example", None, None);

    let logs = Arc::new(Mutex::new(Vec::new()));
    let logs_clone = Arc::clone(&logs);
    let log_fn = Arc::new(move |msg: &str| {
        logs_clone.lock().unwrap().push(msg.to_string());
    });

    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config),
        true,
        false,
        None,
        false,
        0,
        2,
        Some(log_fn),
    )
    .unwrap();

    let rc = orch.run();
    assert_eq!(rc, 0);

    let state = load(orch.run_dir.as_ref().unwrap()).unwrap();
    assert_eq!(state.phase, "READY");

    // The echo lane file was delivered into target branch
    let delivered = repo.join("src").join("example").join("echo-L1-w0.md");
    assert!(delivered.is_file(), "delivered file should exist at {}", delivered.display());

    // Worktrees and gauntlet branches were cleaned up
    assert!(!Path::new(&state.worktrees[0]).exists());
    let branches = git(&repo, &["branch", "--list", "gauntlet/*"]);
    assert_eq!(branches.trim(), "");

    // Run dir artifacts exist
    let run_dir = Path::new(state.run_dir.as_ref().unwrap());
    assert!(run_dir.join("mission.md").is_file());
    assert!(run_dir.join("config.toml").is_file());
    assert!(run_dir.join("state.json").is_file());
    assert!(run_dir.join("report.md").is_file());

    let review_text = std::fs::read_to_string(run_dir.join("verdicts").join("review-w0.json")).unwrap();
    let review_val: serde_json::Value = serde_json::from_str(&review_text).unwrap();
    assert_eq!(review_val, serde_json::json!({ "groups": [] }));

    // Lane capsule was deleted after successful integration
    assert!(!run_dir.join("capsules").join("implementer-L1-w0.md").exists());
}

#[test]
fn test_full_loop_without_lanes_uses_planner() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    let config = write_echo_config(&tmp.path().join("echo.toml"));
    let mission = write_mission(&tmp.path().join("m.md"), &repo, "example", Some(&[]), None);

    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config),
        true,
        false,
        None,
        false,
        0,
        2,
        None,
    )
    .unwrap();

    let rc = orch.run();
    assert_eq!(rc, 0);

    let state = load(orch.run_dir.as_ref().unwrap()).unwrap();
    assert_eq!(state.phase, "READY");
    assert!(repo.join("echo-E1-w0.md").is_file());
    assert_eq!(state.lanes.iter().map(|l| l.id.as_str()).collect::<Vec<_>>(), vec!["E1"]);
}

#[test]
fn test_dry_run_prints_commands_and_changes_nothing() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    let config = write_echo_config(&tmp.path().join("echo.toml"));
    let mission = write_mission(&tmp.path().join("m.md"), &repo, "example", None, None);

    let logs = Arc::new(Mutex::new(Vec::new()));
    let logs_clone = Arc::clone(&logs);
    let log_fn = Arc::new(move |msg: &str| {
        logs_clone.lock().unwrap().push(msg.to_string());
    });

    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config),
        true,
        true,
        None,
        false,
        0,
        2,
        Some(log_fn),
    )
    .unwrap();

    let rc = orch.run();
    assert_eq!(rc, 0);

    let state = load(orch.run_dir.as_ref().unwrap()).unwrap();
    assert_eq!(state.phase, "READY_NO_CHANGE");

    let out = logs.lock().unwrap().join("\n");
    assert!(out.contains("DRY-RUN: (cd"));
    assert!(out.contains("git worktree add"));
    assert!(out.contains("git branch -D"));
    assert!(out.contains("&& true)"));

    assert!(!repo.join("src").exists());
}

#[test]
fn test_resume_after_init_completes() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    let config = write_echo_config(&tmp.path().join("echo.toml"));
    let mission = write_mission(&tmp.path().join("m.md"), &repo, "example", None, None);

    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config),
        true,
        false,
        None,
        false,
        0,
        2,
        None,
    )
    .unwrap();

    orch._phase_init().unwrap();
    assert_eq!(orch.state.phase, "PLAN");
    let run_dir = orch.run_dir.clone().unwrap();

    let mut orch2 = Orchestrator::new(
        &tool_dir,
        None,
        Some(&run_dir),
        None,
        true,
        false,
        None,
        false,
        0,
        2,
        None,
    )
    .unwrap();

    let rc = orch2.run();
    assert_eq!(rc, 0);
    assert_eq!(orch2.state.phase, "READY");
    assert_eq!(orch2.state.run_id, orch.state.run_id);
    assert!(repo.join("src").join("example").join("echo-L1-w0.md").is_file());
}

#[test]
fn test_init_refuses_staged_changes() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    std::fs::write(repo.join("dirty.txt"), "staged\n").unwrap();
    git(&repo, &["add", "dirty.txt"]);

    let config = write_echo_config(&tmp.path().join("echo.toml"));
    let mission = write_mission(&tmp.path().join("m.md"), &repo, "example", None, None);

    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config),
        true,
        false,
        None,
        false,
        0,
        2,
        None,
    )
    .unwrap();

    let rc = orch.run();
    assert_eq!(rc, 2);
    assert_eq!(orch.state.phase, "BLOCKED");
    assert!(orch.state.blocked_reason.unwrap().contains("staged changes"));
}

#[test]
fn test_overlapping_prewritten_lanes_blocked() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    let config = write_echo_config(&tmp.path().join("echo.toml"));
    let lanes = vec![
        LaneSpec { lid: "L1", owns: "\"src/**\"", forbidden: "" },
        LaneSpec { lid: "L2", owns: "\"src/example/**\"", forbidden: "" },
    ];
    let mission = write_mission(&tmp.path().join("m.md"), &repo, "example", Some(&lanes), None);

    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config),
        true,
        false,
        None,
        false,
        0,
        2,
        None,
    )
    .unwrap();

    let rc = orch.run();
    assert_eq!(rc, 2);
    assert_eq!(orch.state.phase, "BLOCKED");
    assert!(orch.state.blocked_reason.unwrap().contains("overlap"));
}
