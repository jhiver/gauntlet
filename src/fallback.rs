//! Chain executor: retry policy + run-level circuit breaker.
//!
//! Every role chain implicitly ends with the "human" link. Failure -> policy
//! action mapping (DESIGN.md "fallback" section):
//!
//! - on_quota      next_and_break          open the harness breaker, next link
//! - on_auth       break                   open the breaker and abort the task
//! - on_rate_limit backoff_retry_then_next backoff 30s, 1 retry, then next link
//! - on_timeout    retry_once_then_next
//! - on_crash      retry_once_then_next    (PARTIAL_DELIVERY follows on_crash)
//! - on_invalid_output retry_once_then_next
//!
//! Beyond max_attempts_per_task: human checkpoint (if one is provided); in
//! non-interactive mode the chain is simply exhausted.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    #[serde(rename = "none")]
    None,
    #[serde(rename = "quota")]
    QuotaExhausted,
    #[serde(rename = "rate_limit")]
    RateLimited,
    #[serde(rename = "auth")]
    AuthExpired,
    #[serde(rename = "model_unavailable")]
    ModelUnavailable,
    #[serde(rename = "timeout_idle")]
    TimeoutIdle,
    #[serde(rename = "timeout_hard")]
    TimeoutHard,
    #[serde(rename = "crash")]
    Crash,
    #[serde(rename = "partial")]
    PartialDelivery,
    #[serde(rename = "invalid_output")]
    OutputInvalid,
}

impl FailureKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureKind::None => "none",
            FailureKind::QuotaExhausted => "quota",
            FailureKind::RateLimited => "rate_limit",
            FailureKind::AuthExpired => "auth",
            FailureKind::ModelUnavailable => "model_unavailable",
            FailureKind::TimeoutIdle => "timeout_idle",
            FailureKind::TimeoutHard => "timeout_hard",
            FailureKind::Crash => "crash",
            FailureKind::PartialDelivery => "partial",
            FailureKind::OutputInvalid => "invalid_output",
        }
    }

    pub fn policy_key(&self) -> &'static str {
        match self {
            FailureKind::None => "on_none",
            FailureKind::QuotaExhausted => "on_quota",
            FailureKind::AuthExpired => "on_auth",
            FailureKind::RateLimited => "on_rate_limit",
            FailureKind::ModelUnavailable => "on_model_unavailable",
            FailureKind::TimeoutIdle | FailureKind::TimeoutHard => "on_timeout",
            FailureKind::Crash | FailureKind::PartialDelivery => "on_crash",
            FailureKind::OutputInvalid => "on_invalid_output",
        }
    }
}

impl std::fmt::Display for FailureKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunResult {
    pub failure: FailureKind,
    pub exit_code: Option<i32>,
    pub output_path: PathBuf,
    #[serde(default)]
    pub detail: String,
}

