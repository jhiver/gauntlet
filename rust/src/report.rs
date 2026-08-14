//! Compact report.md maintenance: one short section per phase.

use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use chrono::Local;

#[derive(Debug, Clone)]
pub struct Report {
    pub path: PathBuf,
}

impl Report {
    pub fn new(path: impl AsRef<Path>, title: Option<&str>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if !path.exists() {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let default_title = "Gauntlet run report";
            let t = title.unwrap_or(default_title);
            std::fs::write(&path, format!("# {t}\n"))?;
        }
        Ok(Self { path })
    }

    fn append(&self, text: &str) -> io::Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(text.as_bytes())?;
        Ok(())
    }

    pub fn section(&self, title: &str, body: &str) -> io::Result<()> {
        let stamp = Local::now().format("%H:%M:%S").to_string();
        let trimmed_body = body.trim_end();
        self.append(&format!("\n## {title} ({stamp})\n\n{trimmed_body}\n"))
    }

    pub fn line(&self, text: &str) -> io::Result<()> {
        self.append(&format!("- {text}\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_report_creation_and_append() {
        let dir = tempdir().unwrap();
        let report_path = dir.path().join("report.md");
        let report = Report::new(&report_path, None).unwrap();
        report.section("INIT", "Initialized state").unwrap();
        report.line("Loaded mission").unwrap();

        let content = std::fs::read_to_string(&report_path).unwrap();
        assert!(content.starts_with("# Gauntlet run report\n"));
        assert!(content.contains("## INIT ("));
        assert!(content.contains("Initialized state"));
        assert!(content.contains("- Loaded mission\n"));
    }
}
