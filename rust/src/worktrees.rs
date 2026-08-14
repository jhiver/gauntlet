//! Git worktree lifecycle (orchestrator-owned) + glob machinery for mechanical checks.
//!
//! Workers never run git; the orchestrator alone runs git and gate commands.
//! With `dry_run = true`, mutating git commands are printed/logged instead of executed;
//! read-only commands (status, diff, rev-parse, ls-files) still run.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

pub const ALWAYS_IGNORED_PATHS: &[&str] = &[
    "node_modules",
    ".puppeteer-cache",
    ".pw-browsers",
    ".chrome-home",
    ".gauntlet",
    ".DS_Store",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError(pub String);

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for GitError {}

impl From<&str> for GitError {
    fn from(s: &str) -> Self {
        GitError(s.to_string())
    }
}

impl From<String> for GitError {
    fn from(s: String) -> Self {
        GitError(s)
    }
}

pub type LogCallback = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Clone)]
pub struct Git {
    pub dry_run: bool,
    log_fn: Option<LogCallback>,
}

impl Git {
    pub fn new(dry_run: bool) -> Self {
        Self {
            dry_run,
            log_fn: None,
        }
    }

    pub fn with_logger<F>(dry_run: bool, log: F) -> Self
    where
        F: Fn(&str) + Send + Sync + 'static,
    {
        Self {
            dry_run,
            log_fn: Some(Arc::new(log)),
        }
    }

    pub fn log(&self, message: &str) {
        if let Some(ref logger) = self.log_fn {
            logger(message);
        } else {
            println!("{message}");
        }
    }

