//! Terminal UI & Live visibility engine for Gauntlet.
//!
//! Provides modern, informative, styled terminal output with ANSI colors,
//! live execution timers/spinners, progress bars, and structured tables.

use std::collections::BTreeMap;
use std::io::{self, IsTerminal, Write};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::verdicts::ClaimGroup;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const ITALIC: &str = "\x1b[3m";
pub const UNDERLINE: &str = "\x1b[4m";

// Foreground colors
pub const BLACK: &str = "\x1b[30m";
pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const BLUE: &str = "\x1b[34m";
pub const MAGENTA: &str = "\x1b[35m";
pub const CYAN: &str = "\x1b[36m";
pub const WHITE: &str = "\x1b[37m";

// Bright foreground colors
pub const BRIGHT_RED: &str = "\x1b[91m";
pub const BRIGHT_GREEN: &str = "\x1b[92m";
pub const BRIGHT_YELLOW: &str = "\x1b[93m";
pub const BRIGHT_BLUE: &str = "\x1b[94m";
pub const BRIGHT_MAGENTA: &str = "\x1b[95m";
pub const BRIGHT_CYAN: &str = "\x1b[96m";
pub const BRIGHT_WHITE: &str = "\x1b[97m";

// Background colors
pub const BG_DARK: &str = "\x1b[48;5;235m";
pub const BG_BLUE: &str = "\x1b[44m";
pub const BG_GREEN: &str = "\x1b[42m";
pub const BG_YELLOW: &str = "\x1b[43m";
pub const BG_RED: &str = "\x1b[41m";

pub const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

#[derive(Debug, Clone)]
pub struct TaskLiveState {
    pub label: String,
    pub harness: String,
    pub model: String,
    pub start_time: Instant,
    pub raw_event: Option<String>,
    pub event_lines: Vec<String>,
}

pub fn format_event_preview(raw: &str, max_lines: usize, max_cols: usize) -> Vec<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut result_lines = Vec::new();

    if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Ok(pretty) = serde_json::to_string_pretty(&json_val) {
            for line in pretty.lines() {
                let char_count = line.chars().count();
                if char_count > max_cols {
                    let s: String = line.chars().take(max_cols.saturating_sub(3)).collect();
                    result_lines.push(format!("{s}..."));
                } else {
                    result_lines.push(line.to_string());
                }
            }
        }
    } else {
        for line in trimmed.lines() {
            let char_count = line.chars().count();
            if char_count > max_cols {
                let s: String = line.chars().take(max_cols.saturating_sub(3)).collect();
                result_lines.push(format!("{s}..."));
            } else {
                result_lines.push(line.to_string());
            }
        }
    }

    if result_lines.len() > max_lines {
        let total = result_lines.len();
        result_lines.truncate(max_lines.saturating_sub(1));
        let remaining = total - result_lines.len();
        result_lines.push(format!("... (+{remaining} more lines)"));
    }

    result_lines
}

pub struct UI {
    pub enable_color: bool,
    last_ticker_len: usize,
    last_live_lines: usize,
    spinner_idx: usize,
    active_tasks: BTreeMap<String, TaskLiveState>,
}

impl UI {
    pub fn new(enable_color: Option<bool>) -> Self {
        let ec = enable_color.unwrap_or_else(|| {
            let no_color = std::env::var("NO_COLOR").is_ok();
            let is_tty = io::stdout().is_terminal();
            is_tty && !no_color
        });
        Self {
            enable_color: ec,
            last_ticker_len: 0,
            last_live_lines: 0,
            spinner_idx: 0,
            active_tasks: BTreeMap::new(),
        }
    }

    pub fn color(&self, text: &str, codes: &[&str]) -> String {
        if !self.enable_color {
            return text.to_string();
        }
        format!("{}{}{}", codes.join(""), text, RESET)
    }

    fn clear_ticker(&mut self) {
        if self.last_ticker_len > 0 && self.enable_color {
            print!("\r{}\r", " ".repeat(self.last_ticker_len));
            let _ = io::stdout().flush();
            self.last_ticker_len = 0;
        }
    }

    pub fn clear_live(&mut self) {
        if self.last_live_lines > 0 && self.enable_color {
            for _ in 0..self.last_live_lines {
                print!("\x1b[2K\x1b[1A");
            }
            print!("\x1b[2K\r");
            let _ = io::stdout().flush();
            self.last_live_lines = 0;
        }
        self.clear_ticker();
    }

