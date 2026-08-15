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
