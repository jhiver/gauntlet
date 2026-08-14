//! echo harness: deterministic fake for tests and --dry-run.
//!
//! Copies the capsule into the output file and emits a canned valid
//! gauntlet-report / gauntlet-verdict (NO_CLAIMS) / gauntlet-plan block, so the
//! extraction path for any role finds a valid block (extractors take the LAST
//! matching block of the requested kind, so emitting all three is safe).
//!
//! When invoked with write=True on a lane capsule (which carries machine-readable
//! `lane-id:` / `lane-owns:` / `lane-tests:` / `wave:` lines), echo also creates
//! one small file inside the lane's first owned path. This makes the full loop
//! (INSPECT diff, commit, merge, deliver) exercisable end-to-end without a real
//! LLM. If the worktree does not exist (e.g. --dry-run), no file is written.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use regex::Regex;

use super::base::{AdapterConfig, FailureKind, HarnessAdapter, RunResult};

pub fn static_prefix(pattern: &str) -> String {
    let mut parts = Vec::new();
    for part in pattern.split('/') {
        if part.chars().any(|c| c == '*' || c == '?' || c == '[') {
            break;
        }
        parts.push(part);
    }
    parts.join("/")
}

fn extract_json_list(re: &Regex, text: &str) -> Vec<String> {
    if let Some(caps) = re.captures(text) {
        if let Some(m) = caps.get(1) {
            if let Ok(val) = serde_json::from_str::<Vec<String>>(m.as_str()) {
                return val;
            }
        }
    }
    Vec::new()
}

pub struct EchoAdapter {
    pub name: String,
    pub counter: AtomicUsize,
}

impl EchoAdapter {
    pub fn new(name: &str, _cfg: Option<&AdapterConfig>) -> Self {
        Self {
            name: name.to_string(),
            counter: AtomicUsize::new(0),
        }
    }
}

impl Default for EchoAdapter {
    fn default() -> Self {
        Self::new("echo", None)
    }
}

impl HarnessAdapter for EchoAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn supports_write(&self) -> bool {
        true
    }

    fn run(
        &self,
        capsule: &Path,
        worktree: &Path,
        write: bool,
        _model: Option<&str>,
        _effort: Option<&str>,
        _hard_timeout_s: u64,
        _idle_timeout_s: Option<u64>,
        out_dir: &Path,
        _role: &str,
        _lane_id: Option<&str>,
    ) -> RunResult {
        let text = match fs::read_to_string(capsule) {
            Ok(t) => t,
            Err(e) => {
                let out_path = out_dir.join("echo_error.out");
                return RunResult::new(
                    FailureKind::Crash,
                    None,
                    out_path,
                    format!("cannot read capsule {}: {}", capsule.display(), e),
                );
            }
        };

        let lane_id_re = Regex::new(r"(?m)^lane-id:\s*(\S+)\s*$").unwrap();
        let lane_owns_re = Regex::new(r"(?m)^lane-owns:\s*(\[.*\])\s*$").unwrap();
        let lane_tests_re = Regex::new(r"(?m)^lane-tests:\s*(\[.*\])\s*$").unwrap();
        let wave_re = Regex::new(r"(?m)^wave:\s*(\d+)\s*$").unwrap();

        let mut changed = Vec::new();
        if write && worktree.is_dir() {
            let owns = extract_json_list(&lane_owns_re, &text);
            if !owns.is_empty() {
                let lane_id = lane_id_re
                    .captures(&text)
                    .and_then(|c| c.get(1).map(|m| m.as_str()))
                    .unwrap_or("lane");
                let wave = wave_re
                    .captures(&text)
                    .and_then(|c| c.get(1).map(|m| m.as_str()))
                    .unwrap_or("0");
                let rel_dir = static_prefix(&owns[0]);
                let filename = format!("echo-{}-w{}.md", lane_id, wave);
                let rel = if rel_dir.is_empty() {
                    filename
                } else {
                    format!("{}/{}", rel_dir, filename)
                };
                let target = worktree.join(&rel);
                if let Some(parent) = target.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                let _ = fs::write(
                    &target,
                    format!(
                        "echo harness deterministic write: lane {}, wave {}\n",
                        lane_id, wave
                    ),
                );
                changed.push(rel);
            }
        }

        let tests = extract_json_list(&lane_tests_re, &text);
        let report = serde_json::json!({
            "files_changed": changed,
            "tests_run": tests,
            "tests_passed": true,
            "partial": false,
            "notes": "echo harness: no real work performed",
        });
        let verdict = serde_json::json!({
            "groups": []
        });
        let plan = serde_json::json!({
            "lanes": [{
                "id": "E1",
                "owns": ["**"],
                "forbidden": [],
                "tests": ["true"],
                "brief": "echo harness canned plan lane",
                "addresses": [],
            }]
        });

        let _ = fs::create_dir_all(out_dir);
        let count = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let stem = capsule
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| "capsule".into());
        let out_path = out_dir.join(format!("{}-echo-{}.out", stem, count));

        let full_output = format!(
            "{}\n\n```gauntlet-report\n{}\n```\n\n```gauntlet-verdict\n{}\n```\n\n```gauntlet-plan\n{}\n```\n",
            text,
            serde_json::to_string(&report).unwrap_or_default(),
            serde_json::to_string(&verdict).unwrap_or_default(),
            serde_json::to_string(&plan).unwrap_or_default(),
        );

        let _ = fs::write(&out_path, full_output);
        RunResult::new(FailureKind::None, Some(0), out_path, "echo harness ok")
    }

    fn describe(
        &self,
        capsule: &Path,
        worktree: &Path,
        _write: bool,
        _model: Option<&str>,
        _effort: Option<&str>,
    ) -> String {
        format!(
            "echo harness (capsule={}, worktree={})",
            capsule.display(),
            worktree.display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_prefix() {
        assert_eq!(static_prefix("src/example/**"), "src/example");
        assert_eq!(static_prefix("README.md"), "README.md");
        assert_eq!(static_prefix("*.rs"), "");
        assert_eq!(static_prefix("a/b/c/file.txt"), "a/b/c/file.txt");
    }

    #[test]
    fn test_echo_adapter_run() {
        let tmp = tempfile::tempdir().unwrap();
        let wt = tmp.path().join("wt");
        fs::create_dir_all(&wt).unwrap();
        let capsule = tmp.path().join("capsule.md");
        let capsule_text = r#"
lane-id: L1
lane-owns: ["src/example/**"]
lane-tests: ["true"]
wave: 0
"#;
        fs::write(&capsule, capsule_text).unwrap();

        let adapter = EchoAdapter::default();
        let out_dir = tmp.path().join("out");
        let res = adapter.run(
            &capsule,
            &wt,
            true,
            None,
            None,
            5,
            None,
            &out_dir,
            "implementer",
            Some("L1"),
        );

        assert_eq!(res.failure, FailureKind::None);
        assert_eq!(res.exit_code, Some(0));
        assert!(wt.join("src/example/echo-L1-w0.md").is_file());

        let out_content = fs::read_to_string(&res.output_path).unwrap();
        assert!(out_content.contains("```gauntlet-report"));
        assert!(out_content.contains("src/example/echo-L1-w0.md"));
        assert!(out_content.contains("```gauntlet-verdict"));
        assert!(out_content.contains("```gauntlet-plan"));
    }
}
