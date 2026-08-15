//! agy harness: wraps the antigravity-delegation launcher script.
//!
//! See DESIGN.md "Concrete harness commands". Short prompt + capsule path,
//! never an inline capsule (long inline prompts fail silently on agy).
//!
//! The capsule is staged INSIDE the lane worktree before invocation: the
//! launcher passes `--add-dir <capsule-dir>` to agy, and a capsule living in
//! the main checkout made agy anchor on (and write into) the main checkout
//! instead of the lane worktree (smoke test 2026-08-14). The staged copy is
//! removed after the run, before INSPECT diffs the worktree.

use std::fs;
use std::path::{Path, PathBuf};

use super::base::{AdapterConfig, HarnessAdapter, RunResult, SubprocessAdapter};

pub const DEFAULT_LAUNCHER: &str =
    "~/aios/.reasonix/skills/antigravity-delegation/scripts/agy-delegate";

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(rest);
        }
    }
    PathBuf::from(path)
}

fn complexity(effort: Option<&str>) -> &'static str {
    match effort.unwrap_or("") {
        "low" => "low",
        "medium" => "medium",
        "high" | "max" => "high",
        _ => "medium",
    }
}

pub struct AgyAdapter {
    pub name: String,
    pub launcher: PathBuf,
    pub subprocess: SubprocessAdapter,
}

impl AgyAdapter {
    pub fn new(name: &str, cfg: Option<&AdapterConfig>) -> Self {
        let launcher_str = cfg
            .and_then(|c| c.launcher.as_deref())
            .unwrap_or(DEFAULT_LAUNCHER);
        let launcher = expand_tilde(launcher_str);
        let subprocess = SubprocessAdapter::new(name, cfg, false);
        Self {
            name: name.to_string(),
            launcher,
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
            "bash".to_string(),
            self.launcher.display().to_string(),
            "--kind".to_string(),
            if write { "implement" } else { "review" }.to_string(),
            "--complexity".to_string(),
            complexity(effort).to_string(),
            "--mission".to_string(),
            capsule.display().to_string(),
            "--cwd".to_string(),
            worktree.display().to_string(),
        ];
        if write {
            argv.push("--write".to_string());
        }
        let chosen = model.or(self.subprocess.default_model.as_deref());
        if let Some(m) = chosen {
            argv.push("--model".to_string());
            argv.push(m.to_string());
        }
        argv
    }
}

impl HarnessAdapter for AgyAdapter {
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
        let staged_dir = worktree.join(".gauntlet");
        let staged = staged_dir.join("capsule.md");

        // Staging guard to ensure cleanup on error or return
        struct StagingGuard<'a>(&'a Path);
        impl<'a> Drop for StagingGuard<'a> {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(self.0);
            }
        }

        let _ = fs::create_dir_all(&staged_dir);
        let _ = fs::copy(capsule, &staged);
        let _guard = StagingGuard(&staged_dir);

        let argv = self.build_argv(&staged, worktree, write, model, effort);
        self.subprocess.run_subprocess(
            &argv,
            &staged,
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
        _capsule: &Path,
        worktree: &Path,
        write: bool,
        model: Option<&str>,
        effort: Option<&str>,
    ) -> String {
        let staged = worktree.join(".gauntlet").join("capsule.md");
        let argv = self.build_argv(&staged, worktree, write, model, effort);
        let sub_desc = self.subprocess.describe(&argv, worktree);
        let parent_str = staged
            .parent()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| ".gauntlet".to_string());
        format!(
            "stage capsule at {}; {}; remove {parent_str}",
            staged.display(),
            sub_desc
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agy_build_argv() {
        let adapter = AgyAdapter::new(
            "agy",
            Some(&AdapterConfig {
                launcher: Some("/custom/agy-delegate".to_string()),
                default_model: Some("gemini-3.7-flash-high".to_string()),
                ..Default::default()
            }),
        );
        let argv = adapter.build_argv(
            Path::new("/capsule.md"),
            Path::new("/wt"),
            true,
            None,
            Some("high"),
        );
        assert_eq!(argv[0], "bash");
        assert_eq!(argv[1], "/custom/agy-delegate");
        assert!(argv.contains(&"--kind".to_string()));
        assert!(argv.contains(&"implement".to_string()));
        assert!(argv.contains(&"--complexity".to_string()));
        assert!(argv.contains(&"high".to_string()));
        assert!(argv.contains(&"--write".to_string()));
        assert!(argv.contains(&"--model".to_string()));
        assert!(argv.contains(&"gemini-3.7-flash-high".to_string()));
    }
}
