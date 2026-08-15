//! Gauntlet CLI entry point.
//!
//! Usage: ./gauntlet [--config FILE] [--auto] [--resume RUN_DIR] [--dry-run] MISSION.md

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use crate::orchestrator::{GauntletError, Orchestrator};
use crate::ui::default_ui;

#[derive(Debug, Clone)]
pub struct CliArgs {
    pub mission: Option<PathBuf>,
    pub config: Option<PathBuf>,
    pub auto: bool,
    pub resume: Option<PathBuf>,
    pub dry_run: bool,
    pub replan: bool,
    pub profile: Option<String>,
    pub no_color: bool,
}

fn resolve_path(path: Option<&str>, base: &Path) -> Option<PathBuf> {
    path.map(|p| {
        let pb = PathBuf::from(p);
        if pb.is_absolute() {
            pb
        } else {
            base.join(pb)
        }
    })
}

pub fn parse_args<I, T>(args_iter: I) -> Result<CliArgs, String>
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let args: Vec<String> = args_iter
        .into_iter()
        .map(|s| s.into().to_string_lossy().to_string())
        .collect();

    let mut mission_str: Option<String> = None;
    let mut config_str: Option<String> = None;
    let mut resume_str: Option<String> = None;
    let mut auto = true;
    let mut dry_run = false;
    let mut replan = false;
    let mut profile_str: Option<String> = None;
    let mut no_color = false;

    let mut i = 1;
    while i < args.len() {
        let arg = match args.get(i) {
            Some(a) => a,
            None => break,
        };
        if arg == "--config" {
            i += 1;
            let val = args.get(i).ok_or_else(|| "error: --config requires a file argument".to_string())?;
            config_str = Some(val.clone());
        } else if let Some(val) = arg.strip_prefix("--config=") {
            config_str = Some(val.to_string());
        } else if arg == "--resume" {
            i += 1;
            let val = args.get(i).ok_or_else(|| "error: --resume requires a directory argument".to_string())?;
            resume_str = Some(val.clone());
        } else if let Some(val) = arg.strip_prefix("--resume=") {
            resume_str = Some(val.to_string());
        } else if arg == "--auto" {
            auto = true;
        } else if arg == "--interactive" || arg == "--no-auto" {
            auto = false;
        } else if arg == "--dry-run" {
            dry_run = true;
        } else if arg == "--replan" {
            replan = true;
        } else if arg == "--no-color" {
            no_color = true;
        } else if arg == "--profile" {
            i += 1;
            let p = args.get(i).ok_or_else(|| {
                "error: --profile requires a tier argument (auto, fast, standard, high-risk)".to_string()
            })?.clone();
            if !matches!(p.as_str(), "auto" | "fast" | "standard" | "high-risk") {
                return Err(format!("error: invalid profile '{p}'. Choose from auto, fast, standard, high-risk"));
            }
            profile_str = if p == "auto" { None } else { Some(p) };
        } else if let Some(val) = arg.strip_prefix("--profile=") {
            let p = val.to_string();
            if !matches!(p.as_str(), "auto" | "fast" | "standard" | "high-risk") {
                return Err(format!("error: invalid profile '{p}'. Choose from auto, fast, standard, high-risk"));
            }
            profile_str = if p == "auto" { None } else { Some(p) };
        } else if arg == "-h" || arg == "--help" {
            println!("Usage: gauntlet [--config FILE] [--auto] [--interactive] [--resume RUN_DIR] [--dry-run] [--replan] [--profile auto|fast|standard|high-risk] [--no-color] [MISSION.md]");
            process::exit(0);
        } else if arg.starts_with('-') {
            return Err(format!("error: unrecognized option '{arg}'"));
        } else {
            if mission_str.is_none() {
                mission_str = Some(arg.clone());
            } else {
                return Err(format!("error: unexpected positional argument '{arg}'"));
            }
        }
        i += 1;
    }

    let invoked_cwd = match env::var("GAUNTLET_INVOKED_CWD") {
        Ok(val) if !val.is_empty() => PathBuf::from(val),
        _ => env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };

    let mission = resolve_path(mission_str.as_deref(), &invoked_cwd);
    let config = resolve_path(config_str.as_deref(), &invoked_cwd);
    let resume = resolve_path(resume_str.as_deref(), &invoked_cwd);

    Ok(CliArgs {
        mission,
        config,
        auto,
        resume,
        dry_run,
        replan,
        profile: profile_str,
        no_color,
    })
}

pub fn main<I, T>(args: I) -> i32
where
    I: IntoIterator<Item = T>,
    T: Into<std::ffi::OsString> + Clone,
{
    let parsed = match parse_args(args) {
        Ok(a) => a,
        Err(err) => {
            eprintln!("gauntlet: {err}");
            return 1;
        }
    };

    if parsed.no_color {
        env::set_var("NO_COLOR", "1");
        if let Ok(mut ui) = default_ui().lock() {
            ui.enable_color = false;
        }
    }

    if parsed.resume.is_none() && parsed.mission.is_none() {
        eprintln!("gauntlet: error: MISSION.md is required unless --resume is given");
        return 1;
    }

    if let Some(ref m_path) = parsed.mission {
        if !m_path.is_file() {
            eprintln!("gauntlet: error: mission file not found: {}", m_path.display());
            return 1;
        }
    }

    if let Some(ref c_path) = parsed.config {
        if !c_path.is_file() {
            eprintln!("gauntlet: error: config file not found: {}", c_path.display());
            return 1;
        }
    }

    if let Some(ref r_dir) = parsed.resume {
        if !r_dir.join("state.json").is_file() {
            eprintln!("gauntlet: error: run directory has no state.json: {}", r_dir.display());
            return 1;
        }
    }

    let tool_dir = match env::current_exe() {
        Ok(exe) => exe
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf(),
        Err(_) => PathBuf::from("."),
    };

    let mut orch = match Orchestrator::new(
        &tool_dir,
        parsed.mission.as_deref(),
        parsed.resume.as_deref(),
        parsed.config.as_deref(),
        parsed.auto,
        parsed.dry_run,
        parsed.profile.as_deref(),
        parsed.replan,
        0,
        2,
        None,
    ) {
        Ok(o) => o,
        Err(GauntletError { message, .. }) => {
            eprintln!("gauntlet: {message}");
            return 1;
        }
    };

    orch.run()
}
