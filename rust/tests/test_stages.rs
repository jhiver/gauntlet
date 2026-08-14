use std::collections::HashSet;
use tempfile::tempdir;

use gauntlet::mission::{create_stage_mission, Mission, Repo, StageSpec};
use gauntlet::verdicts::{extract_planner_result, validate_stages, PlannerResult};

#[test]
fn test_validate_stages_valid() {
    let data = serde_json::json!({
        "stages": [
            {"slug": "01-core", "brief": "Core types", "owns": ["src/types/**"], "contract_ids": ["AC-1"]},
            {"slug": "02-engine", "brief": "Engine", "owns": ["src/engine/**"], "contract_ids": ["AC-2"]},
        ]
    });
    let valid_cids: HashSet<String> = ["AC-1", "AC-2", "AC-3"].iter().map(|s| s.to_string()).collect();
    let stages = validate_stages(&data, Some(&valid_cids)).unwrap();
    assert_eq!(stages.len(), 2);
    assert_eq!(stages[0].slug, "01-core");
    assert_eq!(stages[1].slug, "02-engine");
}

#[test]
fn test_validate_stages_invalid_contract_id() {
    let data = serde_json::json!({
        "stages": [
            {"slug": "01-core", "brief": "Core types", "owns": ["src/**"], "contract_ids": ["UNKNOWN-99"]},
        ]
    });
    let valid_cids: HashSet<String> = ["AC-1"].iter().map(|s| s.to_string()).collect();
    assert!(validate_stages(&data, Some(&valid_cids)).is_err());
}

#[test]
fn test_extract_planner_result_detects_both_kinds() {
    let plan_text = "```gauntlet-plan\n{\"lanes\": [{\"id\": \"L1\", \"owns\": [\"a\"], \"brief\": \"b\"}]}\n```";
    let res1 = extract_planner_result(plan_text, None).unwrap();
    match res1 {
        PlannerResult::Lanes(lanes) => {
            assert_eq!(lanes.len(), 1);
            assert_eq!(lanes[0].id, "L1");
        }
        _ => panic!("expected lanes"),
    }

    let stage_text = "```gauntlet-stages\n{\"stages\": [{\"slug\": \"s1\", \"brief\": \"b\", \"owns\": [\"a\"]}]}\n```";
    let res2 = extract_planner_result(stage_text, None).unwrap();
    match res2 {
        PlannerResult::Stages(stages) => {
            assert_eq!(stages.len(), 1);
            assert_eq!(stages[0].slug, "s1");
        }
        _ => panic!("expected stages"),
    }
}

#[test]
fn test_create_stage_mission_retains_parent_invariants() {
    let dir = tempdir().unwrap();
    let parent = Mission {
        slug: "parent-epic".to_string(),
        repos: vec![Repo {
            path: dir.path().to_str().unwrap().to_string(),
            target_branch: "master".to_string(),
            gates: vec!["npm test".to_string()],
        }],
        lanes: vec![],
        body: "# Objective\nBig Epic\n## Invariants\n- INV-1: Never delete logs\n## Non-Goals\n- NG-1: No Rust\n".to_string(),
        contract_ids: ["INV-1", "NG-1"].iter().map(|s| s.to_string()).collect(),
        source_path: dir.path().join("parent.md"),
    };
    let stage_spec = StageSpec {
        slug: "01-schema".to_string(),
        brief: "Implement DB schema".to_string(),
        owns: vec!["db/**".to_string()],
        contract_ids: vec![],
        gates: vec![],
    };
    let sub_path = dir.path().join("sub.md");
    let sub = create_stage_mission(&parent, &stage_spec, "master", &sub_path).unwrap();
    assert_eq!(sub.slug, "parent-epic-01-schema");
    assert!(sub.body.contains("INV-1: Never delete logs"));
    assert!(sub.body.contains("NG-1: No Rust"));
    assert!(sub.body.contains("parent-epic"));
    assert_eq!(sub.lanes.len(), 1);
    assert_eq!(sub.lanes[0].id, "L1");
    assert_eq!(sub.lanes[0].owns, vec!["db/**"]);
}
