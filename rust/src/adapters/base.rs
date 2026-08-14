//! Harness adapter interface and shared subprocess machinery.
//!
//! See DESIGN.md section "Adapter interface". run() is blocking; the
//! orchestrator runs lanes in threads (one per lane).

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

pub const STDERR_TAIL: usize = 4000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FailureKind {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "quota")]
    QuotaExhausted,
    #[serde(rename = "rate_limit")]
    RateLimited,
    #[serde(rename = "auth")]
    AuthExpired,
    #[serde(rename = "model_unavailable")]
    ModelUnavailable,
    #[serde(rename = "timeout_idle")]
    TimeoutIdle,
    #[serde(rename = "timeout_hard")]
    TimeoutHard,
    #[serde(rename = "crash")]
    Crash,
    #[serde(rename = "partial")]
    PartialDelivery,
    #[serde(rename = "invalid_output")]
    OutputInvalid,
}

impl FailureKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureKind::None => "none",
            FailureKind::QuotaExhausted => "quota",
            FailureKind::RateLimited => "rate_limit",
            FailureKind::AuthExpired => "auth",
            FailureKind::ModelUnavailable => "model_unavailable",
            FailureKind::TimeoutIdle => "timeout_idle",
            FailureKind::TimeoutHard => "timeout_hard",
            FailureKind::Crash => "crash",
            FailureKind::PartialDelivery => "partial",
            FailureKind::OutputInvalid => "invalid_output",
        }
    }

    pub fn policy_key(&self) -> &'static str {
        match self {
            FailureKind::QuotaExhausted => "on_quota",
            FailureKind::AuthExpired => "on_auth",
            FailureKind::RateLimited => "on_rate_limit",
            FailureKind::ModelUnavailable => "on_model_unavailable",
            FailureKind::TimeoutIdle | FailureKind::TimeoutHard => "on_timeout",
            FailureKind::Crash | FailureKind::PartialDelivery => "on_crash",
            FailureKind::OutputInvalid => "on_invalid_output",
            FailureKind::None => "none",
        }
    }
}

impl fmt::Display for FailureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResult {
    pub failure: FailureKind,
    pub exit_code: Option<i32>,
    pub output_path: PathBuf,
    pub detail: String,
}

impl RunResult {
    pub fn new(
        failure: FailureKind,
        exit_code: Option<i32>,
        output_path: PathBuf,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            failure,
            exit_code,
            output_path,
            detail: detail.into(),
        }
    }
}

