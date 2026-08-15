use gauntlet::config::{validate_config, PolicyConfig};
use gauntlet::statemachine::State;

#[test]
fn test_policy_on_blocked_validation() {
    let mut table = toml::Table::new();
    let mut policy = toml::Table::new();
    policy.insert("on_blocked".to_string(), toml::Value::String("auto_heal".to_string()));
    policy.insert("auto_heal_budget".to_string(), toml::Value::Integer(3));
    policy.insert("on_wave_cap".to_string(), toml::Value::String("checkpoint".to_string()));
    policy.insert("max_total_waves".to_string(), toml::Value::Integer(5));
    policy.insert("idle_timeout_s".to_string(), toml::Value::Integer(900));
    policy.insert("hard_timeout_s".to_string(), toml::Value::Integer(2700));
    policy.insert("lane_timeout_s".to_string(), toml::Value::Integer(5400));
    table.insert("policy".to_string(), toml::Value::Table(policy));

    let mut roles = toml::Table::new();
    for r in gauntlet::config::ROLES {
        let mut rc = toml::Table::new();
        let mut chain = Vec::new();
        let mut link = toml::Table::new();
        link.insert("harness".to_string(), toml::Value::String("echo".to_string()));
        chain.push(toml::Value::Table(link));
        rc.insert("chain".to_string(), toml::Value::Array(chain));
        roles.insert(r.to_string(), toml::Value::Table(rc));
    }
    table.insert("roles".to_string(), toml::Value::Table(roles));

    let mut harnesses = toml::Table::new();
    let mut echo = toml::Table::new();
    echo.insert("adapter".to_string(), toml::Value::String("echo".to_string()));
    echo.insert("supports_write".to_string(), toml::Value::Boolean(true));
    harnesses.insert("echo".to_string(), toml::Value::Table(echo));
    table.insert("harnesses".to_string(), toml::Value::Table(harnesses));

    assert!(validate_config(&table).is_ok());

    // Invalid on_blocked
    if let Some(toml::Value::Table(p)) = table.get_mut("policy") {
        p.insert("on_blocked".to_string(), toml::Value::String("invalid_action".to_string()));
    }
    assert!(validate_config(&table).is_err());
}

#[test]
fn test_default_policy_has_auto_heal() {
    let policy = PolicyConfig::default();
    assert_eq!(policy.on_blocked, "auto_heal");
    assert_eq!(policy.auto_heal_budget, 2);
}

#[test]
fn test_state_tracks_auto_heal() {
    let state = State::default();
    assert_eq!(state.auto_heal_attempts, 0);
    assert_eq!(state.gate_auto_heal_attempts, 0);
    assert!(state.safety_pruned_files.is_empty());
}

#[test]
fn test_auto_prune_filters_unowned_files() {
    let owns = vec!["src/auth/**".to_string()];
    let forbidden = vec!["src/auth/secret.rs".to_string()];
    let changed = vec![
        "src/auth/login.rs".to_string(),
        "unowned_noise.txt".to_string(),
        "src/auth/secret.rs".to_string(),
    ];

    let unowned: Vec<String> = changed
        .iter()
        .filter(|p| {
            !owns.iter().any(|g| gauntlet::worktrees::glob_matches(g, p))
                || forbidden.iter().any(|g| gauntlet::worktrees::glob_matches(g, p))
        })
        .cloned()
        .collect();

    assert_eq!(unowned, vec!["unowned_noise.txt".to_string(), "src/auth/secret.rs".to_string()]);

    let clean_changed: Vec<String> = changed
        .into_iter()
        .filter(|p| !unowned.contains(p))
        .collect();

    let violations = gauntlet::worktrees::check_lane_diff(&clean_changed, &owns, &forbidden);
    assert!(violations.is_empty());
}
