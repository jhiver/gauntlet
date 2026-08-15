use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;

use gauntlet::orchestrator::Orchestrator;
use gauntlet::statemachine::load;
use gauntlet::worktrees::{create_worktree, ensure_symlinks, Git};

mod helpers;
use helpers::{make_git_repo, write_echo_config, write_mission};

#[test]
fn test_create_worktree_multi_language_symlinks() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));

    // Create multi-language dependency structures in the root repository
    let venv_dir = repo.join(".venv").join("bin");
    fs::create_dir_all(&venv_dir).unwrap();
    fs::write(venv_dir.join("activate"), "#!/bin/sh\nexport VIRTUAL_ENV=.venv").unwrap();

    let node_dir = repo.join("node_modules").join("my-package");
    fs::create_dir_all(&node_dir).unwrap();
    fs::write(node_dir.join("index.js"), "module.exports = 'test';").unwrap();

    let cache_dir = repo.join("custom_cache");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("cache.dat"), "cache-data-123").unwrap();

    let vendor_dir = repo.join("vendor").join("bundle");
    fs::create_dir_all(&vendor_dir).unwrap();
    fs::write(vendor_dir.join("config"), "vendor-config").unwrap();

    let git = Git::new(false);
    let wt = tmp.path().join("wt-multi-lang");
    let symlinks = vec![
        ".venv".to_string(),
        "node_modules".to_string(),
        "custom_cache".to_string(),
        "vendor/bundle".to_string(),
    ];

    create_worktree(&git, &repo, &wt, "test-branch-1", "main", &symlinks).unwrap();

    // Verify all symlinks exist in the worktree
    assert!(wt.join(".venv").exists(), ".venv symlink must exist in worktree");
    assert!(wt.join("node_modules").exists(), "node_modules symlink must exist in worktree");
    assert!(wt.join("custom_cache").exists(), "custom_cache symlink must exist in worktree");
    assert!(wt.join("vendor/bundle").exists(), "vendor/bundle symlink must exist in worktree");

    // Verify file contents read through symlinks match root repository
    let act_content = fs::read_to_string(wt.join(".venv").join("bin").join("activate")).unwrap();
    assert!(act_content.contains("VIRTUAL_ENV=.venv"));

    let js_content = fs::read_to_string(wt.join("node_modules").join("my-package").join("index.js")).unwrap();
    assert_eq!(js_content, "module.exports = 'test';");

    let cache_content = fs::read_to_string(wt.join("custom_cache").join("cache.dat")).unwrap();
    assert_eq!(cache_content, "cache-data-123");

    let vendor_content = fs::read_to_string(wt.join("vendor").join("bundle").join("config")).unwrap();
    assert_eq!(vendor_content, "vendor-config");

    #[cfg(unix)]
    {
        assert!(fs::symlink_metadata(wt.join(".venv")).unwrap().file_type().is_symlink());
        assert!(fs::symlink_metadata(wt.join("node_modules")).unwrap().file_type().is_symlink());
        assert!(fs::symlink_metadata(wt.join("custom_cache")).unwrap().file_type().is_symlink());
        assert!(fs::symlink_metadata(wt.join("vendor/bundle")).unwrap().file_type().is_symlink());
    }
}

#[test]
fn test_create_worktree_failsafe_on_missing_symlink_sources() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));

    // Only create .venv, but declare node_modules and custom_cache as well
    let venv_dir = repo.join(".venv");
    fs::create_dir_all(&venv_dir).unwrap();
    fs::write(venv_dir.join("pyvenv.cfg"), "home = /usr/bin").unwrap();

    let git = Git::new(false);
    let wt = tmp.path().join("wt-failsafe");
    let symlinks = vec![
        ".venv".to_string(),
        "node_modules".to_string(),
        "nonexistent/cache/dir".to_string(),
    ];

    // Creation should succeed gracefully without failing or crashing
    let res = create_worktree(&git, &repo, &wt, "test-branch-2", "main", &symlinks);
    assert!(res.is_ok(), "create_worktree must succeed even if some symlink sources are missing");

    assert!(wt.join(".venv").exists());
    assert!(!wt.join("node_modules").exists());
    assert!(!wt.join("nonexistent/cache/dir").exists());
}

#[test]
fn test_create_worktree_does_not_overwrite_existing_destination() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));

    let cache_dir = repo.join("custom_cache");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("file.txt"), "repo-file").unwrap();

    let git = Git::new(false);
    let wt = tmp.path().join("wt-preserve");
    create_worktree(&git, &repo, &wt, "test-branch-3", "main", &[]).unwrap();

    // Pre-create destination in worktree
    let wt_cache = wt.join("custom_cache");
    fs::create_dir_all(&wt_cache).unwrap();
    fs::write(wt_cache.join("file.txt"), "worktree-file").unwrap();

    // Now run ensure_symlinks
    ensure_symlinks(&repo, &wt, &[ "custom_cache".to_string() ]);

    // The existing worktree directory/file must not have been overwritten
    let content = fs::read_to_string(wt.join("custom_cache").join("file.txt")).unwrap();
    assert_eq!(content, "worktree-file");
}

#[test]
fn test_orchestrator_integration_with_declarative_symlinks() {
    let tmp = tempdir().unwrap();
    let repo = make_git_repo(&tmp.path().join("repo"));

    // Set up repository dependency directories
    let venv_dir = repo.join(".venv");
    fs::create_dir_all(&venv_dir).unwrap();
    fs::write(venv_dir.join("pyvenv.cfg"), "python").unwrap();

    let cache_dir = repo.join("custom_cache");
    fs::create_dir_all(&cache_dir).unwrap();
    fs::write(cache_dir.join("cache.dat"), "cache-data").unwrap();

    // Config containing [worktree] table with symlinks
    let config_path = tmp.path().join("config.toml");
    let echo_base = write_echo_config(&tmp.path().join("echo.toml"));
    let mut config_str = fs::read_to_string(&echo_base).unwrap();
    config_str.push_str("\n[worktree]\nsymlinks = [\".venv\", \"custom_cache\"]\n");
    fs::write(&config_path, config_str).unwrap();

    let mission = write_mission(&tmp.path().join("m.md"), &repo, "symlinks-mission", None, None);

    let logs = Arc::new(Mutex::new(Vec::new()));
    let logs_clone = Arc::clone(&logs);
    let log_fn = Arc::new(move |msg: &str| {
        logs_clone.lock().unwrap().push(msg.to_string());
    });

    let tool_dir = PathBuf::from(".");
    let mut orch = Orchestrator::new(
        &tool_dir,
        Some(&mission),
        None,
        Some(&config_path),
        true,
        false,
        None,
        false,
        0,
        2,
        Some(log_fn),
    )
    .unwrap();

    assert_eq!(orch.worktree_symlinks(), vec![".venv".to_string(), "custom_cache".to_string()]);

    let rc = orch.run();
    assert_eq!(rc, 0);

    let state = load(orch.run_dir.as_ref().unwrap()).unwrap();
    assert_eq!(state.phase, "READY");
}
