use gauntlet::ui::UI;
use gauntlet::verdicts::ClaimGroup;

#[test]
fn test_banner_renders_cleanly() {
    let mut ui = UI::new(Some(false));
    ui.banner(
        "GAUNTLET MISSION • test-slug",
        Some("Testing UI"),
        Some(&[("Repository", "/path/to/repo"), ("Lanes", "1")]),
        76,
    );
}

#[test]
fn test_phase_card() {
    let mut ui = UI::new(Some(false));
    ui.phase_card("IMPLEMENT", 1, Some("Testing phase"), 76);
}

#[test]
fn test_gate_result() {
    let mut ui = UI::new(Some(false));
    ui.gate_result(1, 5, "npm test", true, 2.3, "");
}

#[test]
fn test_verdicts_table() {
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

#[test]
fn test_format_event_preview_truncation() {
    use gauntlet::ui::format_event_preview;

    // Small JSON
    let raw_json = r#"{"type":"tool_call","name":"edit","input":{"path":"src/main.rs"}}"#;
    let preview = format_event_preview(raw_json, 10, 80);
    assert!(!preview.is_empty());
    assert!(preview.iter().any(|l| l.contains("tool_call")));

    // Long line truncation (> 40 cols)
    let long_line_json = r#"{"msg":"This is a very very very long text line that will exceed the maximum allowed width of forty columns definitely"}"#;
    let preview_trunc = format_event_preview(long_line_json, 10, 40);
    assert!(preview_trunc.iter().any(|l| l.ends_with("...")));

    // Multi-line truncation (> 5 lines)
    let multi_line_raw = (0..20).map(|i| format!("Line #{i}")).collect::<Vec<_>>().join("\n");
    let preview_lines = format_event_preview(&multi_line_raw, 5, 80);
    assert_eq!(preview_lines.len(), 5);
    assert!(preview_lines.last().unwrap().contains("+16 more lines"));
}

#[test]
fn test_live_task_updates_and_cleanup() {
    let mut ui = UI::new(Some(false));
    let start = std::time::Instant::now();
    ui.update_live_task("L1", "L1", "cmd", "claude-sonnet-5", start, Some(r#"{"action":"reading"}"#));
    ui.update_live_task("L2", "L2", "cmd", "claude-sonnet-5", start, None);
    ui.clear_live_task("L1");
    ui.clear_live_task("L2");
    ui.clear_live();
}
