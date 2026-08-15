use std::collections::HashSet;
use std::path::PathBuf;

use gauntlet::autoroute::analyze_mission;
use gauntlet::mission::{Lane, Mission, Repo};

#[test]
fn test_high_risk_mission_detection() {
    let m = Mission {
        slug: "auth-secret-vault".to_string(),
        source_path: PathBuf::from("/tmp/m.md"),
        repos: vec![Repo {
            path: "/repo".to_string(),
            target_branch: "main".to_string(),
            gates: vec![
                "test1".to_string(),
                "test2".to_string(),
                "test3".to_string(),
                "test4".to_string(),
                "test5".to_string(),
            ],
        }],
        lanes: vec![Lane {
            id: "L1".to_string(),
            owns: ["lib/auth/**", "lib/passkey/**", "lib/vault/**"]
                .repeat(8)
                .into_iter()
                .map(|s| s.to_string())
                .collect(),
            forbidden: vec![],
            tests: vec![],
            brief: "Implement passkey auth takeover".to_string(),
            addresses: vec![],
        }],
        body: "# Objective\nImplement secure authentication with passkey credentials\n## AC\n- AC-1: secret\n".to_string(),
        contract_ids: {
            let mut s = HashSet::new();
            s.insert("AC-1".to_string());
            s
        },
    };
    let profile = analyze_mission(&m);
    assert_eq!(profile.tier, "high-risk");
    assert!(profile.score >= 3);
    assert_eq!(
        profile.roles.get("reviewer").unwrap().chain[0].harness,
        "codex"
    );
    assert_eq!(
        profile.roles.get("reviewer").unwrap().chain[0].effort,
        Some("xhigh".to_string())
    );
}

#[test]
fn test_standard_mission_detection() {
    let m = Mission {
        slug: "button-color".to_string(),
        source_path: PathBuf::from("/tmp/m.md"),
        repos: vec![Repo {
            path: "/repo".to_string(),
            target_branch: "main".to_string(),
            gates: vec!["npm test".to_string()],
        }],
        lanes: vec![Lane {
            id: "L1".to_string(),
            owns: vec!["src/button.ts".to_string(), "src/styles.css".to_string()],
            forbidden: vec![],
            tests: vec![],
            brief: "Change button color".to_string(),
            addresses: vec![],
        }],
        body: "# Objective\nChange button color from blue to green\n## AC\n- AC-1: green\n"
            .to_string(),
        contract_ids: {
            let mut s = HashSet::new();
            s.insert("AC-1".to_string());
            s
        },
    };
    let profile = analyze_mission(&m);
    assert_eq!(profile.tier, "fast");
    assert_eq!(
        profile.roles.get("implementer").unwrap().chain[0].harness,
        "agy"
    );
}
