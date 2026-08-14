//! codex harness: OpenAI Codex CLI.
//!
//! See DESIGN.md "Concrete harness commands".

use std::path::Path;

use super::base::{AdapterConfig, HarnessAdapter, RunResult, SubprocessAdapter};

pub struct CodexAdapter {
    pub name: String,
    pub subprocess: SubprocessAdapter,
}

impl CodexAdapter {
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
        worktree: &Path,
        write: bool,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> Vec<String> {
        let mut argv = vec![
            "codex".to_string(),
            "exec".to_string(),
            format!("Execute the mission file at {} and follow it exactly.", capsule.display()),
            "-C".to_string(),
            worktree.display().to_string(),
            "--json".to_string(),
        ];
        let chosen = model.or(self.subprocess.default_model.as_deref());
        if let Some(m) = chosen {
            argv.push("-m".to_string());
            argv.push(m.to_string());
        }
        if let Some(e) = effort {
            argv.push("-c".to_string());
            argv.push(format!("model_reasoning_effort={}", e));
        }
        if write {
            argv.push("--dangerously-bypass-approvals-and-sandbox".to_string());
        } else {
            argv.push("-s".to_string());
            argv.push("read-only".to_string());
        }
        argv
    }
}

impl HarnessAdapter for CodexAdapter {
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
    fn test_codex_build_argv() {
        let adapter = CodexAdapter::new("codex", None);

        let argv_ro = adapter.build_argv(
            Path::new("/c.md"),
            Path::new("/wt"),
            false,
            Some("gpt-5.6-sol"),
            Some("xhigh"),
        );
        assert_eq!(argv_ro[0], "codex");
        assert_eq!(argv_ro[1], "exec");
        assert!(argv_ro.contains(&"-s".to_string()));
        assert!(argv_ro.contains(&"read-only".to_string()));
        assert!(argv_ro.contains(&"-m".to_string()));
        assert!(argv_ro.contains(&"gpt-5.6-sol".to_string()));
        assert!(argv_ro.contains(&"-c".to_string()));
        assert!(argv_ro.contains(&"model_reasoning_effort=xhigh".to_string()));

        let argv_write = adapter.build_argv(
            Path::new("/c.md"),
            Path::new("/wt"),
            true,
            Some("gpt-5.6-sol"),
            Some("xhigh"),
        );
        assert!(argv_write.contains(&"--dangerously-bypass-approvals-and-sandbox".to_string()));
        assert!(!argv_write.contains(&"read-only".to_string()));
    }
}