    pub fn update_live_task(
        &mut self,
        task_key: &str,
        label: &str,
        harness: &str,
        model: &str,
        start_time: Instant,
        raw_event: Option<&str>,
    ) {
        let event_lines = match raw_event {
            Some(r) => format_event_preview(r, 10, 80),
            None => Vec::new(),
        };

        self.active_tasks.insert(
            task_key.to_string(),
            TaskLiveState {
                label: label.to_string(),
                harness: harness.to_string(),
                model: model.to_string(),
                start_time,
                raw_event: raw_event.map(|s| s.to_string()),
                event_lines,
            },
        );

        self.render_live();
    }

    pub fn clear_live_task(&mut self, task_key: &str) {
        self.active_tasks.remove(task_key);
        if self.active_tasks.is_empty() {
            self.clear_live();
        } else {
            self.render_live();
        }
    }

    pub fn render_live(&mut self) {
        if !self.enable_color || self.active_tasks.is_empty() {
            return;
        }

        if self.last_live_lines > 0 {
            for _ in 0..self.last_live_lines {
                print!("\x1b[2K\x1b[1A");
            }
            print!("\x1b[2K\r");
        }

        self.spinner_idx = (self.spinner_idx + 1) % SPINNER_FRAMES.len();
        let frame = SPINNER_FRAMES[self.spinner_idx];
        let mut lines = Vec::new();

        for task in self.active_tasks.values() {
            let elapsed = task.start_time.elapsed().as_secs();
            let spinner = self.color(frame, &[BOLD, CYAN]);
            let label = self.color(&format!("[{}]", task.label), &[BOLD, WHITE]);
            let target = self.color(&format!("{} ({})", task.harness, task.model), &[BRIGHT_CYAN]);
            let timer = self.color(&format!("[{}s]", elapsed), &[DIM, WHITE]);

            lines.push(format!("  {spinner} {label} {target} {timer}"));
            for eline in &task.event_lines {
                let bar = self.color("│", &[DIM, CYAN]);
                let dim_text = self.color(eline, &[DIM, WHITE]);
                lines.push(format!("    {bar} {dim_text}"));
            }
        }

        for l in &lines {
            println!("{l}");
        }
        let _ = io::stdout().flush();
        self.last_live_lines = lines.len();
    }

    pub fn print_line(&mut self, text: &str) {
        self.clear_live();
        println!("{text}");
        let _ = io::stdout().flush();
        if !self.active_tasks.is_empty() {
            self.render_live();
        }
    }

    // ----------------------------------------------------------- Cards & Boxes

    pub fn banner(
        &mut self,
        title: &str,
        subtitle: Option<&str>,
        meta: Option<&[(&str, &str)]>,
        _width: usize,
    ) {
        self.clear_ticker();
        let c_border = CYAN;
        let c_title = &[BOLD, BRIGHT_WHITE];
        let c_sub = &[DIM, WHITE];
        let c_key = CYAN;
        let c_val = BRIGHT_CYAN;

        let top = self.color("╭── ", &[c_border]);
        let top_title = self.color(title, c_title);
        let top_bar = self.color(" ───────────────────────────────────────────────────", &[c_border]);
        let v = self.color("│", &[c_border]);
        let bot = self.color("╰─────────────────────────────────────────────────────────────", &[c_border]);

        let mut lines = vec![format!("{top}{top_title}{top_bar}")];

        if let Some(sub) = subtitle {
            let sub_colored = self.color(sub, c_sub);
            lines.push(format!("{v}  {sub_colored}"));
        }

        if let Some(meta_items) = meta {
            lines.push(v.to_string());
            for (k, val) in meta_items {
                let k_col = self.color(k, &[c_key]);
                let val_col = self.color(val, &[c_val]);
                lines.push(format!("{v}  • {k_col}: {val_col}"));
            }
        }

        lines.push(bot);
        self.print_line(&lines.join("\n"));
    }

