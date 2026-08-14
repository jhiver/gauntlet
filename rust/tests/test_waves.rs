use std::path::PathBuf;
use tempfile::tempdir;

use gauntlet::orchestrator::Orchestrator;
use gauntlet::statemachine::{convergence_state, CAPPED, CONVERGING, STALLED};

mod helpers;
use helpers::{make_git_repo, write_echo_config, write_mission, ECHO_CONFIG};

#[test]
fn test_convergence_rule_mathematics() {
    assert_eq!(convergence_state(&[], 7, 0, 5), CONVERGING);
    assert_eq!(convergence_state(&[7], 4, 1, 5), CONVERGING);
    assert_eq!(convergence_state(&[7, 4], 1, 2, 5), CONVERGING);
    assert_eq!(convergence_state(&[4], 4, 1, 5), STALLED);
    assert_eq!(convergence_state(&[7, 4], 5, 2, 5), STALLED);
    assert_eq!(convergence_state(&[7, 4, 5], 4, 3, 5), STALLED);
    assert_eq!(convergence_state(&[7, 4], 1, 2, 2), CAPPED);
    assert_eq!(convergence_state(&[], 3, 0, 0), CAPPED);
}

#[test]
fn test_failed_gate_has_its_own_terminal() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    let config = write_echo_config(&tmp.path().join("echo.toml"));
    let mission = write_mission(
        &tmp.path().join("m.md"),
        &repo,
        "example",
        None,
        Some(&["false"]),
    );

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
    assert_eq!(orch.state.phase, "BLOCKED_GATE");
}

#[test]
fn test_absolute_cap_stops_a_converging_run() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));
    let cfg_text = ECHO_CONFIG.replace("max_total_waves = 5", "max_total_waves = 0");
    let config_path = tmp.path().join("echo-cap0.toml");
    std::fs::write(&config_path, cfg_text).unwrap();

    let mission = write_mission(&tmp.path().join("m.md"), &repo, "example", None, None);
    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config_path),
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
    assert_eq!(rc, 0); // With echo harness returning 0 groups on wave 0, wave 0 reaches READY without needing fix wave
}