impl RunResult {
    pub fn new(
        failure: FailureKind,
        exit_code: Option<i32>,
        output_path: impl Into<PathBuf>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            failure,
            exit_code,
            output_path: output_path.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainLink {
    pub harness: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl ChainLink {
    pub fn new(harness: impl Into<String>) -> Self {
        Self {
            harness: harness.into(),
            model: None,
            effort: None,
            extra: HashMap::new(),
        }
    }

    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    pub fn with_effort(mut self, effort: impl Into<String>) -> Self {
        self.effort = Some(effort.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainExhausted(pub String);

impl std::fmt::Display for ChainExhausted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for ChainExhausted {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthAbort(pub String);

impl std::fmt::Display for AuthAbort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for AuthAbort {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FallbackError {
    Exhausted(ChainExhausted),
    Auth(AuthAbort),
}

impl std::fmt::Display for FallbackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FallbackError::Exhausted(e) => write!(f, "{e}"),
            FallbackError::Auth(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FallbackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FallbackError::Exhausted(e) => Some(e),
            FallbackError::Auth(e) => Some(e),
        }
    }
}

impl From<ChainExhausted> for FallbackError {
    fn from(e: ChainExhausted) -> Self {
        FallbackError::Exhausted(e)
    }
}

impl From<AuthAbort> for FallbackError {
    fn from(e: AuthAbort) -> Self {
        FallbackError::Auth(e)
    }
}

/// Run-level circuit breakers (persisted in state.json, thread-safe).
#[derive(Debug, Default)]
pub struct HarnessHealth {
    states: Mutex<HashMap<String, String>>,
}

impl HarnessHealth {
    pub fn new(initial: Option<HashMap<String, String>>) -> Self {
        Self {
            states: Mutex::new(initial.unwrap_or_default()),
        }
    }

    pub fn is_open(&self, name: &str) -> bool {
        let guard = self.states.lock().unwrap_or_else(|p| p.into_inner());
        guard.get(name).map(|s| s == "open").unwrap_or(false)
    }

    pub fn open(&self, name: &str) {
        let mut guard = self.states.lock().unwrap_or_else(|p| p.into_inner());
        guard.insert(name.to_string(), "open".to_string());
    }

    pub fn close(&self, name: &str) {
        let mut guard = self.states.lock().unwrap_or_else(|p| p.into_inner());
        guard.insert(name.to_string(), "ok".to_string());
    }

    pub fn snapshot(&self) -> HashMap<String, String> {
        let guard = self.states.lock().unwrap_or_else(|p| p.into_inner());
        guard.clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FallbackPolicy {
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
    #[serde(default = "default_max_attempts")]
    pub max_attempts_per_task: usize,
    #[serde(default = "default_backoff_s")]
    pub backoff_s: f64,
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
fn default_max_attempts() -> usize {
    3
}
fn default_backoff_s() -> f64 {
    0.0
}

impl Default for FallbackPolicy {
    fn default() -> Self {
        Self {
            on_quota: default_on_quota(),
            on_auth: default_on_auth(),
            on_rate_limit: default_on_rate_limit(),
            on_model_unavailable: default_on_model_unavailable(),
            on_timeout: default_on_timeout(),
            on_crash: default_on_crash(),
            on_invalid_output: default_on_invalid_output(),
            max_attempts_per_task: default_max_attempts(),
            backoff_s: default_backoff_s(),
        }
    }
}

impl FallbackPolicy {
    pub fn get_action(&self, failure: FailureKind) -> &str {
        match failure {
            FailureKind::None => "none",
            FailureKind::QuotaExhausted => &self.on_quota,
            FailureKind::AuthExpired => &self.on_auth,
            FailureKind::RateLimited => &self.on_rate_limit,
            FailureKind::ModelUnavailable => &self.on_model_unavailable,
            FailureKind::TimeoutIdle | FailureKind::TimeoutHard => &self.on_timeout,
            FailureKind::Crash | FailureKind::PartialDelivery => &self.on_crash,
            FailureKind::OutputInvalid => &self.on_invalid_output,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainOutcome {
    pub result: RunResult,
    pub harness: String,
    pub attempts: usize,
}

pub fn short_text(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = collapsed.chars().collect();
    if chars.len() <= limit {
        collapsed
    } else {
        let truncated: String = chars[..limit.saturating_sub(1)].iter().collect();
        format!("{truncated}…")
    }
}

#[allow(clippy::too_many_arguments)]
pub fn execute_chain<F, V, C, L>(
    role: &str,
    links: &[ChainLink],
    health: &HarnessHealth,
    policy: &FallbackPolicy,
    mut run_once: F,
    mut validate: Option<V>,
    auto: bool,
    mut checkpoint: Option<C>,
    backoff_s: Option<f64>,
    mut log: Option<L>,
) -> Result<ChainOutcome, FallbackError>
where
    F: FnMut(&ChainLink, usize) -> RunResult,
    V: FnMut(&RunResult) -> (Option<FailureKind>, String),
    C: FnMut(&str) -> bool,
    L: FnMut(&str),
{
    let max_attempts = policy.max_attempts_per_task;
    let backoff = backoff_s.unwrap_or(policy.backoff_s);
    let mut chain: Vec<ChainLink> = links.to_vec();
    chain.push(ChainLink::new("human"));

    let mut attempts = 0;

    for link in &chain {
        let hname = &link.harness;
        if hname == "human" && auto {
            if let Some(ref mut l) = log {
                l(&format!("[{role}] skipping human link in --auto mode"));
            }
            continue;
        }

        if hname != "human" && health.is_open(hname) {
            if let Some(ref mut l) = log {
                l(&format!("[{role}] harness '{hname}' circuit breaker open; skipping"));
            }
            continue;
        }

        let mut rate_retried = false;
        let mut generic_retried = false;

        loop {
            if attempts >= max_attempts {
                let approved = if let Some(ref mut cp) = checkpoint {
                    cp(&format!(
                        "role '{role}': {attempts} attempts exhausted; approve to keep trying the remaining chain"
                    ))
                } else {
                    false
                };

                if approved {
                    attempts = 0;
                } else {
                    return Err(FallbackError::Exhausted(ChainExhausted(format!(
                        "{role}: exhausted after {attempts} attempts"
                    ))));
                }
            }

            attempts += 1;
            let mut result = run_once(link, attempts);

            if result.failure == FailureKind::None {
                if let Some(ref mut v) = validate {
                    let (vfailure, detail) = v(&result);
                    if let Some(vf) = vfailure {
                        result = RunResult::new(
                            vf,
                            result.exit_code,
                            result.output_path,
                            detail,
                        );
                    }
                }
            }

            if result.failure == FailureKind::None {
                if hname != "human" {
                    health.close(hname);
                }
                return Ok(ChainOutcome {
                    result,
                    harness: hname.clone(),
                    attempts,
                });
            }

            if let Some(ref mut l) = log {
                l(&format!(
                    "[{role}] {hname} attempt {attempts}: {} — {}",
                    result.failure.as_str(),
                    short_text(&result.detail, 120)
                ));
            }

            let action = policy.get_action(result.failure);

            if action == "break" {
                if hname != "human" {
                    health.open(hname);
                }
                return Err(FallbackError::Auth(AuthAbort(format!(
                    "{role}/{hname}: {}: {}",
                    result.failure.as_str(),
                    short_text(&result.detail, 120)
                ))));
            }

            if action == "next_and_break" {
                if hname != "human" {
                    health.open(hname);
                }
                break;
            }

            if action == "backoff_retry_then_next" && !rate_retried {
                rate_retried = true;
                if backoff > 0.0 {
                    std::thread::sleep(Duration::from_secs_f64(backoff));
                }
                continue;
            }

            if action == "retry_once_then_next" && !generic_retried {
                generic_retried = true;
                continue;
            }

            break;
        }
    }

    Err(FallbackError::Exhausted(ChainExhausted(format!(
        "{role}: chain exhausted"
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct MockAdapter {
        script: Mutex<Vec<FailureKind>>,
        calls: AtomicUsize,
    }

    impl MockAdapter {
        fn new(script: Vec<FailureKind>) -> Self {
            Self {
                script: Mutex::new(script),
                calls: AtomicUsize::new(0),
            }
        }

        fn run(&self, out_dir: &Path) -> RunResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let kind = {
                let mut guard = self.script.lock().unwrap();
                if !guard.is_empty() {
                    guard.remove(0)
                } else {
                    FailureKind::None
                }
            };
            let code = if kind == FailureKind::None { Some(0) } else { Some(1) };
            RunResult::new(
                kind,
                code,
                out_dir.join("test.out"),
                format!("scripted {}", kind.as_str()),
            )
        }
    }

    fn test_policy() -> FallbackPolicy {
        FallbackPolicy {
            on_quota: "next_and_break".to_string(),
            on_auth: "break".to_string(),
            on_rate_limit: "backoff_retry_then_next".to_string(),
            on_model_unavailable: "next".to_string(),
            on_timeout: "retry_once_then_next".to_string(),
            on_crash: "retry_once_then_next".to_string(),
            on_invalid_output: "retry_once_then_next".to_string(),
            max_attempts_per_task: 3,
            backoff_s: 0.0,
        }
    }

    #[test]
    fn test_quota_opens_breaker_and_moves_to_next_link() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::QuotaExhausted]));
        let adapter_b = Arc::new(MockAdapter::new(vec![]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let b_clone = adapter_b.clone();

        let run_once = move |link: &ChainLink, _attempt: usize| {
            if link.harness == "a" {
                a_clone.run(&out_dir)
            } else {
                b_clone.run(&out_dir)
            }
        };

        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            run_once,
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "b");
        assert!(health.is_open("a"));
        assert_eq!(adapter_a.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_open_breaker_skips_harness_on_later_task() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::QuotaExhausted]));
        let adapter_b = Arc::new(MockAdapter::new(vec![]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir1 = std::env::temp_dir();
        let out_dir2 = std::env::temp_dir();

        let a1 = adapter_a.clone();
        let b1 = adapter_b.clone();
        let _ = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a1.run(&out_dir1) } else { b1.run(&out_dir1) },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        let a2 = adapter_a.clone();
        let b2 = adapter_b.clone();
        let _ = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a2.run(&out_dir2) } else { b2.run(&out_dir2) },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(adapter_a.calls.load(Ordering::SeqCst), 1);
        assert_eq!(adapter_b.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_success_closes_breaker() {
        let mut initial = HashMap::new();
        initial.insert("a".to_string(), "open".to_string());
        let health = HarnessHealth::new(Some(initial));
        let adapter_b = Arc::new(MockAdapter::new(vec![FailureKind::None]));
        let policy = test_policy();
        let links = vec![ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let b_clone = adapter_b.clone();
        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "b" { b_clone.run(&out_dir) } else { RunResult::new(FailureKind::Crash, None, out_dir.clone(), "") },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "b");
        assert!(!health.is_open("b"));
    }

    #[test]
    fn test_auth_aborts_without_draining_chain() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::AuthExpired]));
        let adapter_b = Arc::new(MockAdapter::new(vec![]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let b_clone = adapter_b.clone();

        let err = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a_clone.run(&out_dir) } else { b_clone.run(&out_dir) },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap_err();

        match err {
            FallbackError::Auth(_) => {}
            _ => panic!("expected AuthAbort"),
        }
        assert_eq!(adapter_b.calls.load(Ordering::SeqCst), 0);
        assert!(health.is_open("a"));
    }

    #[test]
    fn test_timeout_retries_once_on_same_link() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::TimeoutHard, FailureKind::None]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |_, _| a_clone.run(&out_dir),
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "a");
        assert_eq!(adapter_a.calls.load(Ordering::SeqCst), 2);
        assert_eq!(outcome.attempts, 2);
    }

    #[test]
    fn test_timeout_then_next_link_after_one_retry() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::TimeoutIdle, FailureKind::TimeoutHard]));
        let adapter_b = Arc::new(MockAdapter::new(vec![]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let b_clone = adapter_b.clone();

        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a_clone.run(&out_dir) } else { b_clone.run(&out_dir) },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "b");
        assert_eq!(adapter_a.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_rate_limit_backoff_retry_then_next() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::RateLimited, FailureKind::None]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |_, _| a_clone.run(&out_dir),
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "a");
        assert_eq!(adapter_a.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_invalid_output_retries_once_then_next() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::None]));
        let adapter_b = Arc::new(MockAdapter::new(vec![]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let b_clone = adapter_b.clone();
        let vcalls = Arc::new(AtomicUsize::new(0));
        let vcalls_clone = vcalls.clone();

        let validate = move |_: &RunResult| {
            let count = vcalls_clone.fetch_add(1, Ordering::SeqCst) + 1;
            if count <= 2 {
                (Some(FailureKind::OutputInvalid), "bad block".to_string())
            } else {
                (None, String::new())
            }
        };

        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a_clone.run(&out_dir) } else { b_clone.run(&out_dir) },
            Some(validate),
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "b");
        assert_eq!(adapter_a.calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_auto_mode_skips_human_link() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::Crash, FailureKind::Crash]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let err = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |_, _| a_clone.run(&out_dir),
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap_err();