pub trait HarnessAdapter: Send + Sync {
    fn name(&self) -> &str;
    fn supports_write(&self) -> bool;
    #[allow(clippy::too_many_arguments)]
    fn run(
        &self,
        capsule: &Path,
        worktree: &Path,
        write: bool,
        model: Option<&str>,
        effort: Option<&str>,
        hard_timeout_s: u64,
        idle_timeout_s: Option<u64>,
        out_dir: &Path,
        role: &str,
        lane_id: Option<&str>,
    ) -> RunResult;
    fn describe(
        &self,
        _capsule: &Path,
        _worktree: &Path,
        _write: bool,
        _model: Option<&str>,
        _effort: Option<&str>,
    ) -> String {
        format!("<{} harness>", self.name())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ErrorPatternsConfig {
    pub quota: Option<Vec<String>>,
    pub auth: Option<Vec<String>>,
    pub rate_limit: Option<Vec<String>>,
    pub model_unavailable: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AdapterConfig {
    pub adapter: Option<String>,
    pub supports_write: Option<bool>,
    pub default_model: Option<String>,
    pub launcher: Option<String>,
    pub errors: Option<ErrorPatternsConfig>,
}

pub struct SubprocessAdapter {
    pub name: String,
    pub supports_write: bool,
    pub default_model: Option<String>,
    pub compiled_errors: HashMap<FailureKind, Vec<Regex>>,
    pub jsonl_output: bool,
    pub counter: AtomicUsize,
}

impl SubprocessAdapter {
    pub fn new(name: &str, cfg: Option<&AdapterConfig>, jsonl_output: bool) -> Self {
        let default_model = cfg.and_then(|c| c.default_model.clone());
        let supports_write = cfg.and_then(|c| c.supports_write).unwrap_or(false);

        let mut compiled = HashMap::new();
        if let Some(cfg) = cfg {
            if let Some(ref errs) = cfg.errors {
                if let Some(ref pats) = errs.quota {
                    compiled.insert(FailureKind::QuotaExhausted, Self::compile_patterns(pats));
                }
                if let Some(ref pats) = errs.auth {
                    compiled.insert(FailureKind::AuthExpired, Self::compile_patterns(pats));
                }
                if let Some(ref pats) = errs.rate_limit {
                    compiled.insert(FailureKind::RateLimited, Self::compile_patterns(pats));
                }
                if let Some(ref pats) = errs.model_unavailable {
                    compiled.insert(FailureKind::ModelUnavailable, Self::compile_patterns(pats));
                }
            }
        }

        Self {
            name: name.to_string(),
            supports_write,
            default_model,
            compiled_errors: compiled,
            jsonl_output,
            counter: AtomicUsize::new(0),
        }
    }

    pub fn compile_patterns(patterns: &[String]) -> Vec<Regex> {
        patterns
            .iter()
            .filter_map(|p| {
                RegexBuilder::new(p)
                    .case_insensitive(true)
                    .build()
                    .ok()
            })
            .collect()
    }

    pub fn classify(&self, stderr_tail: &str) -> Option<FailureKind> {
        let order = [
            FailureKind::QuotaExhausted,
            FailureKind::AuthExpired,
            FailureKind::RateLimited,
            FailureKind::ModelUnavailable,
        ];
        for kind in order {
            if let Some(patterns) = self.compiled_errors.get(&kind) {
                for pat in patterns {
                    if pat.is_match(stderr_tail) {
                        return Some(kind);
                    }
                }
            }
        }
        None
    }

    pub fn tail(path: &Path, n: usize) -> String {
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(_) => return String::new(),
        };
        let len = match file.metadata() {
            Ok(m) => m.len() as usize,
            Err(_) => return String::new(),
        };
        let read_len = n.min(len);
        let offset = len.saturating_sub(read_len);
        if file.seek(SeekFrom::Start(offset as u64)).is_err() {
            return String::new();
        }
        let mut buf = vec![0u8; read_len];
        if file.read_exact(&mut buf).is_err() {
            return String::new();
        }
        String::from_utf8_lossy(&buf).to_string()
    }

    pub fn finalize_stream(out_path: &Path) -> std::io::Result<()> {
        let raw_path = out_path.with_extension("raw");
        if !out_path.exists() {
            return Ok(());
        }
        fs::rename(out_path, &raw_path)?;

        fn extract_strings(value: &serde_json::Value, sink: &mut Vec<String>) {
            match value {
                serde_json::Value::String(s) => sink.push(s.clone()),
                serde_json::Value::Array(arr) => {
                    for item in arr {
                        extract_strings(item, sink);
                    }
                }
                serde_json::Value::Object(map) => {
                    for val in map.values() {
                        extract_strings(val, sink);
                    }
                }
                _ => {}
            }
        }

        let file = File::open(&raw_path)?;
        let reader = BufReader::new(file);
        let mut collected = Vec::new();

        for line in reader.lines() {
            let line = line?;
            let stripped = line.trim();
            if stripped.is_empty() {
                continue;
            }
            match serde_json::from_str::<serde_json::Value>(stripped) {
                Ok(value) => extract_strings(&value, &mut collected),
                Err(_) => collected.push(line.trim_end_matches(['\r', '\n']).to_string()),
            }
        }

        let output_text = if collected.is_empty() {
            "\n".to_string()
        } else {
            collected.join("\n") + "\n"
        };

        fs::write(out_path, output_text)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run_subprocess(
        &self,
        argv: &[String],
        capsule: &Path,
        worktree: &Path,
        hard_timeout_s: u64,
        idle_timeout_s: Option<u64>,
        out_dir: &Path,
        _role: &str,
        _lane_id: Option<&str>,
        _model: Option<&str>,
    ) -> RunResult {
        if argv.is_empty() {
            let out_path = out_dir.join("empty.out");
            return RunResult::new(FailureKind::Crash, None, out_path, "empty argv");
        }

        if let Err(e) = fs::create_dir_all(out_dir) {
            let out_path = out_dir.join("error.out");
            return RunResult::new(FailureKind::Crash, None, out_path, format!("cannot create out_dir: {}", e));
        }

        let count = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let stem_capsule = capsule
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| "capsule".into());
        let stem = format!("{}-{}-{}", stem_capsule, self.name, count);
        let out_path = out_dir.join(format!("{}.out", stem));
        let err_path = out_dir.join(format!("{}.err", stem));

        let out_file = match File::create(&out_path) {
            Ok(f) => f,
            Err(e) => return RunResult::new(FailureKind::Crash, None, out_path, format!("cannot create out file: {}", e)),
        };
        let err_file = match File::create(&err_path) {
            Ok(f) => f,
            Err(e) => return RunResult::new(FailureKind::Crash, None, out_path, format!("cannot create err file: {}", e)),
        };

        let mut cmd = Command::new(&argv[0]);
        if argv.len() > 1 {
            cmd.args(&argv[1..]);
        }
        cmd.current_dir(worktree);
        cmd.stdin(Stdio::null());
        cmd.stdout(out_file);
        cmd.stderr(err_file);

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::write(&out_path, "");
                return RunResult::new(FailureKind::Crash, None, out_path, format!("harness launch failed: {}", e));
            }
        };

        let start_time = Instant::now();
        let deadline = start_time + Duration::from_secs(hard_timeout_s);
        let mut last_activity = Instant::now();
        let mut last_mtime: Option<SystemTime> = None;
        let mut timeout: Option<FailureKind> = None;
        let mut exit_status = None;

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    exit_status = Some(status);
                    break;
                }
                Ok(None) => {}
                Err(_e) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    timeout = Some(FailureKind::Crash);
                    break;
                }
            }

            let now = Instant::now();
            if now >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                timeout = Some(FailureKind::TimeoutHard);
                break;
            }

            if let Some(idle_s) = idle_timeout_s {
                let mtime = fs::metadata(&out_path).ok().and_then(|m| m.modified().ok());
                if mtime != last_mtime {
                    last_mtime = mtime;
                    last_activity = now;
                } else if now.duration_since(last_activity).as_secs() >= idle_s {
                    let _ = child.kill();
                    let _ = child.wait();
                    timeout = Some(FailureKind::TimeoutIdle);
                    break;
                }
            }

            std::thread::sleep(Duration::from_millis(200));
        }

        if self.jsonl_output {
            let _ = Self::finalize_stream(&out_path);
        }

        let tail = Self::tail(&err_path, STDERR_TAIL);

        if let Some(to) = timeout {
            return RunResult::new(to, None, out_path, tail);
        }

        if let Some(status) = exit_status {
            if status.success() {
                return RunResult::new(FailureKind::None, status.code().or(Some(0)), out_path, tail);
            }
            let classified = self.classify(&tail);
            let kind = classified.unwrap_or(FailureKind::Crash);
            let detail = if !tail.is_empty() {
                tail
            } else {
                format!("exit code {:?}", status.code())
            };
            return RunResult::new(kind, status.code(), out_path, detail);
        }

        RunResult::new(FailureKind::Crash, None, out_path, tail)
    }

    pub fn describe(&self, argv: &[String], worktree: &Path) -> String {
        let joined = argv
            .iter()
            .map(|arg| {
                if arg.contains(' ') || arg.contains('"') || arg.is_empty() {
                    format!("\"{}\"", arg.replace('\\', "\\\\").replace('"', "\\\""))
                } else {
                    arg.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        format!("(cd {} && {})", worktree.display(), joined)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_failure_kind_display_and_policy_key() {
        assert_eq!(FailureKind::QuotaExhausted.as_str(), "quota");
        assert_eq!(FailureKind::QuotaExhausted.policy_key(), "on_quota");
        assert_eq!(FailureKind::RateLimited.as_str(), "rate_limit");
        assert_eq!(FailureKind::RateLimited.policy_key(), "on_rate_limit");
        assert_eq!(FailureKind::AuthExpired.as_str(), "auth");
        assert_eq!(FailureKind::AuthExpired.policy_key(), "on_auth");
        assert_eq!(FailureKind::ModelUnavailable.as_str(), "model_unavailable");
        assert_eq!(FailureKind::ModelUnavailable.policy_key(), "on_model_unavailable");
        assert_eq!(FailureKind::TimeoutIdle.as_str(), "timeout_idle");
        assert_eq!(FailureKind::TimeoutIdle.policy_key(), "on_timeout");
        assert_eq!(FailureKind::TimeoutHard.as_str(), "timeout_hard");
        assert_eq!(FailureKind::TimeoutHard.policy_key(), "on_timeout");
        assert_eq!(FailureKind::Crash.as_str(), "crash");
        assert_eq!(FailureKind::Crash.policy_key(), "on_crash");
        assert_eq!(FailureKind::PartialDelivery.as_str(), "partial");
        assert_eq!(FailureKind::PartialDelivery.policy_key(), "on_crash");
        assert_eq!(FailureKind::OutputInvalid.as_str(), "invalid_output");
        assert_eq!(FailureKind::OutputInvalid.policy_key(), "on_invalid_output");
        assert_eq!(FailureKind::None.as_str(), "none");
        assert_eq!(FailureKind::None.policy_key(), "none");
    }

    #[test]
    fn test_finalize_stream_jsonl() {
        let tmp = tempfile::tempdir().unwrap();
        let out_path = tmp.path().join("test.out");

        let lines = [
            serde_json::json!({"role": "assistant", "content": [{"type": "text", "text": "working…"}]}).to_string(),
            "plain non-json line".to_string(),
            serde_json::json!({"type": "result", "finalText": "Done.\n\n```gauntlet-report\n{\"files_changed\": []}\n```"}).to_string(),
        ];
        fs::write(&out_path, lines.join("\n") + "\n").unwrap();

        SubprocessAdapter::finalize_stream(&out_path).unwrap();

        assert!(tmp.path().join("test.raw").is_file());
        let finalized = fs::read_to_string(&out_path).unwrap();
        assert!(finalized.contains("working…"));
        assert!(finalized.contains("plain non-json line"));
        assert!(finalized.contains("```gauntlet-report"));
    }

    #[test]
    fn test_finalize_stream_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let out_path = tmp.path().join("empty.out");
        fs::write(&out_path, "").unwrap();

        SubprocessAdapter::finalize_stream(&out_path).unwrap();
        let content = fs::read_to_string(&out_path).unwrap();
        assert_eq!(content, "\n");
    }
}