    pub fn run(
        &self,
        args: &[&str],
        cwd: Option<&Path>,
        mutating: bool,
        check: bool,
    ) -> Result<Option<String>, GitError> {
        let cmd_str = format!("git {}", args.join(" "));
        if self.dry_run && mutating {
            let prefix = if let Some(dir) = cwd {
                format!("(cd {} && ", dir.display())
            } else {
                "(".to_string()
            };
            let formatted = format!("DRY-RUN: {prefix}{cmd_str})");
            self.log(&formatted);
            return Ok(None);
        }

        if let Some(dir) = cwd {
            if !dir.is_dir() {
                return Err(GitError(format!("git cwd does not exist: {}", dir.display())));
            }
        }

        let mut cmd = Command::new("git");
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let output = cmd.output().map_err(|e| {
            GitError(format!("{cmd_str} failed to launch: {e}"))
        })?;

        if !output.status.success() {
            if !check {
                return Ok(None);
            }
            let rc = output.status.code().unwrap_or(-1);
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(GitError(format!(
                "{cmd_str} failed (rc={rc}): {}",
                stderr.trim()
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        Ok(Some(stdout))
    }

    pub fn rc(&self, args: &[&str], cwd: Option<&Path>) -> i32 {
        if let Some(dir) = cwd {
            if !dir.is_dir() {
                return 128;
            }
        }

        let mut cmd = Command::new("git");
        cmd.args(args);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        match cmd.output() {
            Ok(out) => out.status.code().unwrap_or(128),
            Err(_) => 128,
        }
    }
}

// ---------------------------------------------------------------- git helpers

pub fn is_git_repo(git: &Git, repo: &Path) -> bool {
    git.rc(&["rev-parse", "--git-dir"], Some(repo)) == 0
}

pub fn staged_changes(git: &Git, repo: &Path) -> bool {
    git.rc(&["diff", "--cached", "--quiet"], Some(repo)) != 0
}

pub fn branch_exists(git: &Git, repo: &Path, branch: &str) -> bool {
    git.rc(&["rev-parse", "--verify", branch], Some(repo)) == 0
}

pub fn base_commit(git: &Git, repo: &Path, branch: &str) -> Result<String, GitError> {
    let out = git.run(&["rev-parse", branch], Some(repo), false, true)?;
    Ok(out.unwrap_or_default().trim().to_string())
}

pub fn current_branch(git: &Git, repo: &Path) -> Result<String, GitError> {
    let out = git.run(&["rev-parse", "--abbrev-ref", "HEAD"], Some(repo), false, true)?;
    Ok(out.unwrap_or_default().trim().to_string())
}

pub fn rev_parse(git: &Git, repo: &Path, ref_name: &str) -> Option<String> {
    let out = git.run(&["rev-parse", "--verify", ref_name], Some(repo), false, false).ok().flatten()?;
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn tracked_files(git: &Git, repo: &Path) -> Result<Vec<String>, GitError> {
    let out = git.run(&["ls-files"], Some(repo), false, true)?;
    let files = out
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.trim().to_string())
        .collect();
    Ok(files)
}

pub fn create_worktree(
    git: &Git,
    repo: &Path,
    wt: &Path,
    branch: &str,
    base: &str,
) -> Result<(), GitError> {
    git.run(
        &["worktree", "add", "-b", branch, &wt.to_string_lossy(), base],
        Some(repo),
        true,
        true,
    )?;

    // Symlink node_modules if present in main repo
    let src = repo.join("node_modules");
    let dst = wt.join("node_modules");
    if src.exists() && !dst.exists() {
        #[cfg(unix)]
        {
            let _ = std::os::unix::fs::symlink(&src, &dst);
        }
    }
    Ok(())
}

pub fn remove_worktree(git: &Git, repo: &Path, wt: &Path) -> Result<(), GitError> {
    git.run(
        &["worktree", "remove", "--force", &wt.to_string_lossy()],
        Some(repo),
        true,
        true,
    )?;
    Ok(())
}

pub fn delete_branch(git: &Git, repo: &Path, branch: &str) -> Result<(), GitError> {
    git.run(&["branch", "-D", branch], Some(repo), true, true)?;
    Ok(())
}

pub fn find_worktree_for_branch(
    git: &Git,
    repo: &Path,
    branch: &str,
) -> Result<Option<PathBuf>, GitError> {
    let output = git.run(&["worktree", "list", "--porcelain"], Some(repo), false, true)?;
    let output = output.unwrap_or_default();
    let mut current_wt: Option<String> = None;

    for line in output.lines() {
        if let Some(wt) = line.strip_prefix("worktree ") {
            current_wt = Some(wt.trim().to_string());
        } else if let Some(b) = line.strip_prefix("branch ") {
            if let Some(ref wt) = current_wt {
                let b = b.trim();
                if b == format!("refs/heads/{branch}") || b == branch {
                    return Ok(Some(PathBuf::from(wt)));
                }
            }
        }
    }
    Ok(None)
}

pub fn lane_changed_files(git: &Git, wt: &Path, base: &str) -> Result<Vec<String>, GitError> {
    if !wt.is_dir() {
        return Ok(Vec::new());
    }

    let mut changed = BTreeSet::new();

    let status = git.run(&["status", "--porcelain", "-uall"], Some(wt), false, true)?;
    for line in status.unwrap_or_default().lines() {
        if line.len() < 4 {
            continue;
        }
        let mut path = &line[3..];
        if let Some((_, new_path)) = path.split_once(" -> ") {
            path = new_path;
        }
        let cleaned = path.trim_matches('"').trim_end_matches('/');
        let first_seg = cleaned.split('/').next().unwrap_or("");
        if ALWAYS_IGNORED_PATHS.contains(&cleaned) || ALWAYS_IGNORED_PATHS.contains(&first_seg) {
            continue;
        }
        if !cleaned.is_empty() {
            changed.insert(cleaned.to_string());
        }
    }

    let diff = git.run(&["diff", "--name-only", base], Some(wt), false, true)?;
    for line in diff.unwrap_or_default().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let cleaned = line.trim_matches('"').trim_end_matches('/');
        let first_seg = cleaned.split('/').next().unwrap_or("");
        if ALWAYS_IGNORED_PATHS.contains(&cleaned) || ALWAYS_IGNORED_PATHS.contains(&first_seg) {
            continue;
        }
        if !cleaned.is_empty() {
            changed.insert(cleaned.to_string());
        }
    }

    Ok(changed.into_iter().collect())
}

pub fn commit_all(git: &Git, wt: &Path, message: &str) -> Result<bool, GitError> {
    let status = git.run(&["status", "--porcelain"], Some(wt), false, true)?;
    let status = status.unwrap_or_default();
    if status.trim().is_empty() {
        return Ok(false);
    }

    git.run(&["add", "-A"], Some(wt), true, true)?;

    for ignored in ALWAYS_IGNORED_PATHS {
        let _ = git.run(&["reset", "HEAD", "--", ignored], Some(wt), true, false);
    }

    let staged = git.run(&["diff", "--cached", "--name-only"], Some(wt), false, true)?;
    let staged = staged.unwrap_or_default();
    if staged.trim().is_empty() {
        return Ok(false);
    }

    git.run(
        &[
            "-c",
            "user.name=Gauntlet",
            "-c",
            "user.email=gauntlet@localhost",
            "commit",
            "-m",
            message,
        ],
        Some(wt),
        true,
        true,
    )?;

    Ok(true)
}

pub fn discard_changes(git: &Git, wt: &Path) -> Result<(), GitError> {
    git.run(&["reset", "--hard"], Some(wt), true, true)?;
    git.run(&["clean", "-fd"], Some(wt), true, true)?;
    Ok(())
}

pub fn merge_branch(git: &Git, wt: &Path, branch: &str) -> Result<(), GitError> {
    git.run(&["merge", "--no-edit", branch], Some(wt), true, true)?;
    Ok(())
}

pub fn rebase_onto(git: &Git, wt: &Path, onto: &str) -> Result<(), GitError> {
    git.run(&["rebase", onto], Some(wt), true, true)?;
    Ok(())
}

pub fn ff_merge(git: &Git, repo: &Path, branch: &str) -> Result<(), GitError> {
    git.run(&["merge", "--ff-only", branch], Some(repo), true, true)?;
    Ok(())
}

pub fn checkout_status(git: &Git, repo: &Path) -> Result<Vec<String>, GitError> {
    let out = git.run(&["status", "--porcelain", "-uall"], Some(repo), false, true)?;
    let mut paths = BTreeSet::new();

    for line in out.unwrap_or_default().lines() {
        if line.len() < 4 {
            continue;
        }
        let mut path = &line[3..];
        if let Some((_, new_path)) = path.split_once(" -> ") {
            path = new_path;
        }
        let cleaned = path.trim_matches('"').trim_end_matches('/');
        let first_seg = cleaned.split('/').next().unwrap_or("");
        if ALWAYS_IGNORED_PATHS.contains(&cleaned) || ALWAYS_IGNORED_PATHS.contains(&first_seg) {
            continue;
        }
        if !cleaned.is_empty() {
            paths.insert(cleaned.to_string());
        }
    }

    Ok(paths.into_iter().collect())
}

pub fn checkout_drift(
    before: &[String],
    after: &[String],
    ignore_prefixes: Option<&[&str]>,
) -> Vec<String> {
    let default_prefixes = [".missions/"];
    let prefixes = ignore_prefixes.unwrap_or(&default_prefixes);

    let is_ignored = |p: &str| prefixes.iter().any(|pre| p.starts_with(pre));

    let before_set: HashSet<&str> = before
        .iter()
        .map(|s| s.as_str())
        .filter(|p| !is_ignored(p))
        .collect();

    let mut drift = BTreeSet::new();
    for path in after {
        let p = path.as_str();
        if !is_ignored(p) && !before_set.contains(p) {
            drift.insert(path.clone());
        }
    }

    drift.into_iter().collect()
}

pub fn check_claimed_vs_diff(claimed: &[String], changed: &[String]) -> Vec<String> {
    let changed_set: HashSet<&str> = changed.iter().map(|s| s.as_str()).collect();
    claimed
        .iter()
        .filter(|p| !changed_set.contains(p.as_str()))
        .map(|p| format!("{p} (claimed by worker but absent from lane diff)"))
        .collect()
}

pub fn check_lane_diff(
    changed: &[String],
    owns: &[String],
    forbidden: &[String],
) -> Vec<String> {
    let mut violations = Vec::new();
    for path in changed {
        if forbidden.iter().any(|p| glob_matches(p, path)) {
            violations.push(format!("{path} (forbidden path)"));
        } else if !owns.iter().any(|p| glob_matches(p, path)) {
            violations.push(format!("{path} (outside lane owns)"));
        }
    }
    violations
}

// ------------------------------------------------------------- glob machinery

pub fn glob_to_regex_string(pattern: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let c = chars[i];
        if c == '*' {
            if i + 1 < n && chars[i + 1] == '*' {
                i += 2;
                if i < n && chars[i] == '/' {
                    i += 1;
                    out.push_str("(?:.*/)?");
                } else {
                    out.push_str(".*");
                }
            } else {
                out.push_str("[^/]*");
                i += 1;
            }
        } else if c == '?' {
            out.push_str("[^/]");
            i += 1;
        } else if c == '[' {
            if let Some(j) = pattern[i + 1..].find(']') {
                let j_idx = i + 1 + j;
                let content = &pattern[i + 1..j_idx];
                if let Some(negated) = content.strip_prefix('!') {
                    out.push_str(&format!("[^{negated}]"));
                } else {
                    out.push_str(&format!("[{content}]"));
                }
                i = j_idx + 1;
            } else {
                out.push_str(&regex_escape_char(c));
                i += 1;
            }
        } else if c == '\\' && i + 1 < n {
            out.push_str(&regex_escape_char(chars[i + 1]));
            i += 2;
        } else {
            out.push_str(&regex_escape_char(c));
            i += 1;
        }
    }

    format!("^{out}$")
}

fn regex_escape_char(c: char) -> String {
    match c {
        '.' | '+' | '*' | '?' | '^' | '$' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
            format!("\\{c}")
        }
        _ => c.to_string(),
    }
}

pub fn glob_matches(pattern: &str, path: &str) -> bool {
    let re_str = glob_to_regex_string(pattern);
    match regex::Regex::new(&re_str) {
        Ok(re) => re.is_match(path),
        Err(_) => false,
    }
}

pub fn static_prefix(pattern: &str) -> String {
    let mut parts = Vec::new();
    for part in pattern.split('/') {
        if part.contains('*') || part.contains('?') || part.contains('[') {
            break;
        }
        parts.push(part);
    }
    parts.join("/")
}

pub fn sample(pattern: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = pattern.chars().collect();
    let n = chars.len();
    let mut i = 0;

    while i < n {
        let c = chars[i];
        if c == '*' {
            out.push('x');
            if i + 1 < n && chars[i + 1] == '*' {
                i += 2;
            } else {
                i += 1;
            }
        } else if c == '?' {
            out.push('x');
            i += 1;
        } else if c == '[' {
            if let Some(j) = pattern[i + 1..].find(']') {
                let j_idx = i + 1 + j;
                let content = &pattern[i + 1..j_idx];
                if let Some(stripped) = content.strip_prefix('!') {
                    out.push(stripped.chars().next().unwrap_or('x'));
                } else {
                    out.push(content.chars().next().unwrap_or('x'));
                }
                i = j_idx + 1;
            } else {
                out.push(c);
                i += 1;
            }
        } else if c == '\\' && i + 1 < n {
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(c);
            i += 1;
        }
    }

    out
}

pub fn globs_may_overlap(a: &str, b: &str, repo_files: &[String]) -> bool {
    for path in repo_files {
        if glob_matches(a, path) && glob_matches(b, path) {
            return true;
        }
    }
    glob_matches(a, &sample(b)) || glob_matches(b, &sample(a))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneOverlap {
    pub id: String,
    pub owns: Vec<String>,
}

impl LaneOverlap {
    pub fn new(id: impl Into<String>, owns: Vec<String>) -> Self {
        Self {
            id: id.into(),
            owns,
        }
    }
}

pub fn find_overlaps(
    lanes: &[LaneOverlap],
    repo_files: &[String],
) -> Vec<(String, String, String, String)> {
    let mut overlaps = Vec::new();
    for i in 0..lanes.len() {
        for j in (i + 1)..lanes.len() {
            for ga in &lanes[i].owns {
                for gb in &lanes[j].owns {
                    if globs_may_overlap(ga, gb, repo_files) {
                        overlaps.push((
                            lanes[i].id.clone(),
                            lanes[j].id.clone(),
                            ga.clone(),
                            gb.clone(),
                        ));
                    }
                }
            }
        }
    }
    overlaps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_double_star_crosses_directories() {
        assert!(glob_matches("src/auth/**", "src/auth/session.rs"));
        assert!(glob_matches("src/auth/**", "src/auth/deep/x.rs"));
        assert!(!glob_matches("src/auth/**", "src/api/x.rs"));
    }

    #[test]
    fn test_single_star_stays_in_segment() {
        assert!(glob_matches("src/*", "src/x.rs"));
        assert!(!glob_matches("src/*", "src/a/x.rs"));
    }

    #[test]
    fn test_leading_double_star() {
        assert!(glob_matches("**/x.rs", "a/b/x.rs"));
        assert!(glob_matches("**/x.rs", "x.rs"));
    }

    #[test]
    fn test_full_tree_glob() {
        assert!(glob_matches("**", "anything/at/all.txt"));
    }

    #[test]
    fn test_static_prefix() {
        assert_eq!(static_prefix("src/auth/**"), "src/auth");
        assert_eq!(static_prefix("**"), "");
        assert_eq!(static_prefix("src/x.rs"), "src/x.rs");
    }

    #[test]
    fn test_disjoint_dirs_do_not_overlap() {
        assert!(!globs_may_overlap("src/auth/**", "src/api/**", &[]));
    }

    #[test]
    fn test_nested_globs_overlap() {
        assert!(globs_may_overlap("src/**", "src/auth/**", &[]));
    }

    #[test]
    fn test_exact_file_vs_dir_glob_overlap() {
        assert!(globs_may_overlap("src/auth/**", "src/auth/session.rs", &[]));
    }

    #[test]
    fn test_identical_globs_overlap() {
        assert!(globs_may_overlap("a/**", "a/**", &[]));
    }

    #[test]
    fn test_tracked_file_matching_both_overlaps() {
        assert!(globs_may_overlap(
            "src/*/x.rs",
            "src/a/*.rs",
            &["src/a/x.rs".to_string()]
        ));
    }

    #[test]
    fn test_find_overlaps_across_lanes() {
        let lanes = vec![
            LaneOverlap::new("L1", vec!["src/auth/**".to_string()]),
            LaneOverlap::new("L2", vec!["src/api/**".to_string()]),
            LaneOverlap::new("L3", vec!["src/**".to_string()]),
        ];
        let overlaps = find_overlaps(&lanes, &[]);
        let pairs: HashSet<(String, String)> =
            overlaps.into_iter().map(|(a, b, _, _)| (a, b)).collect();
        assert!(pairs.contains(&("L1".to_string(), "L3".to_string())));
        assert!(pairs.contains(&("L2".to_string(), "L3".to_string())));
        assert!(!pairs.contains(&("L1".to_string(), "L2".to_string())));
    }

    #[test]
    fn test_owned_changes_pass() {
        let violations = check_lane_diff(
            &["src/example/a.md".to_string(), "src/example/deep/b.md".to_string()],
            &["src/example/**".to_string()],
            &[],
        );
        assert_eq!(violations, Vec::<String>::new());
    }

    #[test]
    fn test_forbidden_path_rejected() {
        let violations = check_lane_diff(
            &["src/api/routes.rs".to_string()],
            &["src/**".to_string()],
            &["src/api/**".to_string()],
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("forbidden path"));
    }

    #[test]
    fn test_outside_owns_rejected() {
        let violations = check_lane_diff(
            &["README.md".to_string()],
            &["src/example/**".to_string()],
            &[],
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("outside lane owns"));
    }

    #[test]
    fn test_drift_flags_new_paths_outside_missions() {
        let before = vec![
            ".missions/run/state.json".to_string(),
            "src/old.py".to_string(),
        ];
        let after = vec![
            ".missions/run/state.json".to_string(),
            ".missions/run/report.md".to_string(),
            "src/old.py".to_string(),
            "src/example/hello.py".to_string(),
        ];
        assert_eq!(
            checkout_drift(&before, &after, None),
            vec!["src/example/hello.py".to_string()]
        );
    }

    #[test]
    fn test_drift_empty_when_only_missions_noise() {
        assert_eq!(
            checkout_drift(
                &["a.py".to_string()],
                &[".missions/r/x".to_string(), "a.py".to_string()],
                None
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_claimed_file_missing_from_diff_is_flagged() {
        let violations = check_claimed_vs_diff(
            &["src/example/hello.py".to_string()],
            &[],
        );
        assert_eq!(violations.len(), 1);
        assert!(violations[0].contains("hello.py"));
    }

    #[test]
    fn test_claimed_subset_of_diff_passes() {
        assert_eq!(
            check_claimed_vs_diff(
                &["a.py".to_string()],
                &["a.py".to_string(), "b.py".to_string()],
            ),
            Vec::<String>::new()
        );
    }
}
