use std::collections::HashMap;
use tempfile::tempdir;

use gauntlet::config::{
    builtin_defaults, dump_toml, load_config, merge, Config, WorktreeConfig,
};

#[test]
fn test_worktree_config_default_values() {
    let wt_default = WorktreeConfig::default();
    assert_eq!(wt_default.symlinks, Vec::<String>::new());
    assert!(wt_default.extra.is_empty());

    let config_default = Config::default();
    assert_eq!(config_default.worktree.symlinks, Vec::<String>::new());

    let dir = tempdir().unwrap();
    let loaded = load_config(Some(dir.path()), None, None).unwrap();
    assert_eq!(loaded.worktree.symlinks, Vec::<String>::new());
}

#[test]
fn test_worktree_config_parsing_from_table() {
    let mut table = builtin_defaults();
    let override_table: toml::Table = toml::from_str(
        r#"
[worktree]
symlinks = [".venv", "node_modules", "custom_cache"]
"#,
    )
    .unwrap();
    merge(&mut table, &override_table);

    let config = Config::from_table(table).unwrap();
    assert_eq!(
        config.worktree.symlinks,
        vec![
            ".venv".to_string(),
            "node_modules".to_string(),
            "custom_cache".to_string()
        ]
    );
}

#[test]
fn test_worktree_config_parsing_via_load_config() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("gauntlet.toml");
    std::fs::write(
        &config_file,
        r#"
[worktree]
symlinks = [".venv", "node_modules", "custom_cache"]
"#,
    )
    .unwrap();

    let config = load_config(None, None, Some(&config_file)).unwrap();
    assert_eq!(
        config.worktree.symlinks,
        vec![
            ".venv".to_string(),
            "node_modules".to_string(),
            "custom_cache".to_string()
        ]
    );
}

#[test]
fn test_worktree_config_missing_section_uses_defaults() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("gauntlet.toml");
    std::fs::write(
        &config_file,
        r#"
[policy]
max_total_waves = 3
"#,
    )
    .unwrap();

    let config = load_config(None, None, Some(&config_file)).unwrap();
    assert_eq!(config.worktree.symlinks, Vec::<String>::new());
    assert_eq!(config.policy.max_total_waves, 3);
}

#[test]
fn test_worktree_config_empty_symlinks_array() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("gauntlet.toml");
    std::fs::write(
        &config_file,
        r#"
[worktree]
symlinks = []
"#,
    )
    .unwrap();

    let config = load_config(None, None, Some(&config_file)).unwrap();
    assert_eq!(config.worktree.symlinks, Vec::<String>::new());
}

#[test]
fn test_worktree_config_layer_merging_and_override() {
    let dir = tempdir().unwrap();
    let tool_dir = dir.path().join("tool");
    std::fs::create_dir(&tool_dir).unwrap();
    std::fs::write(
        tool_dir.join("gauntlet.toml"),
        "[worktree]\nsymlinks = [\"tool_dep\"]\n",
    )
    .unwrap();

    let mission_dir = dir.path().join("mission");
    std::fs::create_dir(&mission_dir).unwrap();
    std::fs::write(
        mission_dir.join("gauntlet.toml"),
        "[worktree]\nsymlinks = [\"mission_dep_1\", \"mission_dep_2\"]\n",
    )
    .unwrap();

    let override_file = dir.path().join("override.toml");
    std::fs::write(
        &override_file,
        "[worktree]\nsymlinks = [\".venv\", \"node_modules\"]\n",
    )
    .unwrap();

    // 1. Tool dir only
    let cfg = load_config(Some(&tool_dir), None, None).unwrap();
    assert_eq!(cfg.worktree.symlinks, vec!["tool_dep".to_string()]);

    // 2. Tool dir + mission dir (mission overrides tool)
    let cfg = load_config(Some(&tool_dir), Some(&mission_dir), None).unwrap();
    assert_eq!(
        cfg.worktree.symlinks,
        vec!["mission_dep_1".to_string(), "mission_dep_2".to_string()]
    );

    // 3. Tool dir + mission dir + explicit --config (config overrides all)
    let cfg = load_config(Some(&tool_dir), Some(&mission_dir), Some(&override_file)).unwrap();
    assert_eq!(
        cfg.worktree.symlinks,
        vec![".venv".to_string(), "node_modules".to_string()]
    );
}

#[test]
fn test_worktree_config_serialization_roundtrip() {
    let config = Config {
        worktree: WorktreeConfig {
            symlinks: vec![
                ".venv".to_string(),
                "vendor".to_string(),
                ".cache/build".to_string(),
            ],
            extra: HashMap::new(),
        },
        ..Default::default()
    };

    let table = config.to_table().unwrap();
    let dumped = dump_toml(&table).unwrap();
    let reloaded_table: toml::Table = toml::from_str(&dumped).unwrap();
    let reloaded_config = Config::from_table(reloaded_table).unwrap();

    assert_eq!(config.worktree.symlinks, reloaded_config.worktree.symlinks);
}

#[test]
fn test_builtin_defaults_roundtrip_with_worktree() {
    let defaults = builtin_defaults();
    let dumped = dump_toml(&defaults).unwrap();
    let reloaded: toml::Table = toml::from_str(&dumped).unwrap();
    assert_eq!(reloaded, defaults);

    let config = Config::from_table(reloaded).unwrap();
    assert_eq!(config.worktree.symlinks, Vec::<String>::new());
}

#[test]
fn test_worktree_validation_invalid_worktree_type() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("bad.toml");
    std::fs::write(&config_file, "worktree = \"not_a_table\"\n").unwrap();

    let err = load_config(Some(dir.path()), None, Some(&config_file)).unwrap_err();
    assert!(err.to_string().contains("worktree must be a table"));
}

#[test]
fn test_worktree_validation_invalid_symlinks_scalar() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("bad.toml");
    std::fs::write(&config_file, "[worktree]\nsymlinks = \"not_an_array\"\n").unwrap();

    let err = load_config(Some(dir.path()), None, Some(&config_file)).unwrap_err();
    assert!(err.to_string().contains("worktree 'symlinks' must be an array of strings"));
}

#[test]
fn test_worktree_validation_invalid_symlinks_integer() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("bad.toml");
    std::fs::write(&config_file, "[worktree]\nsymlinks = 42\n").unwrap();

    let err = load_config(Some(dir.path()), None, Some(&config_file)).unwrap_err();
    assert!(err.to_string().contains("worktree 'symlinks' must be an array of strings"));
}

#[test]
fn test_worktree_validation_non_string_array_items() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("bad.toml");
    std::fs::write(&config_file, "[worktree]\nsymlinks = [\"valid\", 123]\n").unwrap();

    let err = load_config(Some(dir.path()), None, Some(&config_file)).unwrap_err();
    assert!(err.to_string().contains("worktree 'symlinks' must be an array of strings"));
}

#[test]
fn test_worktree_config_extra_fields_preserved() {
    let dir = tempdir().unwrap();
    let config_file = dir.path().join("gauntlet.toml");
    std::fs::write(
        &config_file,
        r#"
[worktree]
symlinks = [".venv"]
custom_cache_dir = "/tmp/cache"
shared_index = true
"#,
    )
    .unwrap();

    let config = load_config(None, None, Some(&config_file)).unwrap();
    assert_eq!(config.worktree.symlinks, vec![".venv".to_string()]);
    assert_eq!(
        config.worktree.extra.get("custom_cache_dir"),
        Some(&toml::Value::String("/tmp/cache".to_string()))
    );
    assert_eq!(
        config.worktree.extra.get("shared_index"),
        Some(&toml::Value::Boolean(true))
    );
}
