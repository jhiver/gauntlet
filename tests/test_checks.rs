use gauntlet::worktrees::{
    check_claimed_vs_diff, check_lane_diff, checkout_drift, find_overlaps, glob_matches,
    globs_may_overlap, static_prefix, LaneOverlap,
};

#[test]
fn test_glob_matching() {
    assert!(glob_matches("src/**", "src/auth/login.rs"));
    assert!(glob_matches("src/lib.rs", "src/lib.rs"));
    assert!(!glob_matches("src/*.rs", "src/auth/login.rs"));
    assert!(glob_matches("src/*.rs", "src/lib.rs"));
    assert!(glob_matches("src/test_?.rs", "src/test_1.rs"));
    assert!(!glob_matches("src/test_?.rs", "src/test_12.rs"));
}

#[test]
fn test_static_prefix() {
    assert_eq!(static_prefix("src/auth/**"), "src/auth");
    assert_eq!(static_prefix("src/lib.rs"), "src/lib.rs");
    assert_eq!(static_prefix("**"), "");
}

#[test]
fn test_glob_overlaps() {
    assert!(globs_may_overlap("src/**", "src/auth/**", &[]));
    assert!(!globs_may_overlap("src/auth/**", "src/ui/**", &[]));

    let lanes = vec![
        LaneOverlap::new("L1", vec!["src/auth/**".to_string()]),
        LaneOverlap::new("L2", vec!["src/ui/**".to_string()]),
        LaneOverlap::new("L3", vec!["src/**".to_string()]),
    ];

    let overlaps = find_overlaps(&lanes, &[]);
    assert_eq!(overlaps.len(), 2); // (L1, L3) and (L2, L3)
}

#[test]
fn test_lane_diff_checks() {
    let changed = vec!["src/auth/login.rs".to_string(), "config.json".to_string()];
    let owns = vec!["src/auth/**".to_string()];
    let forbidden = vec!["src/auth/secret.rs".to_string()];

    let violations = check_lane_diff(&changed, &owns, &forbidden);
    assert_eq!(violations.len(), 1);
    assert!(violations[0].contains("config.json"));

    let claimed = vec!["src/auth/login.rs".to_string(), "src/auth/logout.rs".to_string()];
    let misses = check_claimed_vs_diff(&claimed, &changed);
    assert_eq!(misses.len(), 1);
    assert!(misses[0].contains("src/auth/logout.rs"));
}

#[test]
fn test_checkout_drift() {
    let before = vec!["a.txt".to_string(), ".missions/1.log".to_string()];
    let after = vec![
        "a.txt".to_string(),
        ".missions/1.log".to_string(),
        ".missions/2.log".to_string(),
        "rogue.txt".to_string(),
    ];

    let drift = checkout_drift(&before, &after, Some(&[".missions/"]));
    assert_eq!(drift, vec!["rogue.txt"]);
}
