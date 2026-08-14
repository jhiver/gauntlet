//! kimi harness: kimi CLI.
//!
//! See DESIGN.md "Concrete harness commands". `-y` (auto-approve) only for
//! write roles; read-only roles get `--auto`.

use std::path::Path;

use super::base::{AdapterConfig, HarnessAdapter, RunResult, SubprocessAdapter};

pub struct KimiAdapter {
    pub name: String,
    pub subprocess: SubprocessAdapter,
}

impl KimiAdapter {
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
        _effort: Option<&str>,
    ) -> Vec<String> {
        let mut argv = vec![
            "kimi".to_string(),
            "-p".to_string(),
            format!("Execute the mission file at {} and follow it exactly.", capsule.display()),
            "--add-dir".to_string(),
            worktree.display().to_string(),
            "--output-format".to_string(),
            "stream-json".to_string(),
        ];
        argv.push(if write { "-y".to_string() } else { "--auto".to_string() });
        let chosen = model.or(self.subprocess.default_model.as_deref());
        if let Some(m) = chosen {
            argv.push("-m".to_string());
            argv.push(m.to_string());
        }
        argv
    }
}

impl HarnessAdapter for KimiAdapter {
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
    fn test_kimi_build_argv() {
        let adapter = KimiAdapter::new(
            "kimi",
            Some(&AdapterConfig {
                default_model: Some("kimi-code/k3".to_string()),
                ..Default::default()
            }),
        );

        let argv_write = adapter.build_argv(Path::new("/c.md"), Path::new("/wt"), true, None, None);
        assert!(argv_write.contains(&"-y".to_string()));
        assert!(!argv_write.contains(&"--auto".to_string()));
        assert!(argv_write.contains(&"-m".to_string()));
        assert!(argv_write.contains(&"kimi-code/k3".to_string()));

        let argv_ro = adapter.build_argv(Path::new("/c.md"), Path::new("/wt"), false, None, None);
        assert!(argv_ro.contains(&"--auto".to_string()));
        assert!(!argv_ro.contains(&"-y".to_string()));
    }
}
