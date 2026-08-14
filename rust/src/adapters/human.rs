//! human harness: interactive checkpoint on the terminal.
//!
//! Prints the capsule path + what is expected, then waits on stdin for a
//! decision (`approve` / `reject` on their own line) or a pasted fenced
//! gauntlet-* block (kept reading until the closing fence). EOF without any
//! decision is a CRASH, so the adapter is non-interactive-safe: with piped or
//! closed stdin it returns immediately instead of blocking forever.

use std::fs;
use std::io::{self, BufRead, BufReader, IsTerminal, Read};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use regex::Regex;

use super::base::{AdapterConfig, FailureKind, HarnessAdapter, RunResult};

pub fn read_decision(output_path: &Path) -> Option<String> {
    let text = fs::read_to_string(output_path).ok()?;
    for line in text.lines() {
        let stripped = line.trim();
        if stripped == "reject" {
            return Some("reject".to_string());
        }
        if stripped == "approve" {
            return Some("approve".to_string());
        }
    }
    let block_re = Regex::new(r"(?m)^```gauntlet-\w+\s*$").ok()?;
    if block_re.is_match(&text) {
        return Some("output".to_string());
    }
    None
}

pub struct HumanAdapter {
    pub name: String,
    pub counter: AtomicUsize,
}

impl HumanAdapter {
    pub fn new(name: &str, _cfg: Option<&AdapterConfig>) -> Self {
        Self {
            name: name.to_string(),
            counter: AtomicUsize::new(0),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn run_with_reader<R: Read>(
        &self,
        reader: R,
        capsule: &Path,
        _worktree: &Path,
        _write: bool,
        _model: Option<&str>,
        _effort: Option<&str>,
        _hard_timeout_s: u64,
        _idle_timeout_s: Option<u64>,
        out_dir: &Path,
    ) -> RunResult {
        let block_start_re = Regex::new(r"^```gauntlet-\w+\s*$").unwrap();
        let mut collected = Vec::new();
        let mut mode: Option<&'static str> = None;

        let buf = BufReader::new(reader);
        for line in buf.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            collected.push(line.clone());
            let stripped = line.trim();
            if mode == Some("block") {
                if stripped == "```" {
                    break;
                }
            } else if block_start_re.is_match(stripped) {
                mode = Some("block");
            } else if stripped == "approve" || stripped == "reject" {
                mode = Some("decision");
                break;
            }
        }

        let _ = fs::create_dir_all(out_dir);
        let count = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
        let stem = capsule
            .file_stem()
            .map(|s| s.to_string_lossy())
            .unwrap_or_else(|| "capsule".into());
        let out_path = out_dir.join(format!("{}-human-{}.out", stem, count));

        let content = if collected.is_empty() {
            String::new()
        } else {
            collected.join("\n") + "\n"
        };
        let _ = fs::write(&out_path, content);

        match mode {
            None => RunResult::new(
                FailureKind::Crash,
                None,
                out_path,
                "no decision on stdin (EOF)",
            ),
            Some(m) => RunResult::new(
                FailureKind::None,
                Some(0),
                out_path,
                format!("human input captured ({})", m),
            ),
        }
    }
}

impl Default for HumanAdapter {
    fn default() -> Self {
        Self::new("human", None)
    }
}

impl HarnessAdapter for HumanAdapter {
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
        model: Option<&str>,
        effort: Option<&str>,
        hard_timeout_s: u64,
        idle_timeout_s: Option<u64>,
        out_dir: &Path,
        _role: &str,
        _lane_id: Option<&str>,
    ) -> RunResult {
        if io::stdin().is_terminal() {
            println!("{}", "=".repeat(72));
            println!("HUMAN CHECKPOINT");
            println!("  capsule : {}", capsule.display());
            println!("  worktree: {}", worktree.display());
            println!("  Read the capsule, perform or review the task, then");
            println!("  either paste the required fenced gauntlet-* block, or");
            println!("  type 'approve' / 'reject' on its own line.");
            println!("  EOF (Ctrl-D) aborts this task.");
            println!("{}", "=".repeat(72));
        }

        self.run_with_reader(
            io::stdin(),
            capsule,
            worktree,
            write,
            model,
            effort,
            hard_timeout_s,
            idle_timeout_s,
            out_dir,
        )
    }

    fn describe(
        &self,
        capsule: &Path,
        _worktree: &Path,
        _write: bool,
        _model: Option<&str>,
        _effort: Option<&str>,
    ) -> String {
        format!("human checkpoint (capsule={})", capsule.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_human_approve() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = HumanAdapter::default();
        let capsule = tmp.path().join("c.md");
        fs::write(&capsule, "test").unwrap();

        let input = b"approve\n";
        let res = adapter.run_with_reader(
            &input[..],
            &capsule,
            tmp.path(),
            false,
            None,
            None,
            5,
            None,
            tmp.path(),
        );

        assert_eq!(res.failure, FailureKind::None);
        assert_eq!(read_decision(&res.output_path), Some("approve".to_string()));
    }

    #[test]
    fn test_human_reject() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = HumanAdapter::default();
        let capsule = tmp.path().join("c.md");
        fs::write(&capsule, "test").unwrap();

        let input = b"reject\n";
        let res = adapter.run_with_reader(
            &input[..],
            &capsule,
            tmp.path(),
            false,
            None,
            None,
            5,
            None,
            tmp.path(),
        );

        assert_eq!(res.failure, FailureKind::None);
        assert_eq!(read_decision(&res.output_path), Some("reject".to_string()));
    }

    #[test]
    fn test_human_pasted_block() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = HumanAdapter::default();
        let capsule = tmp.path().join("c.md");
        fs::write(&capsule, "test").unwrap();

        let input = b"```gauntlet-verdict\n{\"groups\": []}\n```\n";
        let res = adapter.run_with_reader(
            &input[..],
            &capsule,
            tmp.path(),
            false,
            None,
            None,
            5,
            None,
            tmp.path(),
        );

        assert_eq!(res.failure, FailureKind::None);
        assert_eq!(read_decision(&res.output_path), Some("output".to_string()));
    }

    #[test]
    fn test_human_eof_is_crash() {
        let tmp = tempfile::tempdir().unwrap();
        let adapter = HumanAdapter::default();
        let capsule = tmp.path().join("c.md");
        fs::write(&capsule, "test").unwrap();

        let input = b"";
        let res = adapter.run_with_reader(
            &input[..],
            &capsule,
            tmp.path(),
            false,
            None,
            None,
            5,
            None,
            tmp.path(),
        );

        assert_eq!(res.failure, FailureKind::Crash);
        assert!(res.detail.contains("EOF"));
    }
}