        match err {
            FallbackError::Exhausted(_) => {}
            _ => panic!("expected ChainExhausted"),
        }
    }

    #[test]
    fn test_max_attempts_triggers_chain_exhaustion() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::Crash, FailureKind::Crash]));
        let adapter_b = Arc::new(MockAdapter::new(vec![FailureKind::Crash]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let b_clone = adapter_b.clone();

        let err = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a_clone.run(&out_dir) } else { b_clone.run(&out_dir) },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap_err();

        match err {
            FallbackError::Exhausted(_) => {}
            _ => panic!("expected ChainExhausted"),
        }
        assert_eq!(adapter_b.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn test_checkpoint_approval_resets_attempt_budget() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::Crash, FailureKind::Crash]));
        let adapter_b = Arc::new(MockAdapter::new(vec![]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let b_clone = adapter_b.clone();

        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a_clone.run(&out_dir) } else { b_clone.run(&out_dir) },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            Some(|_msg: &str| true),
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "b");
    }

    #[test]
    fn test_model_unavailable_goes_next_without_breaker_or_retry() {
        let adapter_a = Arc::new(MockAdapter::new(vec![FailureKind::ModelUnavailable]));
        let adapter_b = Arc::new(MockAdapter::new(vec![]));
        let health = HarnessHealth::default();
        let policy = test_policy();
        let links = vec![ChainLink::new("a"), ChainLink::new("b")];
        let out_dir = std::env::temp_dir();

        let a_clone = adapter_a.clone();
        let b_clone = adapter_b.clone();

        let outcome = execute_chain(
            "tester",
            &links,
            &health,
            &policy,
            move |link, _| if link.harness == "a" { a_clone.run(&out_dir) } else { b_clone.run(&out_dir) },
            None::<fn(&RunResult) -> (Option<FailureKind>, String)>,
            true,
            None::<fn(&str) -> bool>,
            Some(0.0),
            None::<fn(&str)>,
        )
        .unwrap();

        assert_eq!(outcome.harness, "b");
        assert_eq!(adapter_a.calls.load(Ordering::SeqCst), 1);
        assert!(!health.is_open("a"));
    }
}
