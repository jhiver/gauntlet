//! Drives the Gauntlet loop (threads for parallel lanes).
//!
//! The orchestrator is a deterministic state machine; no LLM sits in the
//! control loop. Phases (DESIGN.md "State machine"):
//!
//! INIT -> PLAN -> [checkpoint: plan] -> IMPLEMENT(wave=0) -> INSPECT
//! -> INTEGRATE -> GATES -> REVIEW -> JUDGE
//! -> blocking groups: PLAN_FIX -> IMPLEMENT(wave+=1) -> ... -> JUDGE
//! -> none blocking: POLISH -> [checkpoint: deliver] -> DELIVER -> READY
//! Terminals: READY | READY_NO_CHANGE | BLOCKED* (a blocked terminal always
//! means "a human decision is required", and its kind says which one).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::adapters::base::{FailureKind, HarnessAdapter, RunResult};
use crate::adapters::echo::EchoAdapter;
use crate::adapters::human::read_decision;
use crate::adapters::{create_adapter, AdapterConfig, ErrorPatternsConfig};
use crate::autoroute::{analyze_mission, MissionProfile};
use crate::capsules::{self, ClaimGroupLike, LaneLike, MissionLike};
use crate::config::{dump_toml, load_config_table, validate_config, Config, ConfigError};
use crate::fallback::{FallbackPolicy, HarnessHealth};
use crate::gates::{run_gates, GateResult};
use crate::mission::{create_stage_mission, load_mission, Lane, Mission, MissionError, StageSpec};
use crate::report::Report;
use crate::statemachine::{
    convergence_state, load, save, LaneState, State, StatemachineError, BLOCKED_TERMINALS, CAPPED,
    LANE_ACTIVE, STALLED, TERMINALS,
};
use crate::ui::default_ui;
use crate::verdicts::{
    extract_block_from_file, extract_planner_result, validate_report, validate_verdict,
    ClaimGroup, PlanLane, PlannerResult, VerdictError,
};
use crate::worktrees::{
    base_commit, branch_exists, check_claimed_vs_diff, check_lane_diff, checkout_drift,
    checkout_status, commit_all, create_worktree, delete_branch, discard_changes, ff_merge,
    find_overlaps, find_worktree_for_branch, globs_may_overlap, is_git_repo, lane_changed_files, merge_branch,
    rebase_onto, remove_worktree, rev_parse, staged_changes, tracked_files, Git, GitError,
    LaneOverlap,
};

impl MissionLike for Mission {
    fn body(&self) -> &str {
        &self.body
    }
    fn contract_ids(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        Box::new(self.contract_ids.iter().map(|s| s.as_str()))
    }
}

impl LaneLike for Lane {
    fn id(&self) -> &str {
        &self.id
    }
    fn owns(&self) -> &[String] {
        &self.owns
    }
    fn forbidden(&self) -> &[String] {
        &self.forbidden
    }
    fn tests(&self) -> &[String] {
        &self.tests
    }
    fn brief(&self) -> &str {
        &self.brief
    }
    fn addresses(&self) -> &[String] {
        &self.addresses
    }
}

impl LaneLike for LaneState {
    fn id(&self) -> &str {
        &self.id
    }
    fn owns(&self) -> &[String] {
        &self.owns
    }
    fn forbidden(&self) -> &[String] {
        &self.forbidden
    }
    fn tests(&self) -> &[String] {
        &self.tests
    }
    fn brief(&self) -> &str {
        &self.brief
    }
    fn addresses(&self) -> &[String] {
        &self.addresses
    }
}

impl ClaimGroupLike for ClaimGroup {
    fn root_cause(&self) -> &str {
        &self.root_cause
    }
    fn fix(&self) -> &str {
        &self.fix
    }
    fn verdict(&self) -> &str {
        &self.verdict
    }
    fn owns(&self) -> &str {
        &self.owns
    }
    fn defect_class(&self) -> &str {
        &self.defect_class
    }
}

#[derive(Debug, Clone)]
pub struct RoleOutcome {
    pub result: RunResult,
    pub harness: String,
    pub attempts: usize,
}

#[derive(Debug, Clone)]
pub struct GauntletError {
    pub message: String,
    pub kind: String,
}

impl GauntletError {
    pub fn new(message: impl Into<String>, kind: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            kind: kind.into(),
        }
    }

    pub fn blocked(message: impl Into<String>) -> Self {
        Self::new(message, "BLOCKED")
    }
}

impl std::fmt::Display for GauntletError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for GauntletError {}

impl From<GitError> for GauntletError {
    fn from(e: GitError) -> Self {
        GauntletError::blocked(e.to_string())
    }
}

impl From<StatemachineError> for GauntletError {
    fn from(e: StatemachineError) -> Self {
        GauntletError::blocked(e.to_string())
    }
}

impl From<MissionError> for GauntletError {
    fn from(e: MissionError) -> Self {
        GauntletError::blocked(e.to_string())
    }
}

impl From<ConfigError> for GauntletError {
    fn from(e: ConfigError) -> Self {
        GauntletError::blocked(e.to_string())
    }
}

impl From<VerdictError> for GauntletError {
    fn from(e: VerdictError) -> Self {
        GauntletError::blocked(e.to_string())
    }
}

pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Orchestrator {
    pub tool_dir: PathBuf,
    pub auto: bool,
    pub dry_run: bool,
    pub depth: usize,
    pub max_depth: usize,
    pub log_fn: Option<LogFn>,
    state_lock: Arc<Mutex<()>>,
    pub git: Git,
    pub report: Option<Report>,
    pub profile_info: Option<MissionProfile>,
    pub mission: Mission,
    pub config: Config,
    pub state: State,
    pub run_dir: Option<PathBuf>,
    pub health: Arc<HarnessHealth>,
    pub adapters: HashMap<String, Box<dyn HarnessAdapter>>,
    pub echo: EchoAdapter,
    main_before: Vec<String>,
    pending_groups: Vec<ClaimGroup>,
}

