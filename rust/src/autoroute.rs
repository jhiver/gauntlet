//! Super-Auto Routing: Automatic model selection and fallback chain generator
//! calibrated against the Artificial Analysis Pareto frontier (Speed vs Intelligence).
//!
//! Evaluates mission contract complexity, security/safety risk factors, and scope
//! to construct optimal model routing and fallback chains without requiring
//! manual configuration files.

use std::collections::{BTreeSet, HashMap};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::config::{ChainLink, RoleConfig};
use crate::mission::Mission;

pub const HIGH_RISK_PATTERNS: &[&str] = &[
    r"\bauth\w*", r"\btoken\w*", r"\bsecret\w*", r"\bcredential\w*",
    r"\bpasskey\w*", r"\bvault\w*", r"\bcrypto\w*", r"\btakeover\w*",
    r"\bsession\w*", r"\bconcurren\w*", r"\brace\w*", r"\bthread\w*",
    r"\bmutex\w*", r"\block\w*", r"\bdistributed\w*", r"\brecovery\w*",
    r"\bsafety\w*", r"\bpayment\w*", r"\btransaction\w*", r"\bdataloss\w*",
    r"\bsecurity\w*", r"\bexploit\w*", r"\bpermission\w*", r"\bisolation\w*",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionProfile {
    pub tier: String, // "high-risk", "standard", "fast"
    pub score: usize,
    pub reasons: Vec<String>,
    pub roles: HashMap<String, RoleConfig>,
}

fn link(harness: &str, model: Option<&str>, effort: Option<&str>) -> ChainLink {
    ChainLink {
        harness: harness.to_string(),
        model: model.map(|s| s.to_string()),
        effort: effort.map(|s| s.to_string()),
        extra: HashMap::new(),
    }
}

fn role_chain(links: Vec<ChainLink>) -> RoleConfig {
    RoleConfig { chain: links }
}

/// Analyze a Mission instance and return the recommended profile + role routing.
pub fn analyze_mission(mission: &Mission) -> MissionProfile {
    let mut parts = Vec::new();
    parts.push(mission.body.as_str());
    parts.push(mission.slug.as_str());
    for lane in &mission.lanes {
        parts.push(lane.brief.as_str());
    }
    let text_to_scan = parts.join(" ");

    let mut reasons = Vec::new();
    let mut score = 0;

    // 1. High-risk keyword matching
    let mut matched_keywords = BTreeSet::new();
    for pat in HIGH_RISK_PATTERNS {
        if let Ok(re) = Regex::new(&format!("(?i){pat}")) {
            let mut count = 0;
            for mat in re.find_iter(&text_to_scan) {
                matched_keywords.insert(mat.as_str().to_lowercase());
                count += 1;
                if count >= 3 {
                    break;
                }
            }
        }
    }

    if !matched_keywords.is_empty() {
        let kw_sample: Vec<String> = matched_keywords.into_iter().take(5).collect();
        score += 3;
        reasons.push(format!(
            "Security/High-risk concepts detected ({})",
            kw_sample.join(", ")
        ));
    }

    // 2. Scope analysis (owned paths)
    let total_owns: usize = mission.lanes.iter().map(|l| l.owns.len()).sum();
    if total_owns >= 20 {
        score += 2;
        reasons.push(format!("Large scope: {total_owns} owned path patterns"));
    } else if total_owns >= 8 {
        score += 1;
        reasons.push(format!("Moderate scope: {total_owns} owned path patterns"));
    }

    // 3. Gates complexity
    let total_gates: usize = mission.repos.iter().map(|r| r.gates.len()).sum();
    if total_gates >= 5 {
        score += 1;
        reasons.push(format!("High-assurance gate suite ({total_gates} gates)"));
    }

    // 4. Invariants and AC count
    let contract_count = mission.contract_ids.len();
    if contract_count >= 8 {
        score += 1;
        reasons.push(format!(
            "Rigorous contract ({contract_count} AC/INV clauses)"
        ));
    }

    // Tier selection
    let (tier, roles) = if score >= 3 {
        let mut r = HashMap::new();
        r.insert(
            "implementer".to_string(),
            role_chain(vec![
                link("agy", None, None), // gemini-3.7-flash-high
                link("codex", Some("gpt-5.6-sol"), Some("xhigh")),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
                link("kimi", Some("kimi-code/k3"), None),
            ]),
        );
        r.insert(
            "fixer".to_string(),
            role_chain(vec![
                link("codex", Some("gpt-5.6-sol"), Some("xhigh")),
                link("agy", None, None),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
            ]),
        );
        r.insert(
            "reviewer".to_string(),
            role_chain(vec![
                link("codex", Some("gpt-5.6-sol"), Some("xhigh")),
                link("kimi", Some("kimi-code/k3"), None),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
            ]),
        );
        r.insert(
            "judge".to_string(),
            role_chain(vec![
                link("codex", Some("gpt-5.6-sol"), Some("xhigh")),
                link("kimi", Some("kimi-code/k3"), None),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
            ]),
        );
        r.insert(
            "planner".to_string(),
            role_chain(vec![
                link("codex", Some("gpt-5.6-sol"), Some("xhigh")),
                link("kimi", Some("kimi-code/k3"), None),
            ]),
        );
        r.insert(
            "director".to_string(),
            role_chain(vec![link("human", None, None)]),
        );
        ("high-risk".to_string(), r)
    } else if score >= 1 {
        let mut r = HashMap::new();
        r.insert(
            "implementer".to_string(),
            role_chain(vec![
                link("agy", None, None),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
                link("kimi", Some("kimi-code/k3"), None),
            ]),
        );
        r.insert(
            "fixer".to_string(),
            role_chain(vec![
                link("agy", None, None),
                link("codex", Some("gpt-5.6-sol"), Some("high")),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
            ]),
        );
        r.insert(
            "reviewer".to_string(),
            role_chain(vec![
                link("kimi", Some("kimi-code/k3"), None),
                link("codex", Some("gpt-5.6-sol"), Some("high")),
            ]),
        );
        r.insert(
            "judge".to_string(),
            role_chain(vec![
                link("kimi", Some("kimi-code/k3"), None),
                link("codex", Some("gpt-5.6-sol"), Some("high")),
            ]),
        );
        r.insert(
            "planner".to_string(),
            role_chain(vec![
                link("kimi", Some("kimi-code/k3"), None),
                link("codex", Some("gpt-5.6-sol"), Some("high")),
            ]),
        );
        r.insert(
            "director".to_string(),
            role_chain(vec![link("human", None, None)]),
        );
        ("standard".to_string(), r)
    } else {
        reasons.push("Localized scope without high-risk invariants".to_string());
        let mut r = HashMap::new();
        r.insert(
            "implementer".to_string(),
            role_chain(vec![
                link("agy", None, None),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
            ]),
        );
        r.insert(
            "fixer".to_string(),
            role_chain(vec![
                link("agy", None, None),
                link("cmd", Some("gpt-5.6-luna"), Some("max")),
            ]),
        );
        r.insert(
            "reviewer".to_string(),
            role_chain(vec![
                link("kimi", Some("kimi-code/k3"), None),
                link("agy", None, None),
            ]),
        );
        r.insert(
            "judge".to_string(),
            role_chain(vec![
                link("kimi", Some("kimi-code/k3"), None),
                link("agy", None, None),
            ]),
        );
        r.insert(
            "planner".to_string(),
            role_chain(vec![link("kimi", Some("kimi-code/k3"), None)]),
        );
        r.insert(
            "director".to_string(),
            role_chain(vec![link("human", None, None)]),
        );
        ("fast".to_string(), r)
    };

    MissionProfile {
        tier,
        score,
        reasons,
        roles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission::{Lane, Repo};
    use std::collections::HashSet;
    use std::path::PathBuf;

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
}
