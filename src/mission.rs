//! Mission file parsing: TOML frontmatter delimited by +++ plus markdown body.
//!
//! The body is the immutable root contract. AC/INV/NG entries carry stable IDs,
//! extracted with the regex from DESIGN.md to inject into capsules and to
//! validate verdict contract_ids.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MissionError {
    #[error("{0}")]
    Message(String),
}

impl From<String> for MissionError {
    fn from(s: String) -> Self {
        MissionError::Message(s)
    }
}

impl From<&str> for MissionError {
    fn from(s: &str) -> Self {
        MissionError::Message(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Repo {
    pub path: String,
    #[serde(default = "default_target_branch")]
    pub target_branch: String,
    #[serde(default)]
    pub gates: Vec<String>,
}

fn default_target_branch() -> String {
    "main".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Lane {
    pub id: String,
    pub owns: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub addresses: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mission {
    pub slug: String,
    pub repos: Vec<Repo>,
    pub lanes: Vec<Lane>,
    pub body: String,
    pub contract_ids: HashSet<String>,
    pub source_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct StageSpec {
    pub slug: String,
    #[serde(default)]
    pub brief: String,
    #[serde(default)]
    pub owns: Vec<String>,
    #[serde(default)]
    pub contract_ids: Vec<String>,
    #[serde(default)]
    pub gates: Vec<String>,
}

pub fn clean_mission_slug(stem_or_slug: &str) -> String {
    let mut s = stem_or_slug;
    for suffix in &[".doing", ".done", ".blocked", ".todo", ".failed"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped;
        }
    }
    s.to_string()
}

pub fn mission_status_path(current_path: &Path, new_status: &str) -> PathBuf {
    let file_name = match current_path.file_name().and_then(|f| f.to_str()) {
        Some(name) => name,
        None => return current_path.to_path_buf(),
    };

    let parent = current_path.parent().unwrap_or_else(|| Path::new("."));
    let stem = file_name.strip_suffix(".md").unwrap_or(file_name);
    let base_slug = clean_mission_slug(stem);

    if new_status.is_empty() || new_status == "todo" {
        parent.join(format!("{base_slug}.md"))
    } else {
        parent.join(format!("{base_slug}.{new_status}.md"))
    }
}

pub fn parse_mission(text: &str, source_path: &Path) -> Result<Mission, MissionError> {
    static FRONTMATTER_RE: once_cell::sync::Lazy<Option<Regex>> =
        once_cell::sync::Lazy::new(|| Regex::new(r"\A\+\+\+[ \t]*\n([\s\S]*?)\n\+\+\+[ \t]*\n?").ok());
    static CONTRACT_ID_RE: once_cell::sync::Lazy<Option<Regex>> =
        once_cell::sync::Lazy::new(|| Regex::new(r"(?m)^- ((?:AC|INV|NG)-\w+):").ok());

    let frontmatter_re = FRONTMATTER_RE.as_ref().ok_or_else(|| {
        MissionError::Message("failed to compile frontmatter regex".to_string())
    })?;
    let contract_id_re = CONTRACT_ID_RE.as_ref().ok_or_else(|| {
        MissionError::Message("failed to compile contract ID regex".to_string())
    })?;

    let caps = match frontmatter_re.captures(text) {
        Some(c) => c,
        None => {
            return Err(MissionError::Message(format!(
                "{}: missing +++ TOML frontmatter",
                source_path.display()
            )))
        }
    };

    let frontmatter_str = caps.get(1).map(|m| m.as_str()).unwrap_or("");
    let match_end = caps.get(0).map(|m| m.end()).unwrap_or(0);
    let body = text.get(match_end..).unwrap_or("").to_string();

    let front: toml::Table = toml::from_str(frontmatter_str).map_err(|e| {
        MissionError::Message(format!(
            "{}: invalid TOML frontmatter: {e}",
            source_path.display()
        ))
    })?;

    let slug_val = front.get("slug").and_then(|v| v.as_str()).unwrap_or("");
    let slug = if !slug_val.is_empty() {
        clean_mission_slug(slug_val)
    } else {
        let raw = source_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        clean_mission_slug(raw)
    };

    if slug.is_empty() {
        return Err(MissionError::Message(format!(
            "{}: slug must be a non-empty string",
            source_path.display()
        )));
    }

    let repos_val = match front.get("repos") {
        Some(toml::Value::Array(arr)) => arr,
        _ => {
            return Err(MissionError::Message(format!(
                "{}: at least one [[repos]] entry required",
                source_path.display()
            )))
        }
    };

    if repos_val.is_empty() {
        return Err(MissionError::Message(format!(
            "{}: at least one [[repos]] entry required",
            source_path.display()
        )));
    }

    if repos_val.len() > 1 {
        return Err(MissionError::Message(format!(
            "{}: this implementation supports exactly one [[repos]] entry per mission (see DESIGN.md state machine)",
            source_path.display()
        )));
    }

    let mut repos = Vec::new();
    for entry in repos_val {
        let repo_table = match entry.as_table() {
            Some(t) => t,
            None => {
                return Err(MissionError::Message(format!(
                    "{}: every [[repos]] needs a path",
                    source_path.display()
                )))
            }
        };

        let path = match repo_table.get("path").and_then(|v| v.as_str()) {
            Some(p) if !p.is_empty() => p.to_string(),
            _ => {
                return Err(MissionError::Message(format!(
                    "{}: every [[repos]] needs a path",
                    source_path.display()
                )))
            }
        };

        let target_branch = repo_table
            .get("target_branch")
            .and_then(|v| v.as_str())
            .unwrap_or("main")
            .to_string();

        let gates = repo_table
            .get("gates")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        repos.push(Repo {
            path,
            target_branch,
            gates,
        });
    }

    let mut lanes = Vec::new();
    let mut seen_ids = HashSet::new();

    if let Some(lanes_val) = front.get("lanes") {
        let lanes_arr = match lanes_val.as_array() {
            Some(arr) => arr,
            None => {
                return Err(MissionError::Message(format!(
                    "{}: [[lanes]] must be an array",
                    source_path.display()
                )))
            }
        };

        for entry in lanes_arr {
            let lane_table = match entry.as_table() {
                Some(t) => t,
                None => {
                    return Err(MissionError::Message(format!(
                        "{}: every [[lanes]] needs an id",
                        source_path.display()
                    )))
                }
            };

            let id = match lane_table.get("id").and_then(|v| v.as_str()) {
                Some(i) if !i.is_empty() => i.to_string(),
                _ => {
                    return Err(MissionError::Message(format!(
                        "{}: every [[lanes]] needs an id",
                        source_path.display()
                    )))
                }
            };

            if seen_ids.contains(&id) {
                return Err(MissionError::Message(format!(
                    "{}: duplicate lane id '{id}'",
                    source_path.display()
                )));
            }

            let owns: Vec<String> = lane_table
                .get("owns")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            if owns.is_empty() {
                return Err(MissionError::Message(format!(
                    "{}: lane '{id}' must own at least one glob",
                    source_path.display()
                )));
            }

            seen_ids.insert(id.clone());

            let forbidden = lane_table
                .get("forbidden")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let tests = lane_table
                .get("tests")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            let brief = lane_table
                .get("brief")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let addresses = lane_table
                .get("addresses")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();

            lanes.push(Lane {
                id,
                owns,
                forbidden,
                tests,
                brief,
                addresses,
            });
        }
    }

    let mut contract_ids = HashSet::new();
    for cap in contract_id_re.captures_iter(&body) {
        if let Some(m) = cap.get(1) {
            contract_ids.insert(m.as_str().to_string());
        }
    }

    Ok(Mission {
        slug,
        repos,
        lanes,
        body,
        contract_ids,
        source_path: source_path.to_path_buf(),
    })
}

pub fn resolve_mission_candidate(path: &Path) -> PathBuf {
    if path.is_file() {
        return path.to_path_buf();
    }
    for status in &["doing", "blocked", "done", "todo", ""] {
        let cand = mission_status_path(path, status);
        if cand.is_file() {
            return cand;
        }
    }
    path.to_path_buf()
}

pub fn load_mission(path: &Path) -> Result<Mission, MissionError> {
    let resolved = resolve_mission_candidate(path);
    let text = std::fs::read_to_string(&resolved).map_err(|e| {
        MissionError::Message(format!("cannot read mission file {}: {e}", resolved.display()))
    })?;
    parse_mission(&text, &resolved)
}

pub fn create_stage_mission(
    parent_mission: &Mission,
    stage: &StageSpec,
    target_branch: &str,
    path: &Path,
) -> Result<Mission, MissionError> {
    let repo_path = parent_mission
        .repos
        .first()
        .map(|r| r.path.as_str())
        .unwrap_or(".");

    let mut lines = vec![
        "+++".to_string(),
        format!("slug = \"{}-{}\"", parent_mission.slug, stage.slug),
        "".to_string(),
        "[[repos]]".to_string(),
        format!("path = \"{repo_path}\""),
        format!("target_branch = \"{target_branch}\""),
    ];

    let empty_gates = Vec::new();
    let gates = if !stage.gates.is_empty() {
        &stage.gates
    } else if let Some(r) = parent_mission.repos.first() {
        &r.gates
    } else {
        &empty_gates
    };

    if !gates.is_empty() {
        let gates_json = serde_json::to_string(gates).unwrap_or_else(|_| "[]".to_string());
        lines.push(format!("gates = {gates_json}"));
    }

    if !stage.owns.is_empty() {
        lines.push("".to_string());
        lines.push("[[lanes]]".to_string());
        lines.push("id = \"L1\"".to_string());
        let owns_json = serde_json::to_string(&stage.owns).unwrap_or_else(|_| "[]".to_string());
        let brief_json = serde_json::to_string(&stage.brief).unwrap_or_else(|_| "\"\"".to_string());
        lines.push(format!("owns = {owns_json}"));
        lines.push(format!("brief = {brief_json}"));
    }

    lines.push("+++".to_string());
    lines.push("".to_string());
    lines.push(format!("# Stage Contract: {}", stage.slug));
    lines.push("".to_string());
    lines.push(format!("> **Parent Mission Context**: `{}`", parent_mission.slug));
    lines.push("> ⚠️ **ANTI-DRIFT REQUIREMENT**: This stage is an atomic step of a parent composite mission.".to_string());
    lines.push("> It MUST strictly respect all parent Invariants (`INV-*`) and Non-Goals (`NG-*`).".to_string());
    lines.push("".to_string());
    lines.push("## Stage Objective".to_string());
    if stage.brief.is_empty() {
        lines.push("Implement stage deliverables".to_string());
    } else {
        lines.push(stage.brief.clone());
    }
    lines.push("".to_string());

    if !stage.contract_ids.is_empty() {
        lines.push("## Target Acceptance Criteria for this Stage".to_string());
        for cid in &stage.contract_ids {
            lines.push(format!("- {cid}"));
        }
        lines.push("".to_string());
    }

    lines.push("## Parent Global Contract (Inherited - Mandatory Invariants)".to_string());
    lines.push(parent_mission.body.trim().to_string());

    let content = format!("{}\n", lines.join("\n"));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            MissionError::Message(format!(
                "cannot create parent dir {}: {e}",
                parent.display()
            ))
        })?;
    }
    std::fs::write(path, &content).map_err(|e| {
        MissionError::Message(format!("cannot write mission file {}: {e}", path.display()))
    })?;

    parse_mission(&content, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_full_mission() {
        let text = r#"+++
slug = "example"
[[repos]]
path = "/tmp/repo"
target_branch = "main"
gates = ["true"]

[[lanes]]
id = "L1"
owns = ["src/example/**"]
+++

# Objective
Description

## AC
- AC-1: First AC
- AC-2: Second AC

## INV
- INV-1: Safety invariant

## NG
- NG-1: Non goal
"#;
        let m = parse_mission(text, Path::new("m.md")).unwrap();
        assert_eq!(m.slug, "example");
        assert_eq!(m.repos.len(), 1);
        assert_eq!(m.repos[0].target_branch, "main");
        assert_eq!(m.repos[0].gates, vec!["true"]);
        assert_eq!(m.lanes.len(), 1);
        assert_eq!(m.lanes[0].id, "L1");
        assert_eq!(m.lanes[0].owns, vec!["src/example/**"]);
        assert_eq!(
            m.contract_ids,
            ["AC-1", "AC-2", "INV-1", "NG-1"]
                .iter()
                .map(|s| s.to_string())
                .collect()
        );
        assert!(m.body.contains("# Objective"));
    }

    #[test]
    fn test_slug_derived_from_filename() {
        let text = r#"+++
slug = ""
[[repos]]
path = "/tmp/repo"
+++

# Objective
"#;
        let m = parse_mission(text, Path::new("auth-refactor.md")).unwrap();
        assert_eq!(m.slug, "auth-refactor");
    }

    #[test]
    fn test_missing_frontmatter_rejected() {
        let text = "# just markdown\n";
        assert!(parse_mission(text, Path::new("x.md")).is_err());
    }

    #[test]
    fn test_no_repos_rejected() {
        let text = "+++\nslug = \"x\"\n+++\n\n# Body\n";
        assert!(parse_mission(text, Path::new("x.md")).is_err());
    }

    #[test]
    fn test_multiple_repos_rejected() {
        let text = "+++\n[[repos]]\npath = \"/a\"\n[[repos]]\npath = \"/b\"\n+++\n\n# Body\n";
        assert!(parse_mission(text, Path::new("x.md")).is_err());
    }

    #[test]
    fn test_duplicate_lane_ids_rejected() {
        let text = r#"+++
[[repos]]
path = "/tmp/repo"

[[lanes]]
id = "L1"
owns = ["a/**"]

[[lanes]]
id = "L1"
owns = ["b/**"]
+++

# Body
"#;
        assert!(parse_mission(text, Path::new("x.md")).is_err());
    }

    #[test]
    fn test_empty_owns_rejected() {
        let text = r#"+++
[[repos]]
path = "/tmp/repo"

[[lanes]]
id = "L1"
owns = []
+++

# Body
"#;
        assert!(parse_mission(text, Path::new("x.md")).is_err());
    }

    #[test]
    fn test_no_lanes_is_allowed() {
        let text = r#"+++
[[repos]]
path = "/tmp/repo"
+++

# Body
"#;
        let m = parse_mission(text, Path::new("x.md")).unwrap();
        assert_eq!(m.lanes.len(), 0);
    }

    #[test]
    fn test_create_stage_mission_retains_parent_invariants() {
        let dir = tempdir().unwrap();
        let parent = Mission {
            slug: "parent-epic".to_string(),
            repos: vec![Repo {
                path: dir.path().to_str().unwrap().to_string(),
                target_branch: "main".to_string(),
                gates: vec!["npm test".to_string()],
            }],
            lanes: vec![],
            body: "# Objective\nBig Epic\n## Invariants\n- INV-1: Never delete logs\n## Non-Goals\n- NG-1: No Rust".to_string(),
            contract_ids: ["INV-1", "NG-1"].iter().map(|s| s.to_string()).collect(),
            source_path: dir.path().join("parent.md"),
        };
        let stage = StageSpec {
            slug: "01-schema".to_string(),
            brief: "Implement DB schema".to_string(),
            owns: vec!["db/**".to_string()],
            contract_ids: vec![],
            gates: vec![],
        };
        let sub_path = dir.path().join("sub.md");
        let sub = create_stage_mission(&parent, &stage, "master", &sub_path).unwrap();
        assert_eq!(sub.slug, "parent-epic-01-schema");
        assert!(sub.body.contains("INV-1: Never delete logs"));
        assert!(sub.body.contains("NG-1: No Rust"));
        assert!(sub.body.contains("parent-epic"));
        assert_eq!(sub.lanes.len(), 1);
        assert_eq!(sub.lanes[0].id, "L1");
        assert_eq!(sub.lanes[0].owns, vec!["db/**"]);
    }

    #[test]
    fn test_clean_mission_slug_and_status_paths() {
        assert_eq!(super::clean_mission_slug("auth-v2"), "auth-v2");
        assert_eq!(super::clean_mission_slug("auth-v2.doing"), "auth-v2");
        assert_eq!(super::clean_mission_slug("auth-v2.done"), "auth-v2");
        assert_eq!(super::clean_mission_slug("auth-v2.blocked"), "auth-v2");

        let base = Path::new("_missions/01-cache.md");
        assert_eq!(super::mission_status_path(base, "doing"), PathBuf::from("_missions/01-cache.doing.md"));
        assert_eq!(super::mission_status_path(base, "done"), PathBuf::from("_missions/01-cache.done.md"));
        assert_eq!(super::mission_status_path(base, "blocked"), PathBuf::from("_missions/01-cache.blocked.md"));

        let doing = Path::new("_missions/01-cache.doing.md");
        assert_eq!(super::mission_status_path(doing, "done"), PathBuf::from("_missions/01-cache.done.md"));
        assert_eq!(super::mission_status_path(doing, "blocked"), PathBuf::from("_missions/01-cache.blocked.md"));
    }
}
