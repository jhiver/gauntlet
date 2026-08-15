//! Run gate commands in the integration worktree.
//!
//! The orchestrator alone runs gate commands. Each command runs via bash with
//! output captured under `out_dir`; with `dry_run = true` commands are printed instead
//! of executed.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateResult {
    pub command: String,
    pub ok: bool,
    pub returncode: Option<i32>,
    pub log_path: Option<PathBuf>,
    #[serde(default)]
    pub detail: String,
}

impl GateResult {
    pub fn new(
        command: impl Into<String>,
        ok: bool,
        returncode: Option<i32>,
        log_path: Option<PathBuf>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            command: command.into(),
            ok,
            returncode,
            log_path,
            detail: detail.into(),
        }
    }
}

pub type LogCallback<'a> = &'a dyn Fn(&str);
pub type UiGateCallback<'a> = &'a dyn Fn(usize, usize, &str, bool, f64, &str);

#[allow(clippy::too_many_arguments)]
pub fn run_gates(
    commands: &[String],
    cwd: &Path,
    out_dir: &Path,
    dry_run: bool,
    log: Option<LogCallback<'_>>,
    ui_callback: Option<UiGateCallback<'_>>,
    timeout_s: u64,
) -> Vec<GateResult> {
    let _ = fs::create_dir_all(out_dir);
    let mut results = Vec::new();
    let total = commands.len();

    for (idx, command) in commands.iter().enumerate() {
        let gate_num = idx + 1;

        if dry_run {
            let msg = format!("DRY-RUN: (cd {} && {})", cwd.display(), command);
            if let Some(logger) = log {
                logger(&msg);
            } else {
                println!("{msg}");
            }
            results.push(GateResult::new(command.clone(), true, None, None, ""));
            continue;
        }

        let log_path = out_dir.join(format!("gate-{idx}.log"));
        let t0 = Instant::now();

        let log_file = match fs::File::create(&log_path) {
            Ok(f) => f,
            Err(e) => {
                let dur = t0.elapsed().as_secs_f64();
                let detail = format!("failed to create gate log file: {e}");
                if let Some(cb) = ui_callback {
                    cb(gate_num, total, command, false, dur, &detail);
                }
                results.push(GateResult::new(
                    command.clone(),
                    false,
                    None,
                    Some(log_path),
                    detail,
                ));
                continue;
            }
        };
        let err_file = match log_file.try_clone() {
            Ok(f) => f,
            Err(_) => {
                let dur = t0.elapsed().as_secs_f64();
                let detail = "failed to clone log file handle".to_string();
                if let Some(cb) = ui_callback {
                    cb(gate_num, total, command, false, dur, &detail);
                }
                results.push(GateResult::new(
                    command.clone(),
                    false,
                    None,
                    Some(log_path),
                    detail,
                ));
                continue;
            }
        };

        let mut child = match Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(err_file))
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let dur = t0.elapsed().as_secs_f64();
                let detail = format!("failed to spawn bash: {e}");
                let _ = fs::write(&log_path, &detail);
                if let Some(cb) = ui_callback {
                    cb(gate_num, total, command, false, dur, &detail);
                }
                results.push(GateResult::new(
                    command.clone(),
                    false,
                    None,
                    Some(log_path),
                    detail,
                ));
                continue;
            }
        };

        // Poll with timeout
        let timeout_duration = Duration::from_secs(timeout_s);
        let start = Instant::now();
        let mut timed_out = false;

        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if start.elapsed() >= timeout_duration {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => {
                    let _ = child.kill();
                    break None;
                }
            }
        };

        let dur = t0.elapsed().as_secs_f64();

        if timed_out || status.is_none() {
            let _ = fs::OpenOptions::new().append(true).open(&log_path).map(|mut f| {
                use std::io::Write;
                let _ = writeln!(f, "gate timed out after {timeout_s}s");
            });
            let detail = format!("timeout after {timeout_s}s");
            if let Some(cb) = ui_callback {
                cb(gate_num, total, command, false, dur, &detail);
            }
            results.push(GateResult::new(
                command.clone(),
                false,
                None,
                Some(log_path),
                detail,
            ));
        } else if let Some(exit_status) = status {
            let ok = exit_status.success();
            let returncode = exit_status.code();
            let mut detail = String::new();
            if !ok {
                if let Ok(content) = fs::read_to_string(&log_path) {
                    if let Some(last_line) = content.trim().lines().last() {
                        detail = last_line.to_string();
                    }
                }
            }

            if let Some(cb) = ui_callback {
                cb(gate_num, total, command, ok, dur, &detail);
            }

            results.push(GateResult::new(
                command.clone(),
                ok,
                returncode,
                Some(log_path),
                detail,
            ));
        }
    }

    results
}

pub fn all_ok(results: &[GateResult]) -> bool {
    results.iter().all(|r| r.ok)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(prefix: &str) -> Self {
            let mut p = std::env::temp_dir();
            let rand_num: u128 = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            p.push(format!("gauntlet-test-{prefix}-{rand_num}"));
            fs::create_dir_all(&p).unwrap();
            Self { path: p }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn test_passing_gate() {
        let tmp = TempDir::new("pass");
        let out_dir = tmp.path.join("out");
        let results = run_gates(
            &["true".to_string()],
            &tmp.path,
            &out_dir,
            false,
            None,
            None,
            600,
        );
        assert!(all_ok(&results));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].returncode, Some(0));
        assert!(results[0].log_path.as_ref().unwrap().is_file());
    }

    #[test]
    fn test_failing_gate() {
        let tmp = TempDir::new("fail");
        let out_dir = tmp.path.join("out");
        let results = run_gates(
            &["false".to_string()],
            &tmp.path,
            &out_dir,
            false,
            None,
            None,
            600,
        );
        assert!(!all_ok(&results));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].returncode, Some(1));
    }

    #[test]
    fn test_gate_runs_in_cwd() {
        let tmp = TempDir::new("cwd");
        let out_dir = tmp.path.join("out");
        let cmd = vec!["test -f marker.txt".to_string()];

        let results1 = run_gates(&cmd, &tmp.path, &out_dir, false, None, None, 600);
        assert!(!all_ok(&results1));

        fs::write(tmp.path.join("marker.txt"), "x").unwrap();

        let results2 = run_gates(&cmd, &tmp.path, &out_dir, false, None, None, 600);
        assert!(all_ok(&results2));
    }

    #[test]
    fn test_dry_run_prints_and_passes() {
        let tmp = TempDir::new("dry");
        let out_dir = tmp.path.join("out");
        let logs = std::sync::Arc::new(Mutex::new(Vec::new()));
        let logs_clone = logs.clone();

        let log_fn = move |msg: &str| {
            logs_clone.lock().unwrap().push(msg.to_string());
        };

        let results = run_gates(
            &["false".to_string()],
            &tmp.path.join("missing"),
            &out_dir,
            true,
            Some(&log_fn),
            None,
            600,
        );
        assert!(all_ok(&results));
        let captured = logs.lock().unwrap();
        assert!(captured.iter().any(|m| m.contains("DRY-RUN") && m.contains("false")));
    }
}
