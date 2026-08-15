pub mod adapters;
pub mod autoroute;
pub mod capsules;
pub mod cli;
pub mod config;
pub mod fallback;
pub mod gates;
pub mod mission;
pub mod orchestrator;
pub mod report;
pub mod statemachine;
pub mod ui;
pub mod verdicts;
pub mod worktrees;

pub use config::{
    builtin_defaults, dump_toml, load_config, load_config_table, merge as merge_config,
    validate_config, ChainLink, Config, ConfigError, FallbackConfig, HarnessConfig, PolicyConfig,
    RoleConfig, ADAPTER_NAMES, FALLBACK_ACTIONS, ROLES, WAVE_CAP_ACTIONS, WRITE_ROLES,
};

pub use mission::{
    create_stage_mission, load_mission, parse_mission, Lane, Mission, MissionError, Repo,
    StageSpec,
};

pub use statemachine::{
    convergence_state, load as load_state, save as save_state, LaneState, State,
    StatemachineError, BLOCKED_TERMINALS, CAPPED, CONVERGING, LANE_ACTIVE, PHASES, STALLED,
    TERMINALS,
};

pub use verdicts::{
    extract_block, extract_block_from_file, extract_planner_result, validate_plan, validate_report,
    validate_stages, validate_verdict, ClaimGroup, PlanLane, PlannerResult, ReportData,
    VerdictError, ACTIONABLE_VERDICTS, CLASS_VALUES, CODE_DEFECT, VERDICT_VALUES,
};

pub use report::Report;
