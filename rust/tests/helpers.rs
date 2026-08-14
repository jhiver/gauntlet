//! Shared fixtures for Gauntlet tests: tmp git repos, missions, configs.
//!
//! Git mutations happen only inside per-test tempdirs, never in a real repo.

use std::path::{Path, PathBuf};
use std::process::Command;

#[allow(dead_code)]
pub fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .expect("failed to execute git command");

    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).to_string()
}

pub fn make_git_repo(path: &Path) -> PathBuf {
    std::fs::create_dir_all(path).unwrap();
    let _ = Command::new("git")
        .args(["init", "-b", "main"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.email", "test@example.com"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(path)
        .output();

    std::fs::write(path.join("README.md"), "# fixture repo\n").unwrap();
    let _ = Command::new("git")
        .args(["add", "-A"])
        .current_dir(path)
        .output();
    let _ = Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(path)
        .output();

    path.to_path_buf()
}

pub const MISSION_TEMPLATE: &str = r#"+++
slug = "{slug}"

[[repos]]
path = "{repo}"
target_branch = "main"
gates = {gates}

{lanes}+++

# Objective

Test mission.

## AC

- AC-1: The example lane exists and its tests pass.

## INV

- INV-1: No file outside the lane owns is modified.

## NG

- NG-1: No public API change.
"#;

pub const LANE_TEMPLATE: &str = r#"[[lanes]]
id = "{lid}"
owns = [{owns}]
forbidden = [{forbidden}]
tests = ["true"]
brief = "Lane {lid} brief."

"#;

pub struct LaneSpec<'a> {
    pub lid: &'a str,
    pub owns: &'a str,
    pub forbidden: &'a str,
}

pub fn write_mission(
    path: &Path,
    repo: &Path,
    slug: &str,
    lanes: Option<&[LaneSpec]>,
    gates: Option<&[&str]>,
) -> PathBuf {
    let lanes_toml = match lanes {
        Some(ls) => ls
            .iter()
            .map(|l| {
                LANE_TEMPLATE
                    .replace("{lid}", l.lid)
                    .replace("{owns}", l.owns)
                    .replace("{forbidden}", l.forbidden)
            })
            .collect::<Vec<_>>()
            .join(""),
        None => LANE_TEMPLATE
            .replace("{lid}", "L1")
            .replace("{owns}", "\"src/example/**\"")
            .replace("{forbidden}", ""),
    };

    let gates_toml = match gates {
        Some(gs) => {
            let json_items: Vec<String> = gs
                .iter()
                .map(|g| serde_json::to_string(g).unwrap())
                .collect();
            format!("[{}]", json_items.join(", "))
        }
        None => "[\"true\"]".to_string(),
    };

    let content = MISSION_TEMPLATE
        .replace("{slug}", slug)
        .replace("{repo}", &repo.to_string_lossy())
        .replace("{gates}", &gates_toml)
        .replace("{lanes}", &lanes_toml);

    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    std::fs::write(path, content).unwrap();
    path.to_path_buf()
}

pub const ECHO_CONFIG: &str = r#"# Test config: every role resolved by the echo harness.
[roles.implementer]
chain = [ { harness = "echo" } ]
[roles.fixer]
chain = [ { harness = "echo" } ]
[roles.reviewer]
chain = [ { harness = "echo" } ]
[roles.judge]
chain = [ { harness = "echo" } ]
[roles.planner]
chain = [ { harness = "echo" } ]
[roles.director]
chain = [ { harness = "human" } ]

[policy]
checkpoints = []
max_total_waves = 5
on_wave_cap = "checkpoint"
idle_timeout_s = 5
hard_timeout_s = 30
lane_timeout_s = 30

[fallback]
on_quota = "next_and_break"
on_auth = "break"
on_rate_limit = "backoff_retry_then_next"
on_timeout = "retry_once_then_next"
on_crash = "retry_once_then_next"
on_invalid_output = "retry_once_then_next"
max_attempts_per_task = 3
backoff_s = 0
"#;

pub fn write_echo_config(path: &Path) -> PathBuf {
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    std::fs::write(path, ECHO_CONFIG).unwrap();
    path.to_path_buf()
}