    pub fn phase_card(
        &mut self,
        phase: &str,
        wave: usize,
        detail: Option<&str>,
        _width: usize,
    ) {
        self.clear_ticker();
        let color = match phase {
            "INIT" => BRIGHT_BLUE,
            "PLAN" => BRIGHT_MAGENTA,
            "PLAN_CHECKPOINT" => YELLOW,
            "STAGES" => BRIGHT_BLUE,
            "IMPLEMENT" => BRIGHT_CYAN,
            "INSPECT" => CYAN,
            "INTEGRATE" => BLUE,
            "GATES" => YELLOW,
            "REVIEW" => MAGENTA,
            "JUDGE" => BRIGHT_MAGENTA,
            "PLAN_FIX" => BRIGHT_YELLOW,
            "POLISH" => GREEN,
            "DELIVER_CHECKPOINT" => YELLOW,
            "DELIVER" => BRIGHT_GREEN,
            "READY" | "READY_NO_CHANGE" => BRIGHT_GREEN,
            "BLOCKED" | "BLOCKED_CONVERGENCE" | "BLOCKED_ARCHITECTURE" | "BLOCKED_GATE" | "BLOCKED_HARNESS" => BRIGHT_RED,
            _ => WHITE,
        };

        let wave_str = if wave > 0 || matches!(phase, "IMPLEMENT" | "PLAN_FIX" | "REVIEW" | "JUDGE") {
            format!(" [Wave {wave}]")
        } else {
            String::new()
        };

        let icon = match phase {
            "INIT" => "⚙️ ",
            "PLAN" => "🗺️ ",
            "PLAN_CHECKPOINT" => "⏸️ ",
            "STAGES" => "📦",
            "IMPLEMENT" => "⚡",
            "INSPECT" => "🔍",
            "INTEGRATE" => "🔄",
            "GATES" => "🛡️ ",
            "REVIEW" => "🧐",
            "JUDGE" => "⚖️ ",
            "PLAN_FIX" => "🔧",
            "POLISH" => "✨",
            "DELIVER_CHECKPOINT" => "⏸️ ",
            "DELIVER" => "🚀",
            "READY" | "READY_NO_CHANGE" => "🎉",
            _ => "◈",
        };

        let title = format!("{icon}  PHASE: {phase}{wave_str}");
        let title_colored = self.color(&title, &[BOLD, color]);
        let line = self.color(" ───────────────────────────────────────────────────", &[DIM, color]);
        let prefix = self.color("╭──", &[color]);

        self.print_line(&format!("\n{prefix} {title_colored}{line}"));
        if let Some(det) = detail {
            let v = self.color("│", &[color]);
            let det_col = self.color(det, &[DIM, WHITE]);
            self.print_line(&format!("{v}  {det_col}"));
        }
    }

    pub fn stage_header(
        &mut self,
        index: usize,
        total: usize,
        slug: &str,
        brief: &str,
        _width: usize,
    ) {
        self.clear_ticker();
        let badge = format!("📦 STAGE [{index}/{total}]: {slug}");
        let badge_colored = self.color(&badge, &[BOLD, BRIGHT_CYAN]);
        let c_line = self.color(" ───────────────────────────────────────────────────", &[DIM, BRIGHT_BLUE]);
        let corner_tl = self.color("╭──", &[BRIGHT_BLUE]);
        let v = self.color("│", &[BRIGHT_BLUE]);

        self.print_line(&format!("\n{corner_tl} {badge_colored}{c_line}"));
        if !brief.is_empty() {
            let brief_col = self.color(brief, &[WHITE]);
            self.print_line(&format!("{v}  {brief_col}"));
        }
    }

    // --------------------------------------------------------- Progress & Logs

    pub fn info(&mut self, message: &str) {
        self.clear_ticker();
        let icon = self.color("▶", &[BOLD, BRIGHT_BLUE]);
        self.print_line(&format!("  {icon} {message}"));
    }

    pub fn subitem(&mut self, text: &str) {
        self.clear_ticker();
        let dot = self.color("•", &[DIM, WHITE]);
        self.print_line(&format!("    {dot} {text}"));
    }

    pub fn step(&mut self, label: &str, message: &str, detail: &str) {
        self.clear_ticker();
        let badge = self.color(&format!("[{label}]"), &[BOLD, CYAN]);
        let det = if !detail.is_empty() {
            format!(" {}", self.color(&format!("({detail})"), &[DIM]))
        } else {
            String::new()
        };
        self.print_line(&format!(" {badge} {message}{det}"));
    }

    pub fn success(&mut self, message: &str, detail: &str) {
        self.clear_ticker();
        let icon = self.color("✔", &[BOLD, BRIGHT_GREEN]);
        let det = if !detail.is_empty() {
            format!(" {}", self.color(&format!("({detail})"), &[DIM]))
        } else {
            String::new()
        };
        self.print_line(&format!("  {icon} {message}{det}"));
    }

