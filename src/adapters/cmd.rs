//! cmd harness: commandcode.ai CLI.
//!
//! See DESIGN.md "Concrete harness commands". Do NOT use `cmd -w/--worktree`:
//! the orchestrator owns worktrees.

use std::path::Path;

use super::base::{AdapterConfig, HarnessAdapter, RunResult, SubprocessAdapter};

pub struct CmdAdapter {
    pub name: String,
    pub subprocess: SubprocessAdapter,
}

impl CmdAdapter {
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
            "cmd".to_string(),
            "-p".to_string(),
            format!("Execute the mission file at {} and follow it exactly.", capsule.display()),
            "--no-session".to_string(),
            "--skip-onboarding".to_string(),
            "--no-auto-update".to_string(),
            "--output-format".to_string(),
            "json".to_string(),
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
        if let Some(parent) = capsule.parent() {
            argv.push("--add-dir".to_string());
            argv.push(parent.display().to_string());
        }
        if write {
            argv.push("--yolo".to_string());
        } else {
            argv.push("--permission-mode".to_string());
            argv.push("plan".to_string());
        }
        argv
    }
}

impl HarnessAdapter for CmdAdapter {
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
    fn test_cmd_argv_write_and_readonly() {
        let adapter = CmdAdapter::new("cmd", Some(&AdapterConfig {
            default_model: Some("gpt-5.6-luna".to_string()),
            ..Default::default()
        }));

        let argv_write = adapter.build_argv(Path::new("/c.md"), Path::new("/wt"), true, None, Some("max"));
        assert!(argv_write.contains(&"--yolo".to_string()));
        assert!(!argv_write.contains(&"--permission-mode".to_string()));
        assert!(argv_write.contains(&"--model".to_string()));
        assert!(argv_write.contains(&"gpt-5.6-luna".to_string()));
        assert!(argv_write.contains(&"--effort".to_string()));
        assert!(argv_write.contains(&"max".to_string()));

        let argv_ro = adapter.build_argv(Path::new("/c.md"), Path::new("/wt"), false, None, None);
        assert!(argv_ro.contains(&"--permission-mode".to_string()));
        assert!(argv_ro.contains(&"plan".to_string()));
        assert!(!argv_ro.contains(&"--yolo".to_string()));
    }
}
