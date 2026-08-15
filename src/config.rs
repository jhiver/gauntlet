//! TOML config load + merge + chain validation.
//!
//! Resolution order (later overrides): built-in defaults -> <tool>/gauntlet.toml
//! -> <mission-dir>/gauntlet.toml -> --config FILE.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const ADAPTER_NAMES: &[&str] = &["agy", "cmd", "codex", "kimi", "reasonix", "human", "echo"];
pub const ROLES: &[&str] = &["implementer", "fixer", "reviewer", "judge", "planner", "director"];
pub const WRITE_ROLES: &[&str] = &["implementer", "fixer"];
pub const FALLBACK_ACTIONS: &[&str] = &[
    "next",
    "next_and_break",
    "break",
    "backoff_retry_then_next",
    "retry_once_then_next",
];
pub const WAVE_CAP_ACTIONS: &[&str] = &["block", "checkpoint"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConfigError {
    #[error("{0}")]
    Message(String),
}

impl From<String> for ConfigError {
    fn from(s: String) -> Self {
        ConfigError::Message(s)
    }
}

impl From<&str> for ConfigError {
    fn from(s: &str) -> Self {
        ConfigError::Message(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HarnessConfig {
    pub adapter: String,
    #[serde(default)]
    pub supports_write: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub errors: Option<HashMap<String, Vec<String>>>,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChainLink {
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoleConfig {
    #[serde(default)]
    pub chain: Vec<ChainLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyConfig {
    #[serde(default = "default_checkpoints")]
    pub checkpoints: Vec<String>,
    #[serde(default = "default_max_total_waves")]
    pub max_total_waves: usize,
    #[serde(default = "default_on_wave_cap")]
    pub on_wave_cap: String,
    #[serde(default = "default_idle_timeout_s")]
    pub idle_timeout_s: u64,
    #[serde(default = "default_hard_timeout_s")]
    pub hard_timeout_s: u64,
    #[serde(default = "default_lane_timeout_s")]
    pub lane_timeout_s: u64,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

fn default_checkpoints() -> Vec<String> {
    vec!["plan".to_string(), "deliver".to_string()]
}
fn default_max_total_waves() -> usize {
    5
}
fn default_on_wave_cap() -> String {
    "checkpoint".to_string()
}
fn default_idle_timeout_s() -> u64 {
    900
}
fn default_hard_timeout_s() -> u64 {
    2700
}
fn default_lane_timeout_s() -> u64 {
    5400
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FallbackConfig {
    #[serde(default = "default_on_quota")]
    pub on_quota: String,
    #[serde(default = "default_on_auth")]
    pub on_auth: String,
    #[serde(default = "default_on_rate_limit")]
    pub on_rate_limit: String,
    #[serde(default = "default_on_model_unavailable")]
    pub on_model_unavailable: String,
    #[serde(default = "default_on_timeout")]
    pub on_timeout: String,
    #[serde(default = "default_on_crash")]
    pub on_crash: String,
    #[serde(default = "default_on_invalid_output")]
    pub on_invalid_output: String,
    #[serde(default = "default_max_attempts_per_task")]
    pub max_attempts_per_task: usize,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

fn default_on_quota() -> String {
    "next_and_break".to_string()
}
fn default_on_auth() -> String {
    "break".to_string()
}
fn default_on_rate_limit() -> String {
    "next".to_string()
}
fn default_on_model_unavailable() -> String {
    "next".to_string()
}
fn default_on_timeout() -> String {
    "retry_once_then_next".to_string()
}
fn default_on_crash() -> String {
    "retry_once_then_next".to_string()
}
fn default_on_invalid_output() -> String {
    "retry_once_then_next".to_string()
}
fn default_max_attempts_per_task() -> usize {
    3
}
impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            checkpoints: default_checkpoints(),
            max_total_waves: default_max_total_waves(),
            on_wave_cap: default_on_wave_cap(),
            idle_timeout_s: default_idle_timeout_s(),
            hard_timeout_s: default_hard_timeout_s(),
            lane_timeout_s: default_lane_timeout_s(),
            extra: HashMap::new(),
        }
    }
}

impl Default for FallbackConfig {
    fn default() -> Self {
        Self {
            on_quota: default_on_quota(),
            on_auth: default_on_auth(),
            on_rate_limit: default_on_rate_limit(),
            on_model_unavailable: default_on_model_unavailable(),
            on_timeout: default_on_timeout(),
            on_crash: default_on_crash(),
            on_invalid_output: default_on_invalid_output(),
            max_attempts_per_task: default_max_attempts_per_task(),
            extra: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default)]
    pub harnesses: HashMap<String, HarnessConfig>,
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
    #[serde(default)]
    pub policy: PolicyConfig,
    #[serde(default)]
    pub fallback: FallbackConfig,
    #[serde(flatten)]
    pub extra: HashMap<String, toml::Value>,
}

impl Default for Config {
    fn default() -> Self {
        let table = builtin_defaults();
        toml::Value::Table(table).try_into().unwrap_or_else(|_| Config {
            harnesses: HashMap::new(),
            roles: HashMap::new(),
            policy: PolicyConfig::default(),
            fallback: FallbackConfig::default(),
            extra: HashMap::new(),
        })
    }
}

impl Config {
    pub fn from_table(table: toml::Table) -> Result<Self, ConfigError> {
        validate_config(&table)?;
        toml::Value::Table(table)
            .try_into()
            .map_err(|e: toml::de::Error| ConfigError::Message(e.to_string()))
    }

    pub fn to_table(&self) -> Result<toml::Table, ConfigError> {
        let val = toml::Value::try_from(self)
            .map_err(|e: toml::ser::Error| ConfigError::Message(e.to_string()))?;
        match val {
            toml::Value::Table(t) => Ok(t),
            _ => Err(ConfigError::Message("config is not a table".to_string())),
        }
    }
}

pub fn builtin_defaults() -> toml::Table {
    let toml_str = r#"
[harnesses.echo]
adapter = "echo"
supports_write = true

[harnesses.human]
adapter = "human"
supports_write = true

[roles.implementer]
chain = [ { harness = "echo" } ]

[roles.fixer]
chain = [ { harness = "echo" } ]

[roles.reviewer]
chain = [ { harness = "echo" } ]

[roles.judge]
chain = [ { harness = "echo" } ]

[roles.planner]
chain = [ { harness = "echo" } ]

[roles.director]
chain = [ { harness = "human" } ]

[policy]
checkpoints = ["plan", "deliver"]
max_total_waves = 5
on_wave_cap = "checkpoint"
idle_timeout_s = 900
hard_timeout_s = 2700
lane_timeout_s = 5400

[fallback]
on_quota = "next_and_break"
on_auth = "break"
on_rate_limit = "next"
on_model_unavailable = "next"
on_timeout = "retry_once_then_next"
on_crash = "retry_once_then_next"
on_invalid_output = "retry_once_then_next"
max_attempts_per_task = 3
"#;
    toml::from_str(toml_str).unwrap_or_default()
}

/// Deep merge: tables merge recursively, arrays/scalars are replaced.
pub fn merge(base: &mut toml::Table, override_table: &toml::Table) {
    for (key, value) in override_table {
        match (base.get_mut(key), value) {
            (Some(toml::Value::Table(base_sub)), toml::Value::Table(override_sub)) => {
                merge(base_sub, override_sub);
            }
            _ => {
                base.insert(key.clone(), value.clone());
            }
        }
    }
}

pub fn validate_config(cfg: &toml::Table) -> Result<(), ConfigError> {
    let adapter_set: HashSet<&str> = ADAPTER_NAMES.iter().copied().collect();
    let write_roles: HashSet<&str> = WRITE_ROLES.iter().copied().collect();
    let fallback_set: HashSet<&str> = FALLBACK_ACTIONS.iter().copied().collect();
    let wave_cap_set: HashSet<&str> = WAVE_CAP_ACTIONS.iter().copied().collect();

    let harnesses = match cfg.get("harnesses") {
        Some(toml::Value::Table(t)) => t,
        Some(_) => return Err(ConfigError::Message("harnesses must be a table".to_string())),
        None => &toml::Table::new(),
    };

    for (hname, hval) in harnesses {
        let hcfg = match hval {
            toml::Value::Table(t) => t,
            _ => {
                return Err(ConfigError::Message(format!(
                    "harness '{hname}': configuration must be a table"
                )))
            }
        };
        let adapter = match hcfg.get("adapter").and_then(|v| v.as_str()) {
            Some(a) => a,
            None => {
                return Err(ConfigError::Message(format!(
                    "harness '{hname}': missing or non-string adapter"
                )))
            }
        };
        if !adapter_set.contains(adapter) {
            return Err(ConfigError::Message(format!(
                "harness '{hname}': unknown adapter '{adapter}'"
            )));
        }
    }

    let roles = match cfg.get("roles") {
        Some(toml::Value::Table(t)) => t,
        Some(_) => return Err(ConfigError::Message("roles must be a table".to_string())),
        None => &toml::Table::new(),
    };

    for &role in ROLES {
        let role_cfg = roles.get(role).and_then(|v| v.as_table());
        let chain = role_cfg.and_then(|r| r.get("chain")).and_then(|v| v.as_array());
        let chain = match chain {
            Some(c) if !c.is_empty() => c,
            _ => return Err(ConfigError::Message(format!("role '{role}' has an empty chain"))),
        };

        for link in chain {
            let link_tbl = match link.as_table() {
                Some(t) => t,
                None => {
                    return Err(ConfigError::Message(format!(
                        "role '{role}': chain link must be a table"
                    )))
                }
            };
            let hname = match link_tbl.get("harness").and_then(|v| v.as_str()) {
                Some(h) => h,
                None => {
                    return Err(ConfigError::Message(format!(
                        "role '{role}': chain link missing harness name"
                    )))
                }
            };
            let target_harness = match harnesses.get(hname).and_then(|v| v.as_table()) {
                Some(t) => t,
                None => {
                    return Err(ConfigError::Message(format!(
                        "role '{role}': unknown harness '{hname}' in chain"
                    )))
                }
            };
            if write_roles.contains(role) {
                let supports_write = target_harness
                    .get("supports_write")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if !supports_write {
                    return Err(ConfigError::Message(format!(
                        "role '{role}': harness '{hname}' does not support write"
                    )));
                }
            }
        }
    }

    if let Some(toml::Value::Table(fallback)) = cfg.get("fallback") {
        for (key, value) in fallback {
            if key.starts_with("on_") {
                let val_str = value.as_str().unwrap_or("");
                if !fallback_set.contains(val_str) {
                    return Err(ConfigError::Message(format!(
                        "fallback '{key}': unknown action '{val_str}'"
                    )));
                }
            }
        }
        if let Some(val) = fallback.get("max_attempts_per_task") {
            if val.as_integer().is_none() {
                return Err(ConfigError::Message(
                    "fallback 'max_attempts_per_task' must be an integer".to_string(),
                ));
            }
        }
    }

    if let Some(policy_val) = cfg.get("policy") {
        let policy = match policy_val {
            toml::Value::Table(t) => t,
            _ => return Err(ConfigError::Message("policy must be a table".to_string())),
        };
        if policy.contains_key("max_fix_waves") {
            return Err(ConfigError::Message(
                "policy 'max_fix_waves' was replaced by 'max_total_waves' (an absolute safety cap; fix waves are now granted while the blocking-group count keeps falling). Rename the key.".to_string(),
            ));
        }
        for key in &["max_total_waves", "idle_timeout_s", "hard_timeout_s", "lane_timeout_s"] {
            match policy.get(*key) {
                Some(v) if v.as_integer().is_some() => {}
                _ => {
                    return Err(ConfigError::Message(format!(
                        "policy '{key}' must be an integer"
                    )))
                }
            }
        }
        let on_wave_cap = policy.get("on_wave_cap").and_then(|v| v.as_str()).unwrap_or("");
        if !wave_cap_set.contains(on_wave_cap) {
            return Err(ConfigError::Message(
                "policy 'on_wave_cap' must be one of [\"block\", \"checkpoint\"]".to_string(),
            ));
        }
    }

    Ok(())
}

pub fn load_config_table(
    tool_dir: Option<&Path>,
    mission_dir: Option<&Path>,
    config_file: Option<&Path>,
) -> Result<toml::Table, ConfigError> {
    let mut cfg = builtin_defaults();
    let mut candidates = Vec::new();

    if let Some(td) = tool_dir {
        candidates.push(td.join("gauntlet.toml"));
    }
    if let Some(md) = mission_dir {
        candidates.push(md.join("gauntlet.toml"));
    }
    if let Some(cf) = config_file {
        candidates.push(cf.to_path_buf());
    }

    for path in candidates {
        if path.is_file() {
            let content = std::fs::read_to_string(&path)
                .map_err(|e| ConfigError::Message(format!("{}: cannot read file: {}", path.display(), e)))?;
            let loaded: toml::Table = toml::from_str(&content)
                .map_err(|e| ConfigError::Message(format!("{}: invalid TOML: {}", path.display(), e)))?;
            merge(&mut cfg, &loaded);
        }
    }

    // "human" is the implicit terminal link of every chain; make sure the
    // harness entry always exists even if no config file defines it.
    let harnesses_entry = cfg
        .entry("harnesses".to_string())
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    if let toml::Value::Table(harnesses) = harnesses_entry {
        if !harnesses.contains_key("human") {
            let mut human_map = toml::Table::new();
            human_map.insert("adapter".to_string(), toml::Value::String("human".to_string()));
            human_map.insert("supports_write".to_string(), toml::Value::Boolean(true));
            harnesses.insert("human".to_string(), toml::Value::Table(human_map));
        }
    }

    validate_config(&cfg)?;
    Ok(cfg)
}

pub fn load_config(
    tool_dir: Option<&Path>,
    mission_dir: Option<&Path>,
    config_file: Option<&Path>,
) -> Result<Config, ConfigError> {
    let table = load_config_table(tool_dir, mission_dir, config_file)?;
    Config::from_table(table)
}

fn fmt_toml_value(value: &toml::Value) -> Result<String, ConfigError> {
    match value {
        toml::Value::Boolean(b) => Ok(if *b { "true".to_string() } else { "false".to_string() }),
        toml::Value::Integer(i) => Ok(i.to_string()),
        toml::Value::Float(f) => Ok(f.to_string()),
        toml::Value::String(s) => {
            serde_json::to_string(s).map_err(|e| ConfigError::Message(e.to_string()))
        }
        toml::Value::Datetime(dt) => Ok(dt.to_string()),
        toml::Value::Array(arr) => {
            if !arr.is_empty() && arr.iter().all(|item| item.is_table()) {
                let mut tables = Vec::new();
                for item in arr {
                    if let toml::Value::Table(t) = item {
                        let mut inners = Vec::new();
                        for (k, v) in t {
                            inners.push(format!("{k} = {}", fmt_toml_value(v)?));
                        }
                        tables.push(format!("{{ {} }}", inners.join(", ")));
                    }
                }
                Ok(format!("[\n  {},\n]", tables.join(",\n  ")))
            } else {
                let mut items = Vec::new();
                for item in arr {
                    items.push(fmt_toml_value(item)?);
                }
                Ok(format!("[{}]", items.join(", ")))
            }
        }
        toml::Value::Table(_) => Err(ConfigError::Message("cannot format inline table in value scalar".to_string())),
    }
}

pub fn dump_toml(cfg: &toml::Table) -> Result<String, ConfigError> {
    let mut lines: Vec<String> = Vec::new();

    fn emit(prefix: &str, table: &toml::Table, lines: &mut Vec<String>) -> Result<(), ConfigError> {
        let mut scalars: Vec<(&String, &toml::Value)> = Vec::new();
        let mut subtables: Vec<(&String, &toml::Table)> = Vec::new();

        for (k, v) in table {
            match v {
                toml::Value::Table(sub) => subtables.push((k, sub)),
                _ => scalars.push((k, v)),
            }
        }

        if !prefix.is_empty() {
            lines.push(format!("[{prefix}]"));
        }

        for (key, value) in scalars.iter() {
            lines.push(format!("{key} = {}", fmt_toml_value(value)?));
        }

        if !scalars.is_empty() || !prefix.is_empty() {
            lines.push("".to_string());
        }

        for (key, sub) in subtables {
            let next_prefix = if prefix.is_empty() {
                key.to_string()
            } else {
                format!("{prefix}.{key}")
            };
            emit(&next_prefix, sub, lines)?;
        }

        Ok(())
    }

    emit("", cfg, &mut lines)?;
    Ok(format!("{}\n", lines.join("\n").trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_builtin_defaults_are_valid_and_echo_based() {
        let dir = tempdir().unwrap();
        let cfg = load_config(Some(dir.path()), None, None).unwrap();
        assert_eq!(
            cfg.roles.get("implementer").unwrap().chain,
            vec![ChainLink {
                harness: "echo".to_string(),
                model: None,
                effort: None,
                extra: HashMap::new()
            }]
        );
        assert_eq!(
            cfg.roles.get("director").unwrap().chain,
            vec![ChainLink {
                harness: "human".to_string(),
                model: None,
                effort: None,
                extra: HashMap::new()
            }]
        );
        assert_eq!(cfg.fallback.max_attempts_per_task, 3);
    }

    #[test]
    fn test_merge_precedence() {
        let dir = tempdir().unwrap();
        let tool = dir.path().join("tool");
        std::fs::create_dir(&tool).unwrap();
        std::fs::write(
            tool.join("gauntlet.toml"),
            "[policy]\nmax_total_waves = 1\n[harnesses.cmd]\nadapter = \"cmd\"\nsupports_write = true\n",
        )
        .unwrap();

        let mission_dir = dir.path().join("mission");
        std::fs::create_dir(&mission_dir).unwrap();
        std::fs::write(
            mission_dir.join("gauntlet.toml"),
            "[policy]\nmax_total_waves = 2\n",
        )
        .unwrap();

        let override_file = dir.path().join("override.toml");
        std::fs::write(&override_file, "[policy]\nmax_total_waves = 5\n").unwrap();

        let cfg = load_config(Some(&tool), None, None).unwrap();
        assert_eq!(cfg.policy.max_total_waves, 1);
        assert!(cfg.harnesses.contains_key("cmd"));

        let cfg = load_config(Some(&tool), Some(&mission_dir), None).unwrap();
        assert_eq!(cfg.policy.max_total_waves, 2);

        let cfg = load_config(Some(&tool), Some(&mission_dir), Some(&override_file)).unwrap();
        assert_eq!(cfg.policy.max_total_waves, 5);
    }

    #[test]
    fn test_legacy_max_fix_waves_rejected() {
        let dir = tempdir().unwrap();
        let override_file = dir.path().join("legacy.toml");
        std::fs::write(&override_file, "[policy]\nmax_fix_waves = 2\n").unwrap();
        let err = load_config(Some(dir.path()), None, Some(&override_file)).unwrap_err();
        assert!(err.to_string().contains("max_total_waves"));
    }

    #[test]
    fn test_unknown_wave_cap_action_rejected() {
        let dir = tempdir().unwrap();
        let override_file = dir.path().join("bad.toml");
        std::fs::write(&override_file, "[policy]\non_wave_cap = \"panic\"\n").unwrap();
        assert!(load_config(Some(dir.path()), None, Some(&override_file)).is_err());
    }

    #[test]
    fn test_write_role_rejects_read_only_harness() {
        let dir = tempdir().unwrap();
        let override_file = dir.path().join("bad.toml");
        std::fs::write(
            &override_file,
            "[harnesses.ro]\nadapter = \"reasonix\"\nsupports_write = false\n[roles.implementer]\nchain = [ { harness = \"ro\" } ]\n",
        )
        .unwrap();
        assert!(load_config(Some(dir.path()), None, Some(&override_file)).is_err());
    }

    #[test]
    fn test_unknown_harness_in_chain_rejected() {
        let dir = tempdir().unwrap();
        let override_file = dir.path().join("bad.toml");
        std::fs::write(
            &override_file,
            "[roles.reviewer]\nchain = [ { harness = \"nope\" } ]\n",
        )
        .unwrap();
        assert!(load_config(Some(dir.path()), None, Some(&override_file)).is_err());
    }

    #[test]
    fn test_unknown_adapter_rejected() {
        let dir = tempdir().unwrap();
        let override_file = dir.path().join("bad.toml");
        std::fs::write(
            &override_file,
            "[harnesses.x]\nadapter = \"nope\"\nsupports_write = true\n",
        )
        .unwrap();
        assert!(load_config(Some(dir.path()), None, Some(&override_file)).is_err());
    }

    #[test]
    fn test_unknown_fallback_action_rejected() {
        let dir = tempdir().unwrap();
        let override_file = dir.path().join("bad.toml");
        std::fs::write(&override_file, "[fallback]\non_quota = \"panic\"\n").unwrap();
        assert!(load_config(Some(dir.path()), None, Some(&override_file)).is_err());
    }

    #[test]
    fn test_dump_toml_round_trip() {
        let defaults = builtin_defaults();
        let dumped = dump_toml(&defaults).unwrap();
        let reloaded: toml::Table = toml::from_str(&dumped).unwrap();
        assert_eq!(reloaded, defaults);
    }
}