impl Orchestrator {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tool_dir: &Path,
        mission_path: Option<&Path>,
        resume_dir: Option<&Path>,
        config_path: Option<&Path>,
        auto: bool,
        dry_run: bool,
        profile: Option<&str>,
        replan: bool,
        depth: usize,
        max_depth: usize,
        log_fn: Option<LogFn>,
    ) -> Result<Self, GauntletError> {
        let tool_dir = tool_dir.to_path_buf();
        let state_lock = Arc::new(Mutex::new(()));
        let git = if let Some(ref l) = log_fn {
            let l_clone = Arc::clone(l);
            Git::with_logger(dry_run, move |msg| l_clone(msg))
        } else {
            Git::new(dry_run)
        };

        let (mission, config, state, run_dir, report, profile_info) = if let Some(r_dir) = resume_dir {
            let run_dir = r_dir.to_path_buf();
            let state = load(&run_dir).map_err(|e| GauntletError::blocked(e.to_string()))?;
            let mission = load_mission(&run_dir.join("mission.md"))
                .map_err(|e| GauntletError::blocked(e.to_string()))?;
            let mut cfg_table = load_config_table(None, None, Some(&run_dir.join("config.toml")))?;
            if let Some(cp) = config_path {
                let extra_table = load_config_table(None, None, Some(cp))?;
                crate::config::merge(&mut cfg_table, &extra_table);
                validate_config(&cfg_table)?;
            }
            let config = Config::from_table(cfg_table)?;
            let report = Report::new(run_dir.join("report.md"), None).ok();
            (mission, config, state, Some(run_dir), report, None)
        } else {
            let m_path = mission_path.ok_or_else(|| {
                GauntletError::blocked("MISSION.md is required unless --resume is given")
            })?;
            let mut mission = load_mission(m_path).map_err(|e| GauntletError::blocked(e.to_string()))?;
            if replan {
                mission.lanes = Vec::new();
            }
            let profile_info = analyze_mission(&mission);
            let mission_dir = m_path.parent();
            let mut cfg_table = load_config_table(Some(&tool_dir), mission_dir, config_path)?;

            // Super-Auto: apply intelligent Pareto routing if requested or if standard defaults are loaded (non-test echo)
            let is_echo_test = cfg_table
                .get("roles")
                .and_then(|v| v.as_table())
                .map(|roles| {
                    roles.values().all(|r| {
                        r.get("chain")
                            .and_then(|v| v.as_array())
                            .map(|chain| {
                                chain.iter().all(|link| {
                                    link.get("harness")
                                        .and_then(|h| h.as_str())
                                        .map(|h| h == "echo")
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            let has_custom_config = config_path.is_some()
                || mission_dir
                    .map(|d| d.join("gauntlet.toml").is_file())
                    .unwrap_or(false);

            if (profile.is_some() || !has_custom_config) && !is_echo_test {
                if let Ok(toml::Value::Table(roles_table)) = toml::Value::try_from(&profile_info.roles) {
                    let roles_entry = cfg_table
                        .entry("roles".to_string())
                        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
                    if let toml::Value::Table(roles_dest) = roles_entry {
                        crate::config::merge(roles_dest, &roles_table);
                    }
                }
            }

            let config = Config::from_table(cfg_table)?;
            let repo = mission
                .repos
                .first()
                .ok_or_else(|| GauntletError::blocked("mission has no repos defined".to_string()))?;
            let state = State {
                slug: mission.slug.clone(),
                repo: PathBuf::from(&repo.path)
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from(&repo.path))
                    .to_string_lossy()
                    .to_string(),
                target_branch: repo.target_branch.clone(),
                gates: repo.gates.clone(),
                auto,
                dry_run,
                ..Default::default()
            };
            (mission, config, state, None, None, Some(profile_info))
        };

        let health = Arc::new(HarnessHealth::new(Some(state.harness_health.clone())));

        let mut adapters: HashMap<String, Box<dyn HarnessAdapter>> = HashMap::new();
        for (hname, hcfg) in &config.harnesses {
            let error_pats = hcfg.errors.as_ref().map(|errs| {
                let mut epc = ErrorPatternsConfig::default();
                if let Some(q) = errs.get("quota") {
                    epc.quota = Some(q.clone());
                }
                if let Some(a) = errs.get("auth") {
                    epc.auth = Some(a.clone());
                }
                if let Some(r) = errs.get("rate_limit") {
                    epc.rate_limit = Some(r.clone());
                }
                if let Some(m) = errs.get("model_unavailable") {
                    epc.model_unavailable = Some(m.clone());
                }
                epc
            });

            let adapter_cfg = AdapterConfig {
                adapter: Some(hcfg.adapter.clone()),
                supports_write: Some(hcfg.supports_write),
                default_model: hcfg.default_model.clone(),
                launcher: None,
                errors: error_pats,
            };

            if let Ok(ad) = create_adapter(&hcfg.adapter, hname, Some(&adapter_cfg)) {
                adapters.insert(hname.clone(), ad);
            }
        }

        let echo_cfg = AdapterConfig {
            adapter: Some("echo".to_string()),
            supports_write: Some(true),
            default_model: None,
            launcher: None,
            errors: None,
        };
        let echo = EchoAdapter::new("echo", Some(&echo_cfg));

        Ok(Self {
            tool_dir,
            auto,
            dry_run,
            depth,
            max_depth,
            log_fn,
            state_lock,
            git,
            report,
            profile_info,
            mission,
            config,
            state,
            run_dir,
            health,
            adapters,
            echo,
            main_before: Vec::new(),
            pending_groups: Vec::new(),
        })
    }

    pub fn log(&self, message: &str) {
        if let Some(ref l) = self.log_fn {
            l(message);
        } else {
            println!("{message}");
        }
    }

    pub fn repo_path(&self) -> PathBuf {
        PathBuf::from(&self.state.repo)
    }

    pub fn integration_wt(&self) -> PathBuf {
        PathBuf::from(format!(
            "{}-worktree-gauntlet-{}",
            self.state.repo, self.state.run_id
        ))
    }

    pub fn lane_wt(&self, lane_id: &str) -> PathBuf {
        PathBuf::from(format!(
            "{}-worktree-gauntlet-{}-{}",
            self.state.repo, self.state.run_id, lane_id
        ))
    }

    pub fn integration_branch(&self) -> String {
        format!("gauntlet/{}/integration", self.state.run_id)
    }

    pub fn lane_branch(&self, lane_id: &str) -> String {
        format!("gauntlet/{}/{}", self.state.run_id, lane_id)
    }

    pub fn work_wt(&self) -> PathBuf {
        let wt = self.integration_wt();
        if wt.is_dir() {
            wt
        } else {
            self.repo_path()
        }
    }

    pub fn _save(&mut self) -> Result<(), GauntletError> {
        let _guard = self.state_lock.lock().unwrap_or_else(|p| p.into_inner());
        self.state.harness_health = self.health.snapshot();
        if self.state.run_dir.is_some() {
            save(&self.state).map_err(|e| GauntletError::blocked(e.to_string()))?;
        }
        Ok(())
    }

    pub fn _transition(&mut self, phase: &str) -> Result<(), GauntletError> {
        self.state.phase = phase.to_string();
        self._save()?;
        if let Ok(mut ui) = default_ui().lock() {
            ui.phase_card(phase, self.state.wave, None, 76);
        }
        self.log(&format!("phase -> {phase}"));
        Ok(())
    }

    pub fn _diagnosis(&self) -> String {
        let mut lines = vec![
            "### Diagnosis".to_string(),
            "".to_string(),
            format!(
                "- phase at failure: {}",
                self.state.blocked_phase.as_deref().unwrap_or("unknown")
            ),
            format!(
                "- wave: {} of {} max",
                self.state.wave, self.config.policy.max_total_waves
            ),
        ];

        if !self.state.blocking_history.is_empty() {
            let hist_str = self
                .state
                .blocking_history
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(" → ");
            lines.push(format!("- blocking-group trajectory: {hist_str}"));
        }

        for group in self._last_judgment_groups() {
            if group.blocking() {
                let owns_part = if !group.owns.is_empty() {
                    format!(" owns={}", group.owns)
                } else {
                    String::new()
                };
                lines.push(format!(
                    "- remaining: {} [{}, {}]{}",
                    group.root_cause, group.verdict, group.defect_class, owns_part
                ));
            }
        }

        if !self.state.gates.is_empty() {
            lines.push(format!("- gates: {}", self.state.gates.join("; ")));
        }

        let snapshot = self.health.snapshot();
        if !snapshot.is_empty() {
            lines.push(format!("- harness health: {snapshot:?}"));
        }

        if !self.state.branches.is_empty() {
            lines.push(format!(
                "- candidate preserved on branch {} (worktrees kept)",
                self.integration_branch()
            ));
        }

        let next_msg = match self.state.blocked_kind.as_deref() {
            Some("BLOCKED_CONVERGENCE") => {
                "fix waves stopped reducing the defect count: re-scope the contract, or fix the remaining groups by hand and --resume."
            }
            Some("BLOCKED_ARCHITECTURE") => {
                "the judge accepted a REDESIGN group — no proportionate local fix exists. A human redesign decision is required."
            }
            Some("BLOCKED_GATE") => {
                "a required gate failed on the candidate; see outputs/gate-*.log, fix the cause, then --resume."
            }
            Some("BLOCKED_HARNESS") => {
                "no harness in the role chain could deliver (quota, auth, or timeout). Restore access or edit the chain, then --resume."
            }
            _ => "the run hit a condition it must not decide alone; see the reason above.",
        };

        lines.push("".to_string());
        lines.push(format!("Next: {next_msg}"));
        lines.join("\n")
    }

    pub fn _blocked(&mut self, reason: &str, kind: &str) -> Result<(), GauntletError> {
        self.log(&format!("{kind}: {reason}"));
        if let Ok(mut ui) = default_ui().lock() {
            ui.error(&format!("{kind}: {reason}"), "");
        }
        self.state.blocked_reason = Some(reason.to_string());
        self.state.blocked_kind = Some(kind.to_string());
        self.state.blocked_phase = Some(self.state.phase.clone());
        self.state.phase = kind.to_string();

        if let Some(ref report) = self.report {
            let _ = report.section(kind, &format!("{reason}\n\n{}", self._diagnosis()));
        }
        self._save()
    }

    pub fn _write_capsule(&self, name: &str, text: &str) -> Result<PathBuf, GauntletError> {
        let run_dir = self.run_dir.as_ref().ok_or_else(|| {
            GauntletError::blocked("run_dir is not set")
        })?;
        let path = run_dir.join("capsules").join(format!("{name}.md"));
        if let Some(p) = path.parent() {
            let _ = std::fs::create_dir_all(p);
        }
        std::fs::write(&path, text).map_err(|e| {
            GauntletError::blocked(format!("cannot write capsule {}: {e}", path.display()))
        })?;
        Ok(path)
    }

    fn slot<T: Clone>(items: &[T], index: usize, value: T) -> Vec<T> {
        let mut out = if index < items.len() {
            items[..index].to_vec()
        } else {
            items.to_vec()
        };
        out.push(value);
        out
    }

    pub fn _judgment_groups(&self, path: impl AsRef<Path>) -> Vec<ClaimGroup> {
        let path = path.as_ref();
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.log(&format!("warning: unreadable judgment {}: {e}", path.display()));
                return Vec::new();
            }
        };
        let text = String::from_utf8_lossy(&bytes);
        let val: serde_json::Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(e) => {
                self.log(&format!("warning: unreadable judgment JSON {}: {e}", path.display()));
                return Vec::new();
            }
        };
        match validate_verdict(&val, &self.mission.contract_ids) {
            Ok(g) => g,
            Err(e) => {
                self.log(&format!("warning: unreadable judgment schema {}: {e}", path.display()));
                Vec::new()
            }
        }
    }

    pub fn _last_judgment_groups(&self) -> Vec<ClaimGroup> {
        if self.state.judgments.is_empty() {
            return Vec::new();
        }
        self._judgment_groups(&self.state.judgments[self.state.judgments.len() - 1])
    }

    pub fn _prior_findings(&self) -> (Vec<String>, Vec<String>, Vec<String>) {
        let mut fixed = Vec::new();
        let mut deferred = Vec::new();
        let mut dismissed = Vec::new();

        for (wave, path) in self.state.judgments.iter().enumerate() {
            for group in self._judgment_groups(path) {
                let entry = format!("[wave {wave}] {}", group.root_cause);
                if group.blocking() {
                    fixed.push(entry);
                } else if group.polish() {
                    deferred.push(format!("{entry} [{}]", group.defect_class));
                } else {
                    let mut reason = format!(" — {}", group.verdict);
                    if !group.fix.is_empty() {
                        reason.push_str(&format!(": {}", group.fix));
                    }
                    dismissed.push(format!("{entry}{reason}"));
                }
            }
        }
        (fixed, deferred, dismissed)
    }

    pub fn _validate_report(&self, result: &RunResult) -> (Option<FailureKind>, String) {
        match extract_block_from_file(&result.output_path, "report") {
            Ok(val) => match validate_report(&val) {
                Ok(rep) => {
                    if rep.partial {
                        (Some(FailureKind::PartialDelivery), "worker declared partial delivery".to_string())
                    } else {
                        (None, String::new())
                    }
                }
                Err(e) => (Some(FailureKind::OutputInvalid), e.to_string()),
            },
            Err(e) => (Some(FailureKind::OutputInvalid), e.to_string()),
        }
    }

    pub fn _validate_verdict(&self, result: &RunResult) -> (Option<FailureKind>, String) {
        match extract_block_from_file(&result.output_path, "verdict") {
            Ok(val) => match validate_verdict(&val, &self.mission.contract_ids) {
                Ok(_) => (None, String::new()),
                Err(e) => (Some(FailureKind::OutputInvalid), e.to_string()),
            },
            Err(e) => (Some(FailureKind::OutputInvalid), e.to_string()),
        }
    }

    pub fn _validate_plan(&self, result: &RunResult) -> (Option<FailureKind>, String) {
        let bytes = match std::fs::read(&result.output_path) {
            Ok(b) => b,
            Err(e) => return (Some(FailureKind::OutputInvalid), e.to_string()),
        };
        let text = String::from_utf8_lossy(&bytes);
        match extract_planner_result(&text, Some(&self.mission.contract_ids)) {
            Ok(_) => (None, String::new()),
            Err(e) => (Some(FailureKind::OutputInvalid), e.to_string()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_harness(
        &mut self,
        link: &crate::config::ChainLink,
        capsule: &Path,
        worktree: &Path,
        write: bool,
        role: &str,
        lane_id: Option<&str>,
        hard_s: u64,
        idle_s: Option<u64>,
        out_dir: &Path,
    ) -> RunResult {
        let hname = &link.harness;
        let model = link.model.as_deref();
        let effort = link.effort.as_deref();

        if self.dry_run && hname != "echo" && hname != "human" {
            self.log(&format!("DRY-RUN: would run harness: {hname}"));
            return self.echo.run(
                capsule,
                worktree,
                write,
                model,
                effort,
                hard_s,
                idle_s,
                out_dir,
                role,
                lane_id,
            );
        }

        if let Some(adapter) = self.adapters.get_mut(hname) {
            adapter.run(
                capsule,
                worktree,
                write,
                model,
                effort,
                hard_s,
                idle_s,
                out_dir,
                role,
                lane_id,
            )
        } else if hname == "echo" {
            self.echo.run(
                capsule,
                worktree,
                write,
                model,
                effort,
                hard_s,
                idle_s,
                out_dir,
                role,
                lane_id,
            )
        } else {
            RunResult {
                failure: FailureKind::Crash,
                exit_code: None,
                output_path: out_dir.join("error.out"),
                detail: format!("unknown harness '{hname}'"),
            }
        }
    }

    pub fn _run_role(
        &mut self,
        role: &str,
        capsule: &Path,
        worktree: &Path,
        write: bool,
        lane_id: Option<&str>,
    ) -> Result<RoleOutcome, GauntletError> {
        let links = self
            .config
            .roles
            .get(role)
            .map(|r| r.chain.clone())
            .unwrap_or_default();

        let policy = &self.config.policy;
        let is_write_role = role == "implementer" || role == "fixer";
        let (hard_s, idle_s) = if is_write_role {
            (policy.lane_timeout_s, None)
        } else {
            (policy.hard_timeout_s, Some(policy.idle_timeout_s))
        };

        let run_dir = self.run_dir.clone().unwrap_or_else(|| PathBuf::from("."));
        let out_dir = run_dir.join("outputs");

        let fallback_policy = FallbackPolicy {
            on_quota: self.config.fallback.on_quota.clone(),
            on_auth: self.config.fallback.on_auth.clone(),
            on_rate_limit: self.config.fallback.on_rate_limit.clone(),
            on_model_unavailable: self.config.fallback.on_model_unavailable.clone(),
            on_timeout: self.config.fallback.on_timeout.clone(),
            on_crash: self.config.fallback.on_crash.clone(),
            on_invalid_output: self.config.fallback.on_invalid_output.clone(),
            max_attempts_per_task: self.config.fallback.max_attempts_per_task,
            backoff_s: 0.0,
        };

        let mut attempts = 0;
        let mut chain = links.clone();
        chain.push(crate::config::ChainLink {
            harness: "human".to_string(),
            model: None,
            effort: None,
            extra: HashMap::new(),
        });

        for link in &chain {
            let hname = &link.harness;
            if hname == "human" && self.auto {
                self.log(&format!("[{role}] skipping human link in --auto mode"));
                continue;
            }
            if hname != "human" && self.health.is_open(hname) {
                self.log(&format!("[{role}] harness '{hname}' circuit breaker open; skipping"));
                continue;
            }

            let mut rate_retried = false;
            let mut generic_retried = false;

            loop {
                if attempts >= fallback_policy.max_attempts_per_task {
                    if !self.auto && self._ask_human("attempts", &format!("role '{role}': {attempts} attempts exhausted; approve to keep trying the remaining chain")) {
                        attempts = 0;
                    } else {
                        return Err(GauntletError::new(
                            format!("{role}: exhausted after {attempts} attempts"),
                            "BLOCKED_HARNESS",
                        ));
                    }
                }

                attempts += 1;
                let mut result = self.execute_harness(
                    link,
                    capsule,
                    worktree,
                    write,
                    role,
                    lane_id,
                    hard_s,
                    idle_s,
                    &out_dir,
                );

                if result.failure == FailureKind::None {
                    let (vfailure, vdetail) = match role {
                        "implementer" | "fixer" => self._validate_report(&result),
                        "reviewer" | "judge" => self._validate_verdict(&result),
                        "planner" => self._validate_plan(&result),
                        _ => (None, String::new()),
                    };
                    if let Some(vf) = vfailure {
                        result.failure = vf;
                        result.detail = vdetail;
                    }
                }

                if result.failure == FailureKind::None {
                    if hname != "human" {
                        self.health.close(hname);
                    }
                    return Ok(RoleOutcome {
                        result,
                        harness: hname.clone(),
                        attempts,
                    });
                }

                self.log(&format!(
                    "[{role}] {hname} attempt {attempts}: {} — {}",
                    result.failure.as_str(),
                    result.detail
                ));

                let action = match result.failure {
                    FailureKind::QuotaExhausted => &fallback_policy.on_quota,
                    FailureKind::AuthExpired => &fallback_policy.on_auth,
                    FailureKind::RateLimited => &fallback_policy.on_rate_limit,
                    FailureKind::ModelUnavailable => &fallback_policy.on_model_unavailable,
                    FailureKind::TimeoutIdle | FailureKind::TimeoutHard => &fallback_policy.on_timeout,
                    FailureKind::Crash | FailureKind::PartialDelivery => &fallback_policy.on_crash,
                    FailureKind::OutputInvalid => &fallback_policy.on_invalid_output,
                    FailureKind::None => "none",
                };

                if action == "break" {
                    if hname != "human" {
                        self.health.open(hname);
                    }
                    return Err(GauntletError::new(
                        format!("{role}/{hname}: {}: {}", result.failure.as_str(), result.detail),
                        "BLOCKED_HARNESS",
                    ));
                }

                if action == "next_and_break" {
                    if hname != "human" {
                        self.health.open(hname);
                    }
                    break; // next link
                }

                if action == "backoff_retry_then_next" && !rate_retried {
                    rate_retried = true;
                    continue;
                }

                if action == "retry_once_then_next" && !generic_retried {
                    generic_retried = true;
                    continue;
                }

                break; // next link
            }
        }

        Err(GauntletError::new(
            format!("{role}: chain exhausted"),
            "BLOCKED_HARNESS",
        ))
    }

    pub fn _ask_human(&mut self, name: &str, context: &str) -> bool {
        if self.auto {
            return false;
        }
        let capsule_text = capsules::checkpoint(name, context);
        let capsule_path = match self._write_capsule(&format!("checkpoint-{name}"), &capsule_text) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let outcome = match self._run_role("director", &capsule_path, &self.work_wt(), false, None) {
            Ok(o) => o,
            Err(e) => {
                self.log(&format!("director consult failed: {e}"));
                return false;
            }
        };
        let decision = read_decision(&outcome.result.output_path);
        let decision_str = decision.as_deref().unwrap_or("none");
        self.log(&format!("director decision on '{name}': {decision_str}"));
        decision.as_deref() == Some("approve")
    }

    pub fn _checkpoint(&mut self, name: &str, context: &str) -> Result<bool, GauntletError> {
        if !self.config.policy.checkpoints.iter().any(|c| c == name) {
            return Ok(true);
        }
        if self.auto {
            self.log(&format!("checkpoint '{name}': auto-approved (--auto)"));
            return Ok(true);
        }
        Ok(self._ask_human(name, context))
    }

    pub fn _run_planner(
        &mut self,
        groups: Option<&[ClaimGroup]>,
        complaint: Option<&str>,
    ) -> Result<PlannerResult, GauntletError> {
        let capsule_text = capsules::planner(
            &self.mission,
            &self.state.run_id,
            groups,
            complaint,
        );
        let capsule_path = self._write_capsule(
            &format!("planner-w{}", self.state.wave),
            &capsule_text,
        )?;
        let outcome = self._run_role("planner", &capsule_path, &self.work_wt(), false, None)?;
        let bytes = std::fs::read(&outcome.result.output_path).map_err(|e| {
            GauntletError::blocked(format!("cannot read planner output: {e}"))
        })?;
        let text = String::from_utf8_lossy(&bytes);
        extract_planner_result(&text, Some(&self.mission.contract_ids))
            .map_err(|e| GauntletError::blocked(e.to_string()))
    }

    pub fn _check_overlaps(&self, lanes: &[LaneState]) -> Vec<(String, String, String, String)> {
        let repo_lanes: Vec<LaneOverlap> = lanes
            .iter()
            .map(|l| LaneOverlap::new(l.id.clone(), l.owns.clone()))
            .collect();
        let files = tracked_files(&self.git, &self.repo_path()).unwrap_or_default();
        find_overlaps(&repo_lanes, &files)
    }

    // ------------------------------------------------------------- Phases

    pub fn run(&mut self) -> i32 {
        while !TERMINALS.contains(&self.state.phase.as_str()) {
            let res = match self.state.phase.as_str() {
                "INIT" => self._phase_init(),
                "PLAN" => self._phase_plan(),
                "PLAN_CHECKPOINT" => self._phase_plan_checkpoint(),
                "STAGES" => self._phase_stages(),
                "IMPLEMENT" => self._phase_implement(),
                "INSPECT" => self._phase_inspect(),
                "INTEGRATE" => self._phase_integrate(),
                "GATES" => self._phase_gates(),
                "REVIEW" => self._phase_review(),
                "JUDGE" => self._phase_judge(),
                "PLAN_FIX" => self._phase_plan_fix(),
                "POLISH" => self._phase_polish(),
                "DELIVER_CHECKPOINT" => self._phase_deliver_checkpoint(),
                "DELIVER" => self._phase_deliver(),
                other => {
                    self._blocked(&format!("unknown phase '{other}'"), "BLOCKED")
                }
            };

            if let Err(exc) = res {
                let _ = self._blocked(&exc.message, &exc.kind);
            }
        }

        let phase = &self.state.phase;
        let reason = if BLOCKED_TERMINALS.contains(&phase.as_str()) {
            format!(" — {}", self.state.blocked_reason.as_deref().unwrap_or(""))
        } else {
            String::new()
        };

        self.log(&format!("terminal phase: {phase}{reason}"));
        if let Some(ref report) = self.report {
            let _ = report.section("TERMINAL", &format!("{phase}{reason}"));
        }

        if phase == "READY" || phase == "READY_NO_CHANGE" {
            0
        } else {
            2
        }
    }

    pub fn _phase_init(&mut self) -> Result<(), GauntletError> {
        let repo = self.repo_path();
        if !is_git_repo(&self.git, &repo) {
            return Err(GauntletError::blocked(format!(
                "{} is not a git repository",
                repo.display()
            )));
        }

        let target = &self.state.target_branch;
        if !branch_exists(&self.git, &repo, target) {
            return Err(GauntletError::blocked(format!(
                "target branch '{target}' does not exist in {}",
                repo.display()
            )));
        }

        if staged_changes(&self.git, &repo) {
            return Err(GauntletError::blocked(format!(
                "refusing INIT: {} has staged changes on the checkout; commit or stash them first",
                repo.display()
            )));
        }

        self.state.base_commit = base_commit(&self.git, &repo, target)?;

        let date = chrono::Local::now().format("%Y%m%d").to_string();
        let missions_root = repo.join(".missions");
        let mut run_id = format!("{date}-{}", self.state.slug);
        let mut n = 2;
        while missions_root.join(&run_id).exists() {
            run_id = format!("{date}-{}-{n}", self.state.slug);
            n += 1;
        }

        self.state.run_id = run_id.clone();
        let run_dir = missions_root.join(&run_id);
        for sub in &["capsules", "outputs", "verdicts", "reviews"] {
            std::fs::create_dir_all(run_dir.join(sub)).map_err(|e| {
                GauntletError::blocked(format!("cannot create run dir {}: {e}", run_dir.display()))
            })?;
        }
        self.state.run_dir = Some(run_dir.to_string_lossy().to_string());
        self.run_dir = Some(run_dir.clone());

        if self.mission.source_path.is_file() {
            let _ = std::fs::copy(&self.mission.source_path, run_dir.join("mission.md"));
        }

        if let Ok(cfg_table) = self.config.to_table() {
            if let Ok(dumped) = dump_toml(&cfg_table) {
                let _ = std::fs::write(run_dir.join("config.toml"), dumped);
            }
        }

        self.report = Report::new(run_dir.join("report.md"), Some(&format!("Gauntlet run {run_id}"))).ok();

        // Integration worktree from target branch base commit
        let wt = self.integration_wt();
        let branch = self.integration_branch();
        create_worktree(&self.git, &repo, &wt, &branch, &self.state.base_commit)?;

        let wt_str = wt.to_string_lossy().to_string();
        if !self.state.worktrees.contains(&wt_str) {
            self.state.worktrees.push(wt_str);
        }
        if !self.state.branches.contains(&branch) {
            self.state.branches.push(branch);
        }

        self.state.lanes = self
            .mission
            .lanes
            .iter()
            .map(|lane| LaneState {
                id: lane.id.clone(),
                owns: lane.owns.clone(),
                forbidden: lane.forbidden.clone(),
                tests: lane.tests.clone(),
                brief: lane.brief.clone(),
                addresses: lane.addresses.clone(),
                ..Default::default()
            })
            .collect();

        if let Some(ref report) = self.report {
            let _ = report.section(
                "INIT",
                &format!(
                    "repo: {}\nbase: {}\ntarget branch: {}\npre-written lanes: {}",
                    repo.display(),
                    self.state.base_commit,
                    target,
                    self.state.lanes.len()
                ),
            );
        }

        let mut meta = vec![
            ("Repository", self.state.repo.as_str()),
            ("Target Branch", self.state.target_branch.as_str()),
            (
                "Base Commit",
                if self.state.base_commit.len() >= 12 {
                    &self.state.base_commit[..12]
                } else {
                    &self.state.base_commit
                },
            ),
        ];

        let gates_str = format!("{} gate(s)", self.state.gates.len());
        meta.push(("Gates Suite", &gates_str));
        let lanes_str = format!("{} lane(s)", self.state.lanes.len());
        meta.push(("Lanes Planned", &lanes_str));

        let profile_str;
        if let Some(ref pinfo) = self.profile_info {
            let reasons_sample = pinfo.reasons.iter().take(2).cloned().collect::<Vec<_>>().join("; ");
            profile_str = format!("⚡ {} ({reasons_sample})", pinfo.tier.to_uppercase());
            meta.push(("Pareto Profile", &profile_str));
        }

        if let Ok(mut ui) = default_ui().lock() {
            ui.banner(
                &format!("GAUNTLET MISSION • {}", self.state.slug),
                Some("Autonomous Multi-Agent Pareto State Machine"),
                Some(&meta),
                76,
            );
        }

        self._transition("PLAN")
    }

    pub fn _phase_plan(&mut self) -> Result<(), GauntletError> {
        if !self.state.lanes.is_empty() {
            let overlaps = self._check_overlaps(&self.state.lanes);
            if !overlaps.is_empty() {
                return Err(GauntletError::blocked(format!(
                    "config error: pre-written lane owns globs overlap: {overlaps:?}"
                )));
            }
            let summary = self
                .state
                .lanes
                .iter()
                .map(|l| format!("- {}: owns={:?} forbidden={:?}", l.id, l.owns, l.forbidden))
                .collect::<Vec<_>>()
                .join("\n");

            if let Some(ref report) = self.report {
                let _ = report.section("PLAN", &format!("(pre-written lanes)\n{summary}"));
            }
            self._save()?;
            self._transition("IMPLEMENT")?;
            return Ok(());
        }

        let plan_res = self._run_planner(None, None)?;
        match plan_res {
            PlannerResult::Stages(stages) if self.depth < self.max_depth => {
                self.state.stages = stages
                    .iter()
                    .filter_map(|s| serde_json::to_value(s).ok())
                    .collect();
                let summary = stages
                    .iter()
                    .enumerate()
                    .map(|(i, s)| format!("- Stage {} [{}]: {} (owns: {:?})", i + 1, s.slug, s.brief, s.owns))
                    .collect::<Vec<_>>()
                    .join("\n");

                if let Some(ref report) = self.report {
                    let _ = report.section("PLAN", &format!("(sequential stages)\n{summary}"));
                }
                self._save()?;

                if let Ok(mut ui) = default_ui().lock() {
                    ui.step("PLAN", &format!("Decomposed into {} sequential stage(s)", stages.len()), "");
                    for (i, s) in stages.iter().enumerate() {
                        ui.step(&format!("STAGE {}/{}", i + 1, stages.len()), &format!("{}: {}", s.slug, s.brief), "");
                    }
                }
                self._transition("STAGES")?;
                Ok(())
            }
            PlannerResult::Stages(stages) => {
                // Flatten stages into lanes if depth reached max
                let mut lanes = Vec::new();
                for (i, s) in stages.into_iter().enumerate() {
                    lanes.push(LaneState {
                        id: format!("S{}", i + 1),
                        owns: s.owns,
                        brief: s.brief,
                        ..Default::default()
                    });
                }
                self.state.lanes = lanes;
                self._save()?;
                self._transition("PLAN_CHECKPOINT")
            }
            PlannerResult::Lanes(plan_lanes) => {
                let mut lanes: Vec<LaneState> = plan_lanes
                    .into_iter()
                    .map(|p| LaneState {
                        id: p.id,
                        owns: p.owns,
                        forbidden: p.forbidden,
                        tests: p.tests,
                        brief: p.brief,
                        addresses: p.addresses,
                        ..Default::default()
                    })
                    .collect();

                let mut overlaps = self._check_overlaps(&lanes);
                if !overlaps.is_empty() {
                    // Retry once
                    let complaint = format!("lane owns globs overlapped: {overlaps:?}");
                    if let Ok(PlannerResult::Lanes(p2)) = self._run_planner(None, Some(&complaint)) {
                        lanes = p2
                            .into_iter()
                            .map(|p| LaneState {
                                id: p.id,
                                owns: p.owns,
                                forbidden: p.forbidden,
                                tests: p.tests,
                                brief: p.brief,
                                addresses: p.addresses,
                                ..Default::default()
                            })
                            .collect();
                        overlaps = self._check_overlaps(&lanes);
                    }
                    if !overlaps.is_empty() {
                        let context = format!("planner produced overlapping lanes twice: {overlaps:?}\napprove to proceed anyway");
                        if !self._ask_human("plan", &context) {
                            return Err(GauntletError::blocked(format!(
                                "planner could not produce orthogonal lanes: {overlaps:?}"
                            )));
                        }
                    }
                }

                self.state.lanes = lanes;
                let summary = self
                    .state
                    .lanes
                    .iter()
                    .map(|l| format!("- {}: owns={:?} forbidden={:?}", l.id, l.owns, l.forbidden))
                    .collect::<Vec<_>>()
                    .join("\n");

                if let Some(ref report) = self.report {
                    let _ = report.section("PLAN", &summary);
                }
                self._save()?;
                self._transition("PLAN_CHECKPOINT")
            }
        }
    }

    pub fn _phase_stages(&mut self) -> Result<(), GauntletError> {
        let stages_val = self.state.stages.clone();
        let total = stages_val.len();

        for (i, sval) in stages_val.iter().enumerate() {
            let stage: StageSpec = serde_json::from_value(sval.clone())
                .map_err(|e| GauntletError::blocked(format!("invalid stage spec: {e}")))?;

            if let Ok(mut ui) = default_ui().lock() {
                ui.stage_header(i + 1, total, &stage.slug, &stage.brief, 76);
            }

            let run_dir = match &self.run_dir {
                Some(d) => d,
                None => return Err(GauntletError::blocked("run_dir is not set".to_string())),
            };
            let sub_mission_path = run_dir
                .join("sub-missions")
                .join(format!("{:02}-{}.input.md", i + 1, stage.slug));

            let sub_mission = create_stage_mission(
                &self.mission,
                &stage,
                &self.integration_branch(),
                &sub_mission_path,
            )?;

            // Check if already completed
            let date = chrono::Local::now().format("%Y%m%d").to_string();
            let run_date = if self.state.run_id.len() >= 8 && self.state.run_id[..8].chars().all(|c| c.is_ascii_digit()) {
                self.state.run_id[..8].to_string()
            } else {
                date.clone()
            };
            let missions_root = self.repo_path().join(".missions");
            let mut expected_run_dir = missions_root.join(format!("{run_date}-{}", sub_mission.slug));
            if let Ok(entries) = std::fs::read_dir(&missions_root) {
                let mut best_dir = None;
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name.ends_with(&format!("-{}", sub_mission.slug)) || name == sub_mission.slug {
                        let candidate = entry.path();
                        if let Ok(st) = load(&candidate) {
                            if st.phase == "READY" {
                                best_dir = Some(candidate);
                                break;
                            }
                            if best_dir.is_none() {
                                best_dir = Some(candidate);
                            }
                        }
                    }
                }
                if let Some(dir) = best_dir {
                    expected_run_dir = dir;
                }
            }
            let resume_dir = if expected_run_dir.join("state.json").exists() {
                if let Ok(prior_state) = load(&expected_run_dir) {
                    if prior_state.phase == "READY" {
                        if let Ok(mut ui) = default_ui().lock() {
                            ui.success(
                                &format!("Stage {}/{} ({}) already completed (READY).", i + 1, total, stage.slug),
                                "",
                            );
                        }
                        self.state.integrated_changes = true;
                        continue;
                    }
                }
                Some(expected_run_dir)
            } else {
                None
            };

            let mut sub_orch = Orchestrator::new(
                &self.tool_dir,
                Some(&sub_mission_path),
                resume_dir.as_deref(),
                None,
                self.auto,
                self.dry_run,
                None,
                false,
                self.depth + 1,
                self.max_depth,
                self.log_fn.clone(),
            )?;

            let rc = sub_orch.run();
            if rc != 0 {
                let kind = sub_orch.state.blocked_kind.unwrap_or_else(|| "BLOCKED_STAGE".to_string());
                return Err(GauntletError::new(
                    format!(
                        "Stage {}/{} ({}) blocked in phase {}: {}",
                        i + 1,
                        total,
                        stage.slug,
                        sub_orch.state.phase,
                        sub_orch.state.blocked_reason.as_deref().unwrap_or("")
                    ),
                    kind,
                ));
            }

            self.state.integrated_changes = true;
            if let Ok(mut ui) = default_ui().lock() {
                ui.success(
                    &format!("Stage {}/{} ({}) completed successfully.", i + 1, total, stage.slug),
                    "",
                );
            }
        }

        self._transition("DELIVER_CHECKPOINT")
    }

    pub fn _phase_plan_checkpoint(&mut self) -> Result<(), GauntletError> {
        let summary = self
            .state
            .lanes
            .iter()
            .map(|l| format!("- {}: owns={:?}", l.id, l.owns))
            .collect::<Vec<_>>()
            .join("\n");

        if !self._checkpoint("plan", &format!("Planned lanes:\n{summary}"))? {
            return Err(GauntletError::blocked("plan checkpoint rejected by director"));
        }
        self._transition("IMPLEMENT")
    }

    pub fn _phase_implement(&mut self) -> Result<(), GauntletError> {
        let role = if self.state.wave == 0 {
            "implementer"
        } else {
            "fixer"
        };

        let todo_indices: Vec<usize> = self
            .state
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| LANE_ACTIVE.contains(&l.status.as_str()))
            .map(|(i, _)| i)
            .collect();

        if todo_indices.is_empty() {
            self._transition("INSPECT")?;
            return Ok(());
        }

        // Provision lane worktrees serially
        for &idx in &todo_indices {
            let lane = &self.state.lanes[idx];
            let wt = self.lane_wt(&lane.id);
            let branch = self.lane_branch(&lane.id);
            if !wt.exists() {
                create_worktree(
                    &self.git,
                    &self.repo_path(),
                    &wt,
                    &branch,
                    &self.state.base_commit,
                )?;
            }
            let wt_str = wt.to_string_lossy().to_string();
            if !self.state.worktrees.contains(&wt_str) {
                self.state.worktrees.push(wt_str);
            }
            if !self.state.branches.contains(&branch) {
                self.state.branches.push(branch);
            }
        }
        self._save()?;

        // Snapshot checkout status before running workers
        self.main_before = checkout_status(&self.git, &self.repo_path())?;

        // Execute workers across threads
        let mut handles = Vec::new();
        for &idx in &todo_indices {
            let lane = self.state.lanes[idx].clone();
            let role_str = role.to_string();
            let mission = self.mission.clone();
            let wave = self.state.wave;
            let run_id = self.state.run_id.clone();
            let run_dir = self.run_dir.clone().unwrap_or_default();
            let wt = self.lane_wt(&lane.id);
            let dry_run = self.dry_run;
            let config = self.config.clone();
            let health = Arc::clone(&self.health);

            let handle = thread::spawn(move || -> (usize, String, String, Vec<String>) {
                let capsule_text = capsules::implementer(
                    &mission,
                    &Lane {
                        id: lane.id.clone(),
                        owns: lane.owns.clone(),
                        forbidden: lane.forbidden.clone(),
                        tests: lane.tests.clone(),
                        brief: lane.brief.clone(),
                        addresses: lane.addresses.clone(),
                    },
                    wave as u32,
                    &run_id,
                    Some(&role_str),
                    None::<&[ClaimGroup]>,
                );

                let cap_name = format!("{role_str}-{}-w{wave}", lane.id);
                let cap_path = run_dir.join("capsules").join(format!("{cap_name}.md"));
                if let Some(p) = cap_path.parent() {
                    let _ = std::fs::create_dir_all(p);
                }
                let _ = std::fs::write(&cap_path, &capsule_text);

                // Run role
                let echo_cfg = AdapterConfig {
                    adapter: Some("echo".to_string()),
                    supports_write: Some(true),
                    default_model: None,
                    launcher: None,
                    errors: None,
                };
                let echo = EchoAdapter::new("echo", Some(&echo_cfg));

                let links = config
                    .roles
                    .get(&role_str)
                    .map(|r| r.chain.clone())
                    .unwrap_or_default();

                let out_dir = run_dir.join("outputs");
                let run_once = |link: &crate::config::ChainLink, _attempt: usize| -> RunResult {
                    if dry_run || link.harness == "echo" {
                        echo.run(
                            &cap_path,
                            &wt,
                            true,
                            link.model.as_deref(),
                            link.effort.as_deref(),
                            config.policy.lane_timeout_s,
                            None,
                            &out_dir,
                            &role_str,
                            Some(&lane.id),
                        )
                    } else {
                        // In non-dry-run, create and run adapter
                        if let Ok(ad) = create_adapter(&link.harness, &link.harness, None) {
                            ad.run(
                                &cap_path,
                                &wt,
                                true,
                                link.model.as_deref(),
                                link.effort.as_deref(),
                                config.policy.lane_timeout_s,
                                None,
                                &out_dir,
                                &role_str,
                                Some(&lane.id),
                            )
                        } else {
                            RunResult {
                                failure: FailureKind::Crash,
                                exit_code: None,
                                output_path: out_dir.join("err.out"),
                                detail: "unknown harness".to_string(),
                            }
                        }
                    }
                };

                let validate_fn = |res: &RunResult| -> (Option<FailureKind>, String) {
                    match extract_block_from_file(&res.output_path, "report") {
                        Ok(val) => match validate_report(&val) {
                            Ok(rep) => {
                                if rep.partial {
                                    (Some(FailureKind::PartialDelivery), "partial".to_string())
                                } else {
                                    (None, String::new())
                                }
                            }
                            Err(e) => (Some(FailureKind::OutputInvalid), e.to_string()),
                        },
                        Err(e) => (Some(FailureKind::OutputInvalid), e.to_string()),
                    }
                };

                let mut attempts = 0;
                let mut status = "done".to_string();
                let mut detail = String::new();
                let mut claimed = Vec::new();

                let outcome_res = (|| -> Result<RunResult, String> {
                    for link in &links {
                        if health.is_open(&link.harness) {
                            continue;
                        }
                        if attempts >= config.fallback.max_attempts_per_task {
                            break;
                        }
                        attempts += 1;
                        let mut res = run_once(link, attempts);
                        if res.failure == FailureKind::None {
                            let (vf, vd) = validate_fn(&res);
                            if let Some(f) = vf {
                                res.failure = f;
                                res.detail = vd;
                            }
                        }
                        if res.failure == FailureKind::None {
                            health.close(&link.harness);
                            return Ok(res);
                        }
                        if config.fallback.on_quota == "next_and_break" && res.failure == FailureKind::QuotaExhausted {
                            health.open(&link.harness);
                            continue;
                        }
                        if config.fallback.on_auth == "break" && res.failure == FailureKind::AuthExpired {
                            health.open(&link.harness);
                            return Err(format!("auth expired: {}", res.detail));
                        }
                    }
                    Err("chain exhausted".to_string())
                })();

                match outcome_res {
                    Ok(res) => {
                        if let Ok(val) = extract_block_from_file(&res.output_path, "report") {
                            if let Ok(rep) = validate_report(&val) {
                                claimed = rep.files_changed;
                            }
                        }
                    }
                    Err(e) => {
                        status = "failed".to_string();
                        detail = e;
                    }
                }

                (idx, status, detail, claimed)
            });

            handles.push(handle);
        }

        for h in handles {
            if let Ok((idx, status, detail, claimed)) = h.join() {
                self.state.lanes[idx].status = status;
                self.state.lanes[idx].detail = detail;
                self.state.lanes[idx].claimed = claimed;
            }
        }
        self._save()?;

        let failed: Vec<&LaneState> = self
            .state
            .lanes
            .iter()
            .filter(|l| l.status == "failed")
            .collect();

        if !failed.is_empty() {
            let failed_str = failed
                .iter()
                .map(|l| format!("{}: {}", l.id, l.detail))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GauntletError::blocked(format!("lane(s) failed: {failed_str}")));
        }

        if let Some(ref report) = self.report {
            let summary = todo_indices
                .iter()
                .map(|&i| format!("- {}: done ({role})", self.state.lanes[i].id))
                .collect::<Vec<_>>()
                .join("\n");
            let _ = report.section(&format!("IMPLEMENT wave {}", self.state.wave), &summary);
        }

        self._transition("INSPECT")
    }

    pub fn _phase_inspect(&mut self) -> Result<(), GauntletError> {
        let current_checkout = checkout_status(&self.git, &self.repo_path())?;
        let drift = checkout_drift(&self.main_before, &current_checkout, Some(&[".missions/"]));
        if !drift.is_empty() {
            return Err(GauntletError::blocked(format!(
                "SAFETY: main checkout modified while lanes ran (worker escaped its worktree): {}",
                drift.join(", ")
            )));
        }

        let repo_base = self.state.repo.clone();
        let run_id = self.state.run_id.clone();
        let base = if self.state.wave == 0 {
            self.state.base_commit.clone()
        } else {
            self.integration_branch()
        };

        let mut rejected_lanes = Vec::new();
        for lane in &mut self.state.lanes {
            if lane.status != "done" {
                continue;
            }

            let wt = PathBuf::from(format!("{repo_base}-worktree-gauntlet-{run_id}-{}", lane.id));
            lane.changed = lane_changed_files(&self.git, &wt, &base)?;

            let mut violations = check_lane_diff(&lane.changed, &lane.owns, &lane.forbidden);
            violations.extend(check_claimed_vs_diff(&lane.claimed, &lane.changed));

            if !violations.is_empty() {
                lane.status = "rejected".to_string();
                lane.detail = violations.join("; ");
                rejected_lanes.push(lane.clone());
            } else {
                lane.detail = format!("{} file(s) changed", lane.changed.len());
            }
        }
        self._save()?;

        if let Some(ref report) = self.report {
            let summary = self
                .state
                .lanes
                .iter()
                .map(|l| format!("- {}: {} {}", l.id, l.status, l.detail))
                .collect::<Vec<_>>()
                .join("\n");
            let _ = report.section("INSPECT", &summary);
        }

        if !rejected_lanes.is_empty() {
            if let Ok(mut ui) = default_ui().lock() {
                for l in &rejected_lanes {
                    ui.error(&format!("Lane {} rejected", l.id), &l.detail);
                }
            }
            let rej_str = rejected_lanes
                .iter()
                .map(|l| format!("{}: {}", l.id, l.detail))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GauntletError::blocked(format!("INSPECT rejected lane(s): {rej_str}")));
        } else if let Ok(mut ui) = default_ui().lock() {
            for l in &self.state.lanes {
                if l.status == "done" {
                    ui.success(&format!("Lane {} inspection passed", l.id), &l.detail);
                }
            }
        }

        self._transition("INTEGRATE")
    }

    pub fn _phase_integrate(&mut self) -> Result<(), GauntletError> {
        let mut integrated = Vec::new();
        let repo_base = self.state.repo.clone();
        let run_id = self.state.run_id.clone();
        let wave = self.state.wave;
        let int_wt = self.integration_wt();

        for i in 0..self.state.lanes.len() {
            if self.state.lanes[i].status != "done" {
                continue;
            }
            let lane_id = self.state.lanes[i].id.clone();
            let lane_changed = !self.state.lanes[i].changed.is_empty();
            let lane_wt = PathBuf::from(format!("{repo_base}-worktree-gauntlet-{run_id}-{lane_id}"));
            let lane_branch = format!("gauntlet/{run_id}/{lane_id}");

            if lane_changed {
                commit_all(
                    &self.git,
                    &lane_wt,
                    &format!("gauntlet({run_id}): lane {lane_id} wave {wave}"),
                )?;

                merge_branch(&self.git, &int_wt, &lane_branch)?;

                self.state.integrated_changes = true;
                integrated.push(lane_id.clone());

                if let Some(ref run_dir) = self.run_dir {
                    let cap_prefix = if wave == 0 { "implementer" } else { "fixer" };
                    let cap_file = run_dir
                        .join("capsules")
                        .join(format!("{cap_prefix}-{lane_id}-w{wave}.md"));
                    let _ = std::fs::remove_file(cap_file);
                }
            }
            self.state.lanes[i].status = "integrated".to_string();
            self._save()?;
        }

        if let Some(ref report) = self.report {
            let int_str = if integrated.is_empty() {
                "(no changes)".to_string()
            } else {
                integrated.join(", ")
            };
            let _ = report.section("INTEGRATE", &format!("merged lanes: {int_str}"));
        }

        self._transition("GATES")
    }

    pub fn _phase_gates(&mut self) -> Result<(), GauntletError> {
        let out_dir = self
            .run_dir
            .as_ref()
            .map(|d| d.join("outputs"))
            .unwrap_or_else(|| PathBuf::from("outputs"));
        let results = run_gates(
            &self.state.gates,
            &self.integration_wt(),
            &out_dir,
            self.dry_run,
            self.log_fn.as_deref().map(|f| f as &dyn Fn(&str)),
            Some(&|idx, total, cmd, ok, dur, det| {
                if let Ok(mut ui) = default_ui().lock() {
                    ui.gate_result(idx, total, cmd, ok, dur, det);
                }
            }),
            self.config.policy.lane_timeout_s,
        );

        let failed: Vec<&GateResult> = results.iter().filter(|r| !r.ok).collect();

        if let Some(ref report) = self.report {
            let summary = if results.is_empty() {
                "(no gates)".to_string()
            } else {
                results
                    .iter()
                    .map(|r| format!("- {}: {}", if r.ok { "ok" } else { "FAIL" }, r.command))
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            let _ = report.section("GATES", &summary);
        }

        if !failed.is_empty() {
            let fail_str = failed
                .iter()
                .map(|r| format!("{} ({})", r.command, r.detail))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(GauntletError::new(
                format!("required gate(s) failed: {fail_str}"),
                "BLOCKED_GATE",
            ));
        }

        self._transition("REVIEW")
    }

    pub fn _phase_review(&mut self) -> Result<(), GauntletError> {
        let run_dir = match &self.run_dir {
            Some(d) => d.clone(),
            None => return Err(GauntletError::blocked("run_dir is not set".to_string())),
        };
        let diff_path = run_dir
            .join("reviews")
            .join(format!("diff-w{}.patch", self.state.wave));
        if let Some(p) = diff_path.parent() {
            let _ = std::fs::create_dir_all(p);
        }

        let wt = self.integration_wt();
        let diff = if wt.is_dir() {
            self.git
                .run(&["diff", &self.state.base_commit], Some(&wt), false, true)?
                .unwrap_or_else(|| "(empty diff)\n".to_string())
        } else {
            "(integration worktree unavailable — dry-run)\n".to_string()
        };
        let _ = std::fs::write(&diff_path, &diff);

        let (fixed, deferred, dismissed) = self._prior_findings();
        let diff_str = diff_path.to_string_lossy().to_string();
        let capsule_text = capsules::reviewer(
            &self.mission,
            self.state.wave as u32,
            &self.state.run_id,
            Some(&diff_str),
            Some(&fixed),
            Some(&deferred),
            Some(&dismissed),
        );

        let cap_path = self._write_capsule(
            &format!("reviewer-w{}", self.state.wave),
            &capsule_text,
        )?;

        let outcome = self._run_role("reviewer", &cap_path, &self.work_wt(), false, None)?;
        let data = extract_block_from_file(&outcome.result.output_path, "verdict")?;

        let verdict_path = run_dir
            .join("verdicts")
            .join(format!("review-w{}.json", self.state.wave));
        let data_str = serde_json::to_string_pretty(&data).unwrap_or_else(|_| "{}".to_string());
        let _ = std::fs::write(&verdict_path, format!("{data_str}\n"));

        self.state.reviews = Self::slot(
            &self.state.reviews,
            self.state.wave,
            verdict_path.to_string_lossy().to_string(),
        );

        let group_count = data
            .get("groups")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);

        if let Some(ref report) = self.report {
            let _ = report.section(
                &format!("REVIEW wave {}", self.state.wave),
                &format!("groups: {group_count} (harness: {}, attempts: {})", outcome.harness, outcome.attempts),
            );
        }

        if let Ok(mut ui) = default_ui().lock() {
            ui.step("REVIEW", &format!("Review completed with {group_count} finding group(s)"), "");
        }

        self._transition("JUDGE")
    }

    pub fn _phase_judge(&mut self) -> Result<(), GauntletError> {
        let run_dir = match &self.run_dir {
            Some(d) => d.clone(),
            None => return Err(GauntletError::blocked("run_dir is not set".to_string())),
        };
        let review_json_path = match self.state.reviews.last() {
            Some(p) => p.clone(),
            None => return Err(GauntletError::blocked("no reviews recorded".to_string())),
        };
        let review_json = std::fs::read_to_string(&review_json_path)
            .map_err(|e| GauntletError::blocked(format!("cannot read review JSON: {e}")))?;

        let (_, deferred, dismissed) = self._prior_findings();
        let capsule_text = capsules::judge(
            &self.mission,
            self.state.wave as u32,
            &self.state.run_id,
            &review_json,
            Some(&deferred),
            Some(&dismissed),
        );

        let cap_path = self._write_capsule(
            &format!("judge-w{}", self.state.wave),
            &capsule_text,
        )?;

        let outcome = self._run_role("judge", &cap_path, &self.work_wt(), false, None)?;
        let data = extract_block_from_file(&outcome.result.output_path, "verdict")?;
        let groups = validate_verdict(&data, &self.mission.contract_ids)?;

        let judgment_path = run_dir
            .join("verdicts")
            .join(format!("judgment-w{}.json", self.state.wave));
        let data_str = serde_json::to_string_pretty(&data).unwrap_or_else(|_| "{}".to_string());
        let _ = std::fs::write(&judgment_path, format!("{data_str}\n"));

        self.state.judgments = Self::slot(
            &self.state.judgments,
            self.state.wave,
            judgment_path.to_string_lossy().to_string(),
        );

        let blocking: Vec<ClaimGroup> = groups.iter().filter(|g| g.blocking()).cloned().collect();
        let polish: Vec<ClaimGroup> = groups.iter().filter(|g| g.polish()).cloned().collect();

        self.state.blocking_history = Self::slot(
            &self.state.blocking_history,
            self.state.wave,
            blocking.len(),
        );
        self._save()?;

        let trajectory = self
            .state
            .blocking_history
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" → ");

        if let Some(ref report) = self.report {
            let _ = report.section(
                &format!("JUDGE wave {}", self.state.wave),
                &format!(
                    "groups: {}, blocking: {}, polish: {}\nblocking trajectory: {trajectory}",
                    groups.len(),
                    blocking.len(),
                    polish.len()
                ),
            );
        }

        if let Ok(mut ui) = default_ui().lock() {
            ui.verdicts_table(&groups, 76);
        }

        if blocking.is_empty() {
            self._transition("POLISH")?;
            return Ok(());
        }

        let history_slice = if self.state.wave < self.state.blocking_history.len() {
            &self.state.blocking_history[..self.state.wave]
        } else {
            &[]
        };

        let decision = convergence_state(
            history_slice,
            blocking.len(),
            self.state.wave,
            self.config.policy.max_total_waves,
        );

        let kind = if blocking.iter().any(|g| g.verdict == "REDESIGN") {
            "BLOCKED_ARCHITECTURE"
        } else {
            "BLOCKED_CONVERGENCE"
        };

        if decision == CAPPED {
            return Err(GauntletError::new(
                format!(
                    "fix waves hit the absolute cap (max_total_waves={}) with {} blocking group(s) left (trajectory {trajectory})",
                    self.config.policy.max_total_waves,
                    blocking.len()
                ),
                kind,
            ));
        }

        if decision == STALLED {
            let context = format!(
                "convergence stalled: {} blocking group(s) after {} fix wave(s); trajectory {} did not beat its own best round.\napprove to grant one more fix wave anyway",
                blocking.len(),
                self.state.wave,
                trajectory
            );
            if self.config.policy.on_wave_cap != "checkpoint" || !self._ask_human("wave-cap", &context) {
                return Err(GauntletError::new(
                    format!(
                        "convergence stalled: {} blocking group(s) left, trajectory {trajectory}",
                        blocking.len()
                    ),
                    kind,
                ));
            }
            self.log("director granted one more fix wave despite the stall");
        }

        self.state.wave += 1;
        self.pending_groups = blocking;
        self._transition("PLAN_FIX")
    }

    pub fn _phase_plan_fix(&mut self) -> Result<(), GauntletError> {
        let groups = if !self.pending_groups.is_empty() {
            self.pending_groups.clone()
        } else {
            self._last_judgment_groups()
                .into_iter()
                .filter(|g| g.blocking())
                .collect()
        };

        let is_echo = self
            .config
            .roles
            .get("planner")
            .and_then(|r| r.chain.first())
            .map(|l| l.harness == "echo")
            .unwrap_or(false);

        let lanes: Vec<LaneState> = if !is_echo && groups.len() == 1 {
            let g = &groups[0];
            let mut owns = if !g.owns.is_empty() {
                vec![g.owns.clone()]
            } else {
                Vec::new()
            };
            if let Some(first_own) = owns.first() {
                if first_own.starts_with("lib/") {
                    let cand = first_own.replace("lib/", "test/").replace(".js", ".test.js");
                    if self.repo_path().join(&cand).is_file() && !owns.contains(&cand) {
                        owns.push(cand);
                    }
                }
            }
            let tests = if owns.len() > 1 && owns.last().map(|s| s.ends_with(".test.js")).unwrap_or(false) {
                owns.last().map(|last| vec![format!("node --test {last}")]).unwrap_or_default()
            } else {
                Vec::new()
            };
            vec![LaneState {
                id: "L1".to_string(),
                owns,
                forbidden: Vec::new(),
                tests,
                brief: if !g.fix.is_empty() { g.fix.clone() } else { g.root_cause.clone() },
                addresses: vec![g.root_cause.clone()],
                ..Default::default()
            }]
        } else {
            let plan_res = self._run_planner(Some(&groups), None)?;
            let plan_lanes = match plan_res {
                PlannerResult::Lanes(l) => l,
                PlannerResult::Stages(s) => s
                    .into_iter()
                    .enumerate()
                    .map(|(i, st)| PlanLane {
                        id: format!("L{}", i + 1),
                        owns: st.owns,
                        forbidden: Vec::new(),
                        tests: Vec::new(),
                        brief: st.brief,
                        addresses: Vec::new(),
                    })
                    .collect(),
            };

            let mut lstates: Vec<LaneState> = plan_lanes
                .into_iter()
                .map(|p| LaneState {
                    id: p.id,
                    owns: p.owns,
                    forbidden: p.forbidden,
                    tests: p.tests,
                    brief: p.brief,
                    addresses: p.addresses,
                    ..Default::default()
                })
                .collect();

            let mut overlaps = self._check_overlaps(&lstates);
            if !overlaps.is_empty() {
                let complaint = format!("lane owns globs overlapped: {overlaps:?}");
                if let Ok(PlannerResult::Lanes(p2)) = self._run_planner(Some(&groups), Some(&complaint)) {
                    lstates = p2
                        .into_iter()
                        .map(|p| LaneState {
                            id: p.id,
                            owns: p.owns,
                            forbidden: p.forbidden,
                            tests: p.tests,
                            brief: p.brief,
                            addresses: p.addresses,
                            ..Default::default()
                        })
                        .collect();
                    overlaps = self._check_overlaps(&lstates);
                }
                if !overlaps.is_empty() {
                    let should_coalesce = self.auto
                        || !self._ask_human(
                            "plan-fix",
                            &format!(
                                "fix-wave lanes overlap: {overlaps:?}\ncoalesce overlapping lanes automatically"
                            ),
                        );
                    if should_coalesce {
                        let repo_files = tracked_files(&self.git, &self.repo_path())
                            .unwrap_or_default();
                        let mut merged = lstates;
                        let mut changed = true;
                        while changed {
                            changed = false;
                            let mut pair = None;
                            for i in 0..merged.len() {
                                for j in (i + 1)..merged.len() {
                                    let owns_i = &merged[i].owns;
                                    let owns_j = &merged[j].owns;
                                    let has_overlap = owns_i.iter().any(|ga| {
                                        owns_j
                                            .iter()
                                            .any(|gb| globs_may_overlap(ga, gb, &repo_files))
                                    });
                                    if has_overlap {
                                        pair = Some((i, j));
                                        break;
                                    }
                                }
                                if pair.is_some() {
                                    break;
                                }
                            }
                            if let Some((idx_a, idx_b)) = pair {
                                let la = merged[idx_a].clone();
                                let lb = merged[idx_b].clone();

                                let mut combined_owns = la.owns;
                                for o in lb.owns {
                                    if !combined_owns.contains(&o) {
                                        combined_owns.push(o);
                                    }
                                }
                                let mut combined_addr = la.addresses;
                                for a in lb.addresses {
                                    if !combined_addr.contains(&a) {
                                        combined_addr.push(a);
                                    }
                                }
                                let mut combined_tests = la.tests;
                                for t in lb.tests {
                                    if !combined_tests.contains(&t) {
                                        combined_tests.push(t);
                                    }
                                }
                                let combined_brief = if la.brief != lb.brief {
                                    format!("{}\nAlso: {}", la.brief, lb.brief)
                                } else {
                                    la.brief
                                };

                                let new_lane = LaneState {
                                    id: format!("L{}", idx_a + 1),
                                    owns: combined_owns,
                                    forbidden: Vec::new(),
                                    tests: combined_tests,
                                    brief: combined_brief,
                                    addresses: combined_addr,
                                    ..Default::default()
                                };
                                merged.remove(idx_b);
                                merged.remove(idx_a);
                                merged.insert(idx_a, new_lane);
                                changed = true;
                            }
                        }
                        for (idx, lane) in merged.iter_mut().enumerate() {
                            lane.id = format!("L{}", idx + 1);
                        }
                        lstates = merged;
                    } else {
                        return Err(GauntletError::blocked(format!(
                            "fix-wave planner could not produce orthogonal lanes: {overlaps:?}"
                        )));
                    }
                }
            }
            lstates
        };

        self.state.lanes = lanes;
        if let Some(ref report) = self.report {
            let summary = self
                .state
                .lanes
                .iter()
                .map(|l| format!("- {}: owns={:?} addresses={:?}", l.id, l.owns, l.addresses))
                .collect::<Vec<_>>()
                .join("\n");
            let _ = report.section(&format!("PLAN_FIX wave {}", self.state.wave), &summary);
        }
        self._save()?;
        self._transition("IMPLEMENT")
    }

    pub fn _polish_groups(&self) -> Vec<ClaimGroup> {
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for path in &self.state.judgments {
            for group in self._judgment_groups(path) {
                if group.polish() && !seen.contains(&group.root_cause) {
                    seen.insert(group.root_cause.clone());
                    out.push(group);
                }
            }
        }
        out
    }

    pub fn _phase_polish(&mut self) -> Result<(), GauntletError> {
        let wt = self.integration_wt();
        let groups = self._polish_groups();

        if self.state.polish_done || groups.is_empty() || !wt.is_dir() {
            self.state.polish_done = true;
            if !groups.is_empty() && !wt.is_dir() {
                self.state.polish_detail = format!(
                    "{} non-blocking finding(s) left unpolished: no integration worktree",
                    groups.len()
                );
            }
            self._transition("DELIVER_CHECKPOINT")?;
            return Ok(());
        }

        let owns: Vec<String> = groups
            .iter()
            .filter(|g| !g.owns.is_empty())
            .map(|g| g.owns.clone())
            .collect();
        let contained = owns.len() == groups.len();

        let capsule_text = capsules::polish(
            &self.mission,
            &groups,
            self.state.wave as u32,
            &self.state.run_id,
            if contained { Some(&owns) } else { None },
        );

        let cap_path = self._write_capsule(
            &format!("polish-w{}", self.state.wave),
            &capsule_text,
        )?;

        let before = checkout_status(&self.git, &self.repo_path())?;
        let outcome_res = self._run_role("fixer", &cap_path, &wt, true, None);

        let detail = match outcome_res {
            Err(e) => format!("polish pass failed, candidate unchanged: {e}"),
            Ok(_) => self._settle_polish(&wt, &groups, &owns, contained)?,
        };

        let current = checkout_status(&self.git, &self.repo_path())?;
        let drift = checkout_drift(&before, &current, Some(&[".missions/"]));
        if !drift.is_empty() {
            return Err(GauntletError::blocked(format!(
                "SAFETY: main checkout modified during the polish pass: {}",
                drift.join(", ")
            )));
        }

        self.state.polish_done = true;
        self.state.polish_detail = detail.clone();
        if let Some(ref report) = self.report {
            let _ = report.section(&format!("POLISH wave {}", self.state.wave), &detail);
        }
        self._save()?;
        self._transition("DELIVER_CHECKPOINT")
    }

    pub fn _settle_polish(
        &mut self,
        wt: &Path,
        groups: &[ClaimGroup],
        owns: &[String],
        contained: bool,
    ) -> Result<String, GauntletError> {
        let changed = checkout_status(&self.git, wt)?;
        if changed.is_empty() {
            return Ok(format!("{} finding(s) submitted, nothing changed", groups.len()));
        }

        if contained {
            let violations = check_lane_diff(&changed, owns, &[]);
            if !violations.is_empty() {
                discard_changes(&self.git, wt)?;
                return Ok(format!(
                    "polish discarded (wrote outside the findings' owns): {}",
                    violations.join("; ")
                ));
            }
        } else {
            self.log("polish: some findings declare no owns; containment not enforced");
        }

        let out_dir = self
            .run_dir
            .as_ref()
            .map(|d| d.join("outputs"))
            .unwrap_or_else(|| PathBuf::from("outputs"));
        let results = run_gates(
            &self.state.gates,
            wt,
            &out_dir,
            self.dry_run,
            self.log_fn.as_deref().map(|f| f as &dyn Fn(&str)),
            None,
            self.config.policy.lane_timeout_s,
        );

        let failed: Vec<&GateResult> = results.iter().filter(|r| !r.ok).collect();
        if !failed.is_empty() {
            discard_changes(&self.git, wt)?;
            let fail_str = failed.iter().map(|r| r.command.as_str()).collect::<Vec<_>>().join("; ");
            return Ok(format!("polish discarded (gates failed on its result): {fail_str}"));
        }

        commit_all(
            &self.git,
            wt,
            &format!(
                "gauntlet({}): polish pass ({} non-blocking finding(s))",
                self.state.run_id,
                groups.len()
            ),
        )?;

        Ok(format!(
            "{} finding(s) cleared, {} file(s) changed: {}",
            groups.len(),
            changed.len(),
            changed.join(", ")
        ))
    }

    pub fn _phase_deliver_checkpoint(&mut self) -> Result<(), GauntletError> {
        let polish_det = if !self.state.polish_detail.is_empty() {
            &self.state.polish_detail
        } else {
            "nothing to polish"
        };
        let context = format!(
            "run: {}\nwave: {}\nchanges integrated: {}\njudgment: no blocking groups\npolish: {}\napprove to deliver into the target branch",
            self.state.run_id,
            self.state.wave,
            self.state.integrated_changes,
            polish_det
        );

        if !self._checkpoint("deliver", &context)? {
            return Err(GauntletError::blocked("deliver checkpoint rejected by director"));
        }
        self._transition("DELIVER")
    }

    pub fn _phase_deliver(&mut self) -> Result<(), GauntletError> {
        let repo = self.repo_path();
        let target = self.state.target_branch.clone();
        let branch = self.integration_branch();
        let head = rev_parse(&self.git, &repo, &branch);

        let final_phase = if !self.state.integrated_changes
            || head.is_none()
            || head.as_ref() == Some(&self.state.base_commit)
        {
            "READY_NO_CHANGE".to_string()
        } else {
            if let Err(rebase_err) = rebase_onto(&self.git, &self.integration_wt(), &target) {
                let _ = self.git.run(&["rebase", "--abort"], Some(&self.integration_wt()), true, true);
                if let Err(merge_err) = merge_branch(&self.git, &self.integration_wt(), &target) {
                    return Err(GauntletError::blocked(format!(
                        "deliver integration sync failed (rebase: {rebase_err}, merge: {merge_err})"
                    )));
                }
            }

            let out_dir = self
                .run_dir
                .as_ref()
                .map(|d| d.join("outputs"))
                .unwrap_or_else(|| PathBuf::from("outputs"));
            let results = run_gates(
                &self.state.gates,
                &self.integration_wt(),
                &out_dir,
                self.dry_run,
                self.log_fn.as_deref().map(|f| f as &dyn Fn(&str)),
                None,
                self.config.policy.lane_timeout_s,
            );

            let failed: Vec<&GateResult> = results.iter().filter(|r| !r.ok).collect();
            if !failed.is_empty() {
                let fail_str = failed.iter().map(|r| r.command.as_str()).collect::<Vec<_>>().join("; ");
                return Err(GauntletError::new(
                    format!("gate(s) failed after rebase: {fail_str}"),
                    "BLOCKED_GATE",
                ));
            }

            if self.depth > 0 {
                let target_wt = find_worktree_for_branch(&self.git, &repo, &target)?;
                if let Some(t_wt) = target_wt {
                    if let Err(_) = ff_merge(&self.git, &t_wt, &branch) {
                        merge_branch(&self.git, &t_wt, &branch)?;
                    }
                } else {
                    let _ = self.git.run(&["branch", "-f", &target, &branch], Some(&repo), true, true);
                }
            } else {
                let curr_b = crate::worktrees::current_branch(&self.git, &repo)?;
                if curr_b != target {
                    return Err(GauntletError::blocked(format!(
                        "main checkout is not on '{target}'; refusing fast-forward merge"
                    )));
                }
                if let Err(_) = ff_merge(&self.git, &repo, &branch) {
                    merge_branch(&self.git, &repo, &branch)?;
                }
            }
            "READY".to_string()
        };

        self._cleanup();

        if let Some(ref report) = self.report {
            let _ = report.section("DELIVER", &final_phase);
        }

        if let Ok(mut ui) = default_ui().lock() {
            if final_phase == "READY" {
                ui.success(
                    &format!("Mission '{}' delivered successfully into '{}'!", self.state.slug, target),
                    "",
                );
            } else {
                ui.success(
                    &format!("Mission '{}' verified: behavior already satisfies contract.", self.state.slug),
                    "",
                );
            }
        }

        self._transition(&final_phase)
    }

    pub fn _cleanup(&mut self) {
        for wt_str in &self.state.worktrees {
            let wt = Path::new(wt_str);
            if wt.is_dir() || self.dry_run {
                let _ = remove_worktree(&self.git, &self.repo_path(), wt);
            }
        }
        for branch in &self.state.branches {
            let _ = delete_branch(&self.git, &self.repo_path(), branch);
        }
    }
}
