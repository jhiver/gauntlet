//! reasonix harness: reasonix CLI.
//!
//! Uses `reasonix -p --output-format stream-json`: the stream carries the
//! actual content (`kind:"text"` events + a final `type:"result"` with a
//! `result` field). Do NOT use `reasonix run --events-jsonl` for output
//! capture — that stream is REDACTED (kind markers only, no content).
//!
//! Read-only roles get --allowed-tools deny rules: the reviewer reads the diff
//! file and worktree files, it needs no shell, write, or git tool.

use std::path::Path;

use super::base::{AdapterConfig, HarnessAdapter, RunResult, SubprocessAdapter};

pub struct ReasonixAdapter {
    pub name: String,
    pub subprocess: SubprocessAdapter,
}

impl ReasonixAdapter {
    pub fn new(name: &str, cfg: Option<&AdapterConfig>) -> Self {
        let subprocess = SubprocessAdapter::new(name, cfg, true);
        Self {
            name: name.to_string(),
            subprocess,
        }
    }

    pub fn build_argv(
        &self,
        capsule: &Path,
        _worktree: &Path,
        write: bool,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Vec<String> {
        let mut argv = vec![
            "reasonix".to_string(),
            "-p".to_string(),
            format!("Execute the mission file at {} and follow it exactly.", capsule.display()),
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];
        let chosen = model.or(self.subprocess.default_model.as_deref());
        if let Some(m) = chosen {
            argv.push("--model".to_string());
            argv.push(m.to_string());
        }
        if let Some(e) = effort {
            argv.push("--effort".to_string());
            argv.push(e.to_string());
        }
        if !write {
            argv.push("--allowed-tools".to_string());
            argv.push("deny:write,deny:bash,deny:git".to_string());
        }
        argv
    }
}

impl HarnessAdapter for ReasonixAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn supports_write(&self) -> bool {
        self.subprocess.supports_write
    }

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
    ) -> RunResult {
        let argv = self.build_argv(capsule, worktree, write, model, effort);
        self.subprocess.run_subprocess(
            &argv,
            capsule,
            worktree,
            hard_timeout_s,
            idle_timeout_s,
            out_dir,
            role,
            lane_id,
            model,
        )
    }

    fn describe(
        &self,
        capsule: &Path,
        worktree: &Path,
        write: bool,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> String {
        let argv = self.build_argv(capsule, worktree, write, model, effort);
        self.subprocess.describe(&argv, worktree)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reasonix_build_argv() {
        let adapter = ReasonixAdapter::new("reasonix", None);

        let argv_ro = adapter.build_argv(
            Path::new("/c.md"),
            Path::new("/wt"),
            false,
            Some("deepseek-v4-pro"),
            None,
        );
        assert_eq!(argv_ro[0], "reasonix");
        assert_eq!(argv_ro[1], "-p");
        assert!(argv_ro.contains(&"--output-format".to_string()));
        assert!(argv_ro.contains(&"stream-json".to_string()));
        assert!(argv_ro.contains(&"--model".to_string()));
        assert!(argv_ro.contains(&"deepseek-v4-pro".to_string()));
        assert!(argv_ro.contains(&"--allowed-tools".to_string()));
        let idx = argv_ro.iter().position(|r| r == "--allowed-tools").unwrap();
        assert_eq!(argv_ro[idx + 1], "deny:write,deny:bash,deny:git");

        let argv_write = adapter.build_argv(
            Path::new("/c.md"),
            Path::new("/wt"),
            true,
            Some("deepseek-v4-pro"),
            None,
        );
        assert!(!argv_write.contains(&"--allowed-tools".to_string()));
    }
}