    pub fn warning(&mut self, message: &str, detail: &str) {
        self.clear_ticker();
        let icon = self.color("⚠", &[BOLD, BRIGHT_YELLOW]);
        let det = if !detail.is_empty() {
            format!(" {}", self.color(&format!("({detail})"), &[DIM]))
        } else {
            String::new()
        };
        let msg = self.color(message, &[YELLOW]);
        self.print_line(&format!("  {icon} {msg}{det}"));
    }

    pub fn error(&mut self, message: &str, detail: &str) {
        self.clear_ticker();
        let icon = self.color("✖", &[BOLD, BRIGHT_RED]);
        let det = if !detail.is_empty() {
            format!("\n    {}", self.color(detail, &[RED]))
        } else {
            String::new()
        };
        let msg = self.color(message, &[BRIGHT_RED]);
        self.print_line(&format!("  {icon} {msg}{det}"));
    }

    pub fn gate_result(
        &mut self,
        index: usize,
        total: usize,
        command: &str,
        ok: bool,
        duration_s: f64,
        detail: &str,
    ) {
        self.clear_ticker();
        let idx_str = self.color(&format!("[{index}/{total}]"), &[DIM]);
        let dur = self.color(&format!("({duration_s:.1}s)"), &[DIM]);
        if ok {
            let status = self.color("✔ PASS", &[BOLD, BRIGHT_GREEN]);
            self.print_line(&format!("  {idx_str} {command:<48} {status} {dur}"));
        } else {
            let status = self.color("✖ FAIL", &[BOLD, BRIGHT_RED]);
            self.print_line(&format!("  {idx_str} {command:<48} {status} {dur}"));
            if !detail.is_empty() {
                let det = self.color(&format!("↳ {detail}"), &[RED]);
                self.print_line(&format!("      {det}"));
            }
        }
    }

    // ------------------------------------------------------------- Live Ticker

    #[allow(clippy::too_many_arguments)]
    pub fn ticker(
        &mut self,
        role: &str,
        harness: &str,
        model: Option<&str>,
        lane_id: Option<&str>,
        elapsed_s: f64,
        bytes_count: usize,
        idle_s: Option<f64>,
        status_text: &str,
    ) {
        if !self.enable_color {
            return;
        }

        let frame = SPINNER_FRAMES[self.spinner_idx % SPINNER_FRAMES.len()];
        self.spinner_idx += 1;

        let total_secs = elapsed_s as u64;
        let mins = total_secs / 60;
        let secs = total_secs % 60;
        let time_str = format!("{mins:02}:{secs:02}");

        let mut harness_label = harness.to_string();
        if let Some(m) = model {
            harness_label.push(':');
            harness_label.push_str(m);
        }

        let lane_str = lane_id.map(|l| format!(" [{l}]")).unwrap_or_default();
        let size_kb = bytes_count as f64 / 1024.0;
        let size_str = if size_kb > 0.0 {
            format!("{size_kb:.1} KB out")
        } else {
            "starting".to_string()
        };

        let idle_str = match idle_s {
            Some(i) if i > 0.0 => format!(", active {}s ago", i as u64),
            _ => String::new(),
        };

        let frame_col = self.color(frame, &[BOLD, BRIGHT_CYAN]);
        let role_col = self.color(&format!("[{}{lane_str}]", role.to_uppercase()), &[BOLD, CYAN]);
        let h_col = self.color(&harness_label, &[BRIGHT_WHITE]);
        let time_col = self.color(&time_str, &[YELLOW]);
        let stat_col = self.color(&format!("{size_str}{idle_str}"), &[DIM]);

        let mut line = format!(
            "\r {frame_col} {role_col} {h_col} • {time_col} • {stat_col}"
        );
        if !status_text.is_empty() {
            let st_col = self.color(status_text, &[DIM]);
            line.push_str(&format!(" • {st_col}"));
        }

        let vis_len = format!(" [{role}{lane_str}] {harness_label} • {time_str} • {size_str}{idle_str}").len() + 4;
        let padding = self.last_ticker_len.saturating_sub(vis_len);
        print!("{line}{}", " ".repeat(padding));
        let _ = io::stdout().flush();
        self.last_ticker_len = vis_len;
    }

    pub fn finish_ticker(&mut self, message: Option<&str>) {
        if self.last_ticker_len > 0 {
            self.clear_ticker();
            if let Some(msg) = message {
                self.success(msg, "");
            }
        }
    }

