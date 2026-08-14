//! gauntlet-report / gauntlet-verdict / gauntlet-plan block parsing+validation.
//!
//! Extracts the LAST matching fenced block, JSON-parses, and schema-validates:
//! verdict enum, defect class enum, contract IDs must exist in the contract, lane
//! ownership globs non-empty. See DESIGN.md "Structured I/O protocol".

use std::collections::HashSet;
use std::path::Path;
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::mission::StageSpec;

pub const VERDICT_VALUES: &[&str] = &["DISMISS", "FIX", "REDESIGN", "REPORT_ONLY"];
pub const ACTIONABLE_VERDICTS: &[&str] = &["FIX", "REDESIGN"];
pub const CODE_DEFECT: &str = "code_defect";
pub const CLASS_VALUES: &[&str] = &["code_defect", "doc_drift", "evidence_gap"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum VerdictError {
    #[error("{0}")]
    Message(String),
}

impl From<String> for VerdictError {
    fn from(s: String) -> Self {
        VerdictError::Message(s)
    }
}

impl From<&str> for VerdictError {
    fn from(s: &str) -> Self {
        VerdictError::Message(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClaimGroup {
    pub root_cause: String,
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default)]
    pub contract_ids: Vec<String>,
    #[serde(default = "default_verdict")]
    pub verdict: String,
    #[serde(default)]
    pub fix: String,
    #[serde(default)]
    pub owns: String,
    #[serde(default = "default_class", rename = "class")]
    pub defect_class: String,
}

fn default_verdict() -> String {
    "REPORT_ONLY".to_string()
}
fn default_class() -> String {
    CODE_DEFECT.to_string()
}

impl ClaimGroup {
    pub fn actionable(&self) -> bool {
        self.verdict == "FIX" || self.verdict == "REDESIGN"
    }

    /// Actionable groups that must be fixed before delivery.
    ///
    /// REDESIGN always blocks whatever its class: only a code defect can
    /// make the smallest additive patch disproportionate.
    pub fn blocking(&self) -> bool {
        self.actionable() && (self.verdict == "REDESIGN" || self.defect_class == CODE_DEFECT)
    }

    /// Actionable but non-blocking: handled by the pre-delivery polish.
    pub fn polish(&self) -> bool {
        self.actionable() && !self.blocking()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReportData {
    pub files_changed: Vec<String>,
    pub tests_run: Vec<String>,
    pub tests_passed: bool,
    pub partial: bool,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanLane {
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
pub enum PlannerResult {
    Lanes(Vec<PlanLane>),
    Stages(Vec<StageSpec>),
}

/// Return the parsed JSON of the LAST ```gauntlet-<kind> block.
pub fn extract_block(text: &str, kind: &str) -> Result<serde_json::Value, VerdictError> {
    let block_re = Regex::new(r"```gauntlet-(report|verdict|plan|stages)[ \t]*\n([\s\S]*?)```").unwrap();

    let mut found = None;
    for cap in block_re.captures_iter(text) {
        if cap.get(1).map(|m| m.as_str()) == Some(kind) {
            found = cap.get(2).map(|m| m.as_str());
        }
    }

    let found_text = match found {
        Some(t) => t,
        None => return Err(VerdictError::Message(format!("no gauntlet-{kind} fenced block found"))),
    };

    let data: serde_json::Value = serde_json::from_str(found_text)
        .map_err(|e| VerdictError::Message(format!("gauntlet-{kind} block is not valid JSON: {e}")))?;

    if !data.is_object() {
        return Err(VerdictError::Message(format!(
            "gauntlet-{kind} block must be a JSON object"
        )));
    }

    Ok(data)
}

pub fn extract_block_from_file(path: &Path, kind: &str) -> Result<serde_json::Value, VerdictError> {
    let bytes = std::fs::read(path).map_err(|e| {
        VerdictError::Message(format!("cannot read file {}: {e}", path.display()))
    })?;
    let text = String::from_utf8_lossy(&bytes);
    extract_block(&text, kind)
}

fn extract_str_list(value: Option<&serde_json::Value>, what: &str) -> Result<Vec<String>, VerdictError> {
    match value {
        Some(serde_json::Value::Array(arr)) => {
            let mut list = Vec::new();
            for item in arr {
                match item.as_str() {
                    Some(s) => list.push(s.to_string()),
                    None => return Err(VerdictError::Message(format!("{what} must be a list of strings"))),
                }
            }
            Ok(list)
        }
        Some(_) => Err(VerdictError::Message(format!("{what} must be a list of strings"))),
        None => Err(VerdictError::Message(format!("{what} missing"))),
    }
}

fn extract_optional_str_list(
    value: Option<&serde_json::Value>,
    what: &str,
) -> Result<Vec<String>, VerdictError> {
    match value {
        Some(serde_json::Value::Array(arr)) => {
            let mut list = Vec::new();
            for item in arr {
                match item.as_str() {
                    Some(s) => list.push(s.to_string()),
                    None => return Err(VerdictError::Message(format!("{what} must be a list of strings"))),
                }
            }
            Ok(list)
        }
        Some(_) => Err(VerdictError::Message(format!("{what} must be a list of strings"))),
        None => Ok(Vec::new()),
    }
}

pub fn validate_report(data: &serde_json::Value) -> Result<ReportData, VerdictError> {
    let obj = match data.as_object() {
        Some(o) => o,
        None => return Err(VerdictError::Message("gauntlet-report must be an object".to_string())),
    };

    if !obj.contains_key("files_changed") {
        return Err(VerdictError::Message("gauntlet-report missing key 'files_changed'".to_string()));
    }
    if !obj.contains_key("tests_run") {
        return Err(VerdictError::Message("gauntlet-report missing key 'tests_run'".to_string()));
    }
    if !obj.contains_key("tests_passed") {
        return Err(VerdictError::Message("gauntlet-report missing key 'tests_passed'".to_string()));
    }
    if !obj.contains_key("partial") {
        return Err(VerdictError::Message("gauntlet-report missing key 'partial'".to_string()));
    }

    let files_changed = extract_str_list(obj.get("files_changed"), "files_changed")?;
    let tests_run = extract_str_list(obj.get("tests_run"), "tests_run")?;

    let tests_passed = match obj.get("tests_passed").and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => return Err(VerdictError::Message("tests_passed must be a boolean".to_string())),
    };

    let partial = match obj.get("partial").and_then(|v| v.as_bool()) {
        Some(b) => b,
        None => return Err(VerdictError::Message("partial must be a boolean".to_string())),
    };

    let notes = match obj.get("notes") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(_) => return Err(VerdictError::Message("notes must be a string".to_string())),
        None => String::new(),
    };

    Ok(ReportData {
        files_changed,
        tests_run,
        tests_passed,
        partial,
        notes,
    })
}

pub fn validate_verdict(
    data: &serde_json::Value,
    contract_ids: &HashSet<String>,
) -> Result<Vec<ClaimGroup>, VerdictError> {
    let groups_raw = match data.get("groups") {
        Some(serde_json::Value::Array(arr)) => arr,
        _ => return Err(VerdictError::Message("gauntlet-verdict 'groups' must be a list".to_string())),
    };

    let verdict_set: HashSet<&str> = VERDICT_VALUES.iter().copied().collect();
    let class_set: HashSet<&str> = CLASS_VALUES.iter().copied().collect();

    let mut groups = Vec::new();
    for (i, raw) in groups_raw.iter().enumerate() {
        let group_obj = match raw.as_object() {
            Some(o) => o,
            None => return Err(VerdictError::Message(format!("verdict group {i} must be an object"))),
        };

        let verdict = match group_obj.get("verdict").and_then(|v| v.as_str()) {
            Some(v) if verdict_set.contains(v) => v.to_string(),
            Some(v) => {
                return Err(VerdictError::Message(format!(
                    "verdict group {i}: verdict must be one of {:?}, got {v:?}",
                    VERDICT_VALUES
                )))
            }
            None => {
                return Err(VerdictError::Message(format!(
                    "verdict group {i}: verdict must be one of {:?}, got None",
                    VERDICT_VALUES
                )))
            }
        };

        let ids = extract_optional_str_list(group_obj.get("contract_ids"), &format!("verdict group {i} contract_ids"))?;
        let unknown: Vec<String> = ids
            .iter()
            .filter(|id| !contract_ids.contains(*id))
            .cloned()
            .collect();
        if !unknown.is_empty() {
            return Err(VerdictError::Message(format!(
                "verdict group {i}: unknown contract IDs {unknown:?}"
            )));
        }

        let root_cause = match group_obj.get("root_cause").and_then(|v| v.as_str()) {
            Some(rc) if !rc.is_empty() => rc.to_string(),
            _ => return Err(VerdictError::Message(format!("verdict group {i}: root_cause required"))),
        };

        let defect_class = match group_obj.get("class").and_then(|v| v.as_str()) {
            Some(c) if class_set.contains(c) => c.to_string(),
            Some(c) => {
                return Err(VerdictError::Message(format!(
                    "verdict group {i}: class must be one of {:?}, got {c:?}",
                    CLASS_VALUES
                )))
            }
            None => CODE_DEFECT.to_string(),
        };

        let claims = extract_optional_str_list(group_obj.get("claims"), &format!("group {i} claims"))?;
        let fix = group_obj
            .get("fix")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let owns = group_obj
            .get("owns")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        groups.push(ClaimGroup {
            root_cause,
            claims,
            contract_ids: ids,
            verdict,
            fix,
            owns,
            defect_class,
        });
    }

    Ok(groups)
}

pub fn validate_plan(data: &serde_json::Value) -> Result<Vec<PlanLane>, VerdictError> {
    let lanes_raw = match data.get("lanes") {
        Some(serde_json::Value::Array(arr)) if !arr.is_empty() => arr,
        _ => return Err(VerdictError::Message("gauntlet-plan 'lanes' must be a non-empty list".to_string())),
    };

    let mut lanes = Vec::new();
    let mut seen = HashSet::new();

    for (i, raw) in lanes_raw.iter().enumerate() {
        let lane_obj = match raw.as_object() {
            Some(o) => o,
            None => return Err(VerdictError::Message(format!("plan lane {i} must be an object"))),
        };

        let lane_id = match lane_obj.get("id").and_then(|v| v.as_str()) {
            Some(id) if !id.is_empty() => id.to_string(),
            _ => return Err(VerdictError::Message(format!("plan lane {i}: id required"))),
        };

        if seen.contains(&lane_id) {
            return Err(VerdictError::Message(format!(
                "plan lane {i}: duplicate lane id '{lane_id}'"
            )));
        }
        seen.insert(lane_id.clone());

        let owns = extract_optional_str_list(lane_obj.get("owns"), &format!("plan lane {i} owns"))?;
        if owns.is_empty() {
            return Err(VerdictError::Message(format!(
                "plan lane {i} ('{lane_id}') must own at least one glob"
            )));
        }

        let forbidden = extract_optional_str_list(lane_obj.get("forbidden"), &format!("plan lane {i} forbidden"))?;
        let tests = extract_optional_str_list(lane_obj.get("tests"), &format!("plan lane {i} tests"))?;
        let brief = lane_obj
            .get("brief")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let addresses = extract_optional_str_list(lane_obj.get("addresses"), &format!("plan lane {i} addresses"))?;

        lanes.push(PlanLane {
            id: lane_id,
            owns,
            forbidden,
            tests,
            brief,
            addresses,
        });
    }

    Ok(lanes)
}

pub fn validate_stages(
    data: &serde_json::Value,
    valid_contract_ids: Option<&HashSet<String>>,
) -> Result<Vec<StageSpec>, VerdictError> {
    let stages_raw = match data.get("stages") {
        Some(serde_json::Value::Array(arr)) if !arr.is_empty() => arr,
        _ => return Err(VerdictError::Message("gauntlet-stages 'stages' must be a non-empty list".to_string())),
    };

    let mut stages = Vec::new();
    let mut seen = HashSet::new();

    for (i, raw) in stages_raw.iter().enumerate() {
        let stage_obj = match raw.as_object() {
            Some(o) => o,
            None => return Err(VerdictError::Message(format!("stage {i} must be an object"))),
        };

        let slug = match stage_obj.get("slug").and_then(|v| v.as_str()) {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => return Err(VerdictError::Message(format!("stage {i}: slug required"))),
        };

        if seen.contains(&slug) {
            return Err(VerdictError::Message(format!("stage {i}: duplicate slug '{slug}'")));
        }
        seen.insert(slug.clone());

        let brief = stage_obj
            .get("brief")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let owns = extract_optional_str_list(stage_obj.get("owns"), &format!("stage {i} owns"))?;
        let contract_ids = extract_optional_str_list(stage_obj.get("contract_ids"), &format!("stage {i} contract_ids"))?;

        if let Some(valid_ids) = valid_contract_ids {
            let mut unknown: Vec<String> = contract_ids
                .iter()
                .filter(|id| !valid_ids.contains(*id))
                .cloned()
                .collect();
            if !unknown.is_empty() {
                unknown.sort();
                return Err(VerdictError::Message(format!(
                    "stage {i} mentions unknown contract IDs: {unknown:?}"
                )));
            }
        }

        let gates = extract_optional_str_list(stage_obj.get("gates"), &format!("stage {i} gates"))?;

        stages.push(StageSpec {
            slug,
            brief,
            owns,
            contract_ids,
            gates,
        });
    }

    Ok(stages)
}

/// Extract either gauntlet-plan (lanes) or gauntlet-stages (sequential stages).
/// Returns PlannerResult::Lanes(lanes) or PlannerResult::Stages(stages).
pub fn extract_planner_result(
    text: &str,
    valid_contract_ids: Option<&HashSet<String>>,
) -> Result<PlannerResult, VerdictError> {
    let block_re = Regex::new(r"```gauntlet-(report|verdict|plan|stages)[ \t]*\n([\s\S]*?)```").unwrap();

    let mut found_kind = None;
    let mut found_text = None;

    for cap in block_re.captures_iter(text) {
        if let Some(kind_match) = cap.get(1) {
            let kind = kind_match.as_str();
            if kind == "plan" || kind == "stages" {
                found_kind = Some(kind.to_string());
                found_text = cap.get(2).map(|m| m.as_str().to_string());
            }
        }
    }

    let (kind, text_content) = match (found_kind, found_text) {
        (Some(k), Some(t)) => (k, t),
        _ => {
            return Err(VerdictError::Message(
                "no gauntlet-plan or gauntlet-stages fenced block found".to_string(),
            ))
        }
    };

    let data: serde_json::Value = serde_json::from_str(&text_content).map_err(|e| {
        VerdictError::Message(format!("gauntlet-{kind} block is not valid JSON: {e}"))
    })?;

    if !data.is_object() {
        return Err(VerdictError::Message(format!(
            "gauntlet-{kind} block must be a JSON object"
        )));
    }

    if kind == "stages" {
        let stages = validate_stages(&data, valid_contract_ids)?;
        Ok(PlannerResult::Stages(stages))
    } else {
        let lanes = validate_plan(&data)?;
        Ok(PlannerResult::Lanes(lanes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_contract_ids() -> HashSet<String> {
        ["AC-1", "AC-2", "INV-1", "NG-1"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn test_extracts_last_matching_block() {
        let text = r#"prose
```gauntlet-verdict
{"groups": [{"root_cause": "old", "verdict": "DISMISS"}]}
```
more prose
```gauntlet-verdict
{"groups": []}
```
trailing
"#;
        let data = extract_block(text, "verdict").unwrap();
        assert_eq!(data, serde_json::json!({"groups": []}));
    }

    #[test]
    fn test_ignores_other_block_kinds() {
        let text = r#"```gauntlet-report
{"files_changed": [], "tests_run": [], "tests_passed": true, "partial": false, "notes": ""}
```
"#;
        assert!(extract_block(text, "verdict").is_err());
    }

    #[test]
    fn test_missing_block_rejected() {
        assert!(extract_block("no blocks here", "report").is_err());
    }

    #[test]
    fn test_invalid_json_rejected() {
        assert!(extract_block("```gauntlet-verdict\n{not json}\n```", "verdict").is_err());
    }

    #[test]
    fn test_non_object_rejected() {
        assert!(extract_block("```gauntlet-verdict\n[1, 2]\n```", "verdict").is_err());
    }

    #[test]
    fn test_valid_report() {
        let json_val = serde_json::json!({
            "files_changed": ["src/a.rs"],
            "tests_run": ["cargo test"],
            "tests_passed": true,
            "partial": false,
            "notes": "ok"
        });
        let report = validate_report(&json_val).unwrap();
        assert_eq!(report.files_changed, vec!["src/a.rs"]);
        assert!(!report.partial);
    }

    #[test]
    fn test_report_missing_key_rejected() {
        let json_val = serde_json::json!({
            "files_changed": []
        });
        assert!(validate_report(&json_val).is_err());
    }

    #[test]
    fn test_report_wrong_type_rejected() {
        let json_val = serde_json::json!({
            "files_changed": "src/a.rs",
            "tests_run": [],
            "tests_passed": true,
            "partial": false,
            "notes": ""
        });
        assert!(validate_report(&json_val).is_err());

        let json_val2 = serde_json::json!({
            "files_changed": [],
            "tests_run": [],
            "tests_passed": "yes",
            "partial": false,
            "notes": ""
        });
        assert!(validate_report(&json_val2).is_err());
    }

    fn sample_group(kw: serde_json::Value) -> serde_json::Value {
        let mut base = serde_json::json!({
            "root_cause": "rc",
            "claims": ["c"],
            "contract_ids": ["AC-2"],
            "verdict": "FIX",
            "fix": "do x",
            "owns": "src/a.rs"
        });
        if let Some(obj) = kw.as_object() {
            for (k, v) in obj {
                base[k] = v.clone();
            }
        }
        base
    }

    #[test]
    fn test_valid_groups() {
        let json_val = serde_json::json!({
            "groups": [sample_group(serde_json::json!({}))]
        });
        let groups = validate_verdict(&json_val, &sample_contract_ids()).unwrap();
        assert_eq!(groups.len(), 1);
        assert!(groups[0].actionable());
        assert_eq!(groups[0].contract_ids, vec!["AC-2"]);
    }

    #[test]
    fn test_empty_groups_is_valid_no_claims() {
        let json_val = serde_json::json!({"groups": []});
        let groups = validate_verdict(&json_val, &sample_contract_ids()).unwrap();
        assert!(groups.is_empty());
    }

    #[test]
    fn test_bad_verdict_enum_rejected() {
        let json_val = serde_json::json!({
            "groups": [sample_group(serde_json::json!({"verdict": "MAYBE"}))]
        });
        assert!(validate_verdict(&json_val, &sample_contract_ids()).is_err());
    }

    #[test]
    fn test_unknown_contract_id_rejected() {
        let json_val = serde_json::json!({
            "groups": [sample_group(serde_json::json!({"contract_ids": ["AC-99"]}))]
        });
        assert!(validate_verdict(&json_val, &sample_contract_ids()).is_err());
    }

    #[test]
    fn test_missing_root_cause_rejected() {
        let json_val = serde_json::json!({
            "groups": [sample_group(serde_json::json!({"root_cause": ""}))]
        });
        assert!(validate_verdict(&json_val, &sample_contract_ids()).is_err());
    }

    #[test]
    fn test_actionable_verdicts() {
        let json_val = serde_json::json!({
            "groups": [
                sample_group(serde_json::json!({"verdict": "FIX"})),
                sample_group(serde_json::json!({"verdict": "REDESIGN"})),
                sample_group(serde_json::json!({"verdict": "REPORT_ONLY"})),
                sample_group(serde_json::json!({"verdict": "DISMISS"})),
            ]
        });
        let groups = validate_verdict(&json_val, &sample_contract_ids()).unwrap();
        assert_eq!(
            groups.iter().map(|g| g.actionable()).collect::<Vec<_>>(),
            vec![true, true, false, false]
        );
    }

    #[test]
    fn test_class_defaults_to_code_defect() {
        let json_val = serde_json::json!({
            "groups": [sample_group(serde_json::json!({}))]
        });
        let groups = validate_verdict(&json_val, &sample_contract_ids()).unwrap();
        assert_eq!(groups[0].defect_class, "code_defect");
        assert!(groups[0].blocking());
        assert!(!groups[0].polish());
    }

    #[test]
    fn test_unknown_class_rejected() {
        let json_val = serde_json::json!({
            "groups": [sample_group(serde_json::json!({"class": "style"}))]
        });
        assert!(validate_verdict(&json_val, &sample_contract_ids()).is_err());
    }

    #[test]
    fn test_doc_and_evidence_classes_are_polish_not_blocking() {
        let json_val = serde_json::json!({
            "groups": [
                sample_group(serde_json::json!({"class": "doc_drift"})),
                sample_group(serde_json::json!({"class": "evidence_gap"})),
            ]
        });
        let groups = validate_verdict(&json_val, &sample_contract_ids()).unwrap();
        assert_eq!(
            groups.iter().map(|g| g.blocking()).collect::<Vec<_>>(),
            vec![false, false]
        );
        assert_eq!(
            groups.iter().map(|g| g.polish()).collect::<Vec<_>>(),
            vec![true, true]
        );
    }

    #[test]
    fn test_redesign_blocks_whatever_its_class() {
        let json_val = serde_json::json!({
            "groups": [sample_group(serde_json::json!({"verdict": "REDESIGN", "class": "doc_drift"}))]
        });
        let groups = validate_verdict(&json_val, &sample_contract_ids()).unwrap();
        assert!(groups[0].blocking());
    }

    #[test]
    fn test_non_actionable_verdicts_are_neither_blocking_nor_polish() {
        let json_val = serde_json::json!({
            "groups": [
                sample_group(serde_json::json!({"verdict": "REPORT_ONLY", "class": "doc_drift"})),
                sample_group(serde_json::json!({"verdict": "DISMISS"})),
            ]
        });
        let groups = validate_verdict(&json_val, &sample_contract_ids()).unwrap();
        assert_eq!(
            groups.iter().map(|g| g.blocking()).collect::<Vec<_>>(),
            vec![false, false]
        );
        assert_eq!(
            groups.iter().map(|g| g.polish()).collect::<Vec<_>>(),
            vec![false, false]
        );
    }

    #[test]
    fn test_valid_plan() {
        let json_val = serde_json::json!({
            "lanes": [{
                "id": "F1",
                "owns": ["src/auth/**"],
                "forbidden": [],
                "tests": ["t"],
                "brief": "b",
                "addresses": ["rc"]
            }]
        });
        let lanes = validate_plan(&json_val).unwrap();
        assert_eq!(lanes[0].id, "F1");
        assert_eq!(lanes[0].addresses, vec!["rc"]);
    }

    #[test]
    fn test_empty_owns_rejected() {
        let json_val = serde_json::json!({
            "lanes": [{"id": "F1", "owns": []}]
        });
        assert!(validate_plan(&json_val).is_err());
    }

    #[test]
    fn test_empty_lane_list_rejected() {
        let json_val = serde_json::json!({"lanes": []});
        assert!(validate_plan(&json_val).is_err());
    }

    #[test]
    fn test_duplicate_lane_ids_rejected() {
        let json_val = serde_json::json!({
            "lanes": [
                {"id": "F1", "owns": ["a/**"]},
                {"id": "F1", "owns": ["b/**"]},
            ]
        });
        assert!(validate_plan(&json_val).is_err());
    }

    #[test]
    fn test_defaults_for_optional_fields() {
        let json_val = serde_json::json!({
            "lanes": [{"id": "F1", "owns": ["a/**"]}]
        });
        let lanes = validate_plan(&json_val).unwrap();
        assert_eq!(lanes[0].forbidden, Vec::<String>::new());
        assert_eq!(lanes[0].tests, Vec::<String>::new());
        assert_eq!(lanes[0].brief, "");
    }

    #[test]
    fn test_validate_stages_valid() {
        let json_val = serde_json::json!({
            "stages": [
                {"slug": "01-core", "brief": "Core types", "owns": ["src/types/**"], "contract_ids": ["AC-1"]},
                {"slug": "02-engine", "brief": "Engine", "owns": ["src/engine/**"], "contract_ids": ["AC-2"]},
            ]
        });
        let valid_ids: HashSet<String> = ["AC-1", "AC-2", "AC-3"].iter().map(|s| s.to_string()).collect();
        let stages = validate_stages(&json_val, Some(&valid_ids)).unwrap();
        assert_eq!(stages.len(), 2);
        assert_eq!(stages[0].slug, "01-core");
        assert_eq!(stages[1].slug, "02-engine");
    }

    #[test]
    fn test_validate_stages_invalid_contract_id() {
        let json_val = serde_json::json!({
            "stages": [
                {"slug": "01-core", "brief": "Core types", "owns": ["src/**"], "contract_ids": ["UNKNOWN-99"]},
            ]
        });
        let valid_ids: HashSet<String> = ["AC-1"].iter().map(|s| s.to_string()).collect();
        assert!(validate_stages(&json_val, Some(&valid_ids)).is_err());
    }

    #[test]
    fn test_extract_planner_result_detects_both_kinds() {
        let plan_text = "```gauntlet-plan\n{\"lanes\": [{\"id\": \"L1\", \"owns\": [\"a\"], \"brief\": \"b\"}]}\n```";
        match extract_planner_result(plan_text, None).unwrap() {
            PlannerResult::Lanes(lanes) => {
                assert_eq!(lanes[0].id, "L1");
            }
            _ => panic!("expected lanes"),
        }

        let stage_text = "```gauntlet-stages\n{\"stages\": [{\"slug\": \"s1\", \"brief\": \"b\", \"owns\": [\"a\"]}]}\n```";
        match extract_planner_result(stage_text, None).unwrap() {
            PlannerResult::Stages(stages) => {
                assert_eq!(stages[0].slug, "s1");
            }
            _ => panic!("expected stages"),
        }
    }
}
