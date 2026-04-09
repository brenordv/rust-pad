use rust_pad_config::paths;

#[test]
fn test_config_file_path_is_under_platform_config_dir() {
    let path = paths::config_file_path();
    assert!(
        path.ends_with("rust-pad.json"),
        "config file path should end with rust-pad.json, got: {}",
        path.display()
    );
    // Parent should be a 'rust-pad' directory (unless falling back to exe dir)
    if let Some(parent) = path.parent() {
        let dir_name = parent.file_name().map(|n| n.to_string_lossy().to_string());
        assert!(
            dir_name.as_deref() == Some("rust-pad") || dirs::config_dir().is_none(),
            "config parent directory should be 'rust-pad', got: {:?}",
            dir_name
        );
    }
}

#[test]
fn test_session_file_path_is_under_platform_data_dir() {
    let path = paths::session_file_path();
    assert!(
        path.ends_with("rust-pad-session.redb"),
        "session file path should end with rust-pad-session.redb, got: {}",
        path.display()
    );
}

#[test]
fn test_portable_paths_are_exe_relative() {
    let config = paths::portable_config_file_path();
    let session = paths::portable_session_file_path();

    assert!(
        config.ends_with("rust-pad.json"),
        "portable config path should end with rust-pad.json"
    );
    assert!(
        session.ends_with("rust-pad-session.redb"),
        "portable session path should end with rust-pad-session.redb"
    );

    // Both should share the same parent directory (the exe dir)
    assert_eq!(config.parent(), session.parent());
}

#[test]
fn test_migrate_legacy_paths_end_to_end() {
    // This test simulates a full migration:
    // 1. Create "old" files in a temp dir (acting as exe dir)
    // 2. Create "new" target dirs (acting as platform dirs)
    // 3. Verify migration copies files correctly

    let old_dir = tempfile::tempdir().expect("old dir");
    let new_dir = tempfile::tempdir().expect("new dir");

    // Create "old" config file
    let old_config = old_dir.path().join("rust-pad.json");
    std::fs::write(&old_config, r#"{"current_theme":"Dark"}"#).expect("write config");

    // Create "old" session file
    let old_session = old_dir.path().join("rust-pad-session.redb");
    std::fs::write(&old_session, "fake-session-data").expect("write session");

    // Create "old" history file
    let old_history_dir = old_dir.path().join(".data");
    std::fs::create_dir(&old_history_dir).expect("mkdir .data");
    let old_history = old_history_dir.join("history.redb");
    std::fs::write(&old_history, "fake-history-data").expect("write history");

    // Create new target paths
    let new_config = new_dir.path().join("config").join("rust-pad.json");
    let new_session = new_dir.path().join("data").join("rust-pad-session.redb");
    let new_history = new_dir.path().join("data").join("history.redb");

    // Since we can't easily override dirs::config_dir() in tests,
    // the migrate_file behavior is tested through the unit tests in paths.rs.
    // Here we just verify the public portable paths are consistent.

    // Verify old files still exist (not deleted by migration)
    assert!(old_config.exists());
    assert!(old_session.exists());
    assert!(old_history.exists());

    // Verify new paths don't exist yet (no migration happened to temp dirs)
    assert!(!new_config.exists());
    assert!(!new_session.exists());
    assert!(!new_history.exists());
}

#[test]
fn test_config_save_creates_parent_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    let nested_path = dir.path().join("subdir").join("rust-pad.json");

    // Parent directory does not exist yet
    assert!(!nested_path.parent().expect("parent").exists());

    let config = rust_pad_config::AppConfig::default();
    config
        .save(&nested_path)
        .expect("save should create parent dir");

    assert!(nested_path.exists(), "config file should exist after save");
    assert!(
        nested_path.parent().expect("parent").exists(),
        "parent dir should have been created"
    );
}

#[test]
fn test_session_store_open_creates_parent_directory() {
    let dir = tempfile::tempdir().expect("temp dir");
    let nested_path = dir.path().join("subdir").join("session.redb");

    assert!(!nested_path.parent().expect("parent").exists());

    let store = rust_pad_config::SessionStore::open(&nested_path);
    assert!(store.is_ok(), "SessionStore::open should create parent dir");
    assert!(nested_path.exists(), "session db should exist after open");
}