    // ---------------------------------------------------------- Verdicts Table

    pub fn verdicts_table(&mut self, groups: &[ClaimGroup], width: usize) {
        self.clear_ticker();
        if groups.is_empty() {
            self.success("No claims or defects found in review (NO_CLAIMS). Candidate is clean.", "");
            return;
        }

        let title = format!("╭─ ⚖️  JUDGMENT VERDICTS ({} group(s)) ─", groups.len());
        let line_len = width.saturating_sub(title.len() + 1);
        let header = self.color(&format!("{title}{}╮", "─".repeat(line_len)), &[MAGENTA]);
        self.print_line(&format!("\n{header}"));

        let border = self.color("│", &[MAGENTA]);

        for (idx, g) in groups.iter().enumerate() {
            let num = idx + 1;
            let v_badge = match g.verdict.as_str() {
                "FIX" => self.color(" FIX ", &[BOLD, WHITE, BG_RED]),
                "REDESIGN" => self.color(" REDESIGN ", &[BOLD, WHITE, BG_RED]),
                "REPORT_ONLY" => self.color(" REPORT_ONLY ", &[BOLD, WHITE, BG_BLUE]),
                "DISMISS" => self.color(" DISMISS ", &[DIM]),
                other => self.color(&format!(" {other} "), &[BOLD]),
            };

            let root_col = self.color(&g.root_cause, &[BOLD, WHITE]);
            let idx_col = self.color(&format!("#{num}"), &[BOLD]);
            self.print_line(&format!("{border} {idx_col} {v_badge} {root_col}"));

            let class_col = self.color(&g.defect_class, &[CYAN]);
            let owns_display = if g.owns.is_empty() { "N/A" } else { &g.owns };
            let owns_col = self.color(owns_display, &[YELLOW]);
            let cids_display = if g.contract_ids.is_empty() {
                "None".to_string()
            } else {
                g.contract_ids.join(", ")
            };
            let cids_col = self.color(&cids_display, &[DIM]);
            self.print_line(&format!("{border}    • Class: {class_col} | Owner: {owns_col} | Contract: {cids_col}"));

            if !g.fix.is_empty() {
                let fix_col = self.color(&g.fix, &[DIM]);
                self.print_line(&format!("{border}    • Proposed fix: {fix_col}"));
            }

            for c in g.claims.iter().take(2) {
                let c_col = self.color(c, &[DIM]);
                self.print_line(&format!("{border}      - {c_col}"));
            }
            if g.claims.len() > 2 {
                let more_count = g.claims.len() - 2;
                let more_col = self.color(&format!("... +{more_count} more claims"), &[DIM]);
                self.print_line(&format!("{border}      - {more_col}"));
            }
        }

        let bot = self.color(&format!("╰{}╯", "─".repeat(width.saturating_sub(2))), &[MAGENTA]);
        self.print_line(&format!("{bot}\n"));
    }
}

static GLOBAL_UI: OnceLock<Mutex<UI>> = OnceLock::new();

pub fn default_ui() -> &'static Mutex<UI> {
    GLOBAL_UI.get_or_init(|| Mutex::new(UI::new(None)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_banner_renders_without_color() {
        let mut ui = UI::new(Some(false));
        ui.banner(
            "GAUNTLET MISSION • test-slug",
            Some("Testing UI"),
            Some(&[("Repository", "/path/to/repo"), ("Lanes", "1")]),
            76,
        );
    }

    #[test]
    fn test_phase_card_renders() {
        let mut ui = UI::new(Some(false));
        ui.phase_card("IMPLEMENT", 1, Some("Testing phase"), 76);
    }

    #[test]
    fn test_gate_result_renders() {
        let mut ui = UI::new(Some(false));
        ui.gate_result(1, 5, "npm test", true, 2.3, "");
        ui.gate_result(2, 5, "npm test fail", false, 1.1, "exit code 1");
    }

    #[test]
    fn test_verdicts_table_renders() {
        let mut ui = UI::new(Some(false));
        let groups = vec![ClaimGroup {
            root_cause: "Missing null check in token parser".to_string(),
            claims: vec!["Crash when token is empty".to_string()],
            contract_ids: vec!["AC-1".to_string()],
            verdict: "FIX".to_string(),
            fix: "Add early return if token is None".to_string(),
            owns: "lib/auth.js".to_string(),
            defect_class: "code_defect".to_string(),
        }];
        ui.verdicts_table(&groups, 76);
    }
}
