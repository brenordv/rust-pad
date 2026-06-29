use rust_pad_config::session::{generate_session_id, SessionData, SessionStore, SessionTabEntry};

/// Builds an `Unsaved` tab entry with default pin/colour.
fn unsaved(id: &str, title: &str) -> SessionTabEntry {
    SessionTabEntry::Unsaved {
        session_id: id.to_string(),
        title: title.to_string(),
        pinned: false,
        tab_color: None,
    }
}

/// Builds a `SessionData` listing the given unsaved session ids, mirroring
/// what `build_session_snapshot` produces so the content pairs and meta agree.
fn unsaved_meta(ids: &[&str]) -> SessionData {
    SessionData {
        tabs: ids.iter().map(|id| unsaved(id, "Untitled")).collect(),
        active_tab_index: 0,
        split: None,
    }
}

/// Convenience: `(id, text)` pair as owned strings.
fn pair(id: &str, text: &str) -> (String, String) {
    (id.to_string(), text.to_string())
}

#[test]
fn test_session_store_save_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    let data = SessionData {
        tabs: vec![
            SessionTabEntry::File {
                path: "/home/user/file.rs".to_string(),
                pinned: false,
                tab_color: None,
            },
            unsaved("sess-0", "Untitled"),
        ],
        active_tab_index: 1,
        split: None,
    };

    store
        .save_snapshot(&data, &[pair("sess-0", "")], true)
        .unwrap();

    // Close and reopen the database
    drop(store);
    let store2 = SessionStore::open(&db_path).unwrap();
    let loaded = store2.load_session().unwrap().unwrap();

    assert_eq!(loaded.tabs.len(), 2);
    assert_eq!(loaded.active_tab_index, 1);
    match &loaded.tabs[0] {
        SessionTabEntry::File { path, .. } => assert_eq!(path, "/home/user/file.rs"),
        _ => panic!("expected File entry"),
    }
}

#[test]
fn test_session_store_content_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");

    let store = SessionStore::open(&db_path).unwrap();
    store
        .save_snapshot(
            &unsaved_meta(&["tab-1", "tab-2"]),
            &[
                pair("tab-1", "Hello, world!"),
                pair("tab-2", "Second tab content"),
            ],
            false,
        )
        .unwrap();
    drop(store);

    let store2 = SessionStore::open(&db_path).unwrap();
    assert_eq!(
        store2.load_content("tab-1").unwrap().unwrap(),
        "Hello, world!"
    );
    assert_eq!(
        store2.load_content("tab-2").unwrap().unwrap(),
        "Second tab content"
    );
}

#[test]
fn test_session_store_overwrite_session() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    // Save first session
    let data1 = SessionData {
        tabs: vec![SessionTabEntry::File {
            path: "old.rs".to_string(),
            pinned: false,
            tab_color: None,
        }],
        active_tab_index: 0,
        split: None,
    };
    store.save_snapshot(&data1, &[], false).unwrap();

    // Overwrite with new session
    let data2 = SessionData {
        tabs: vec![
            SessionTabEntry::File {
                path: "new1.rs".to_string(),
                pinned: false,
                tab_color: None,
            },
            SessionTabEntry::File {
                path: "new2.rs".to_string(),
                pinned: false,
                tab_color: None,
            },
        ],
        active_tab_index: 1,
        split: None,
    };
    store.save_snapshot(&data2, &[], false).unwrap();

    let loaded = store.load_session().unwrap().unwrap();
    assert_eq!(loaded.tabs.len(), 2);
    assert_eq!(loaded.active_tab_index, 1);
}

#[test]
fn test_session_store_unicode_content() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    let unicode_content = "日本語テスト 🦀🎉\nrüstig héllo\n\ttab\tindented\n";
    store
        .save_snapshot(
            &unsaved_meta(&["unicode"]),
            &[pair("unicode", unicode_content)],
            false,
        )
        .unwrap();

    let loaded = store.load_content("unicode").unwrap().unwrap();
    assert_eq!(loaded, unicode_content);
}

#[test]
fn test_session_store_empty_content() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    store
        .save_snapshot(&unsaved_meta(&["empty"]), &[pair("empty", "")], false)
        .unwrap();
    let loaded = store.load_content("empty").unwrap().unwrap();
    assert_eq!(loaded, "");
}

#[test]
fn test_session_store_delete_then_load() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    store
        .save_snapshot(
            &unsaved_meta(&["to-delete"]),
            &[pair("to-delete", "some content")],
            false,
        )
        .unwrap();
    assert!(store.load_content("to-delete").unwrap().is_some());

    store.delete_content("to-delete").unwrap();
    assert!(store.load_content("to-delete").unwrap().is_none());
}

#[test]
fn test_session_store_delete_nonexistent_is_ok() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    // Deleting content that was never stored should not error
    store.delete_content("nonexistent").unwrap();
}

#[test]
fn test_snapshot_with_empty_content_clears_table_keeps_meta() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    store
        .save_snapshot(
            &unsaved_meta(&["a", "b", "c"]),
            &[pair("a", "alpha"), pair("b", "bravo"), pair("c", "charlie")],
            false,
        )
        .unwrap();

    // A later snapshot that lists a file tab with no unsaved content must drop
    // all prior content rows while leaving the (new) metadata intact.
    let meta = SessionData {
        tabs: vec![SessionTabEntry::File {
            path: "test.rs".to_string(),
            pinned: false,
            tab_color: None,
        }],
        active_tab_index: 0,
        split: None,
    };
    store.save_snapshot(&meta, &[], true).unwrap();

    assert!(store.load_content("a").unwrap().is_none());
    assert!(store.load_content("b").unwrap().is_none());
    assert!(store.load_content("c").unwrap().is_none());
    assert!(store.load_session().unwrap().is_some());
}

#[test]
fn test_session_store_full_workflow() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");

    // Simulate app exit: atomic snapshot of session + unsaved content.
    let sid = generate_session_id();
    {
        let store = SessionStore::open(&db_path).unwrap();

        let data = SessionData {
            tabs: vec![
                SessionTabEntry::File {
                    path: "main.rs".to_string(),
                    pinned: true,
                    tab_color: Some("green".to_string()),
                },
                SessionTabEntry::Unsaved {
                    session_id: sid.clone(),
                    title: "Untitled 2".to_string(),
                    pinned: false,
                    tab_color: None,
                },
            ],
            active_tab_index: 0,
            split: None,
        };
        store
            .save_snapshot(&data, &[pair(&sid, "unsaved content here")], true)
            .unwrap();
    }

    // Simulate app startup: load session + restore content.
    {
        let store = SessionStore::open(&db_path).unwrap();
        assert!(store.was_clean_shutdown().unwrap(), "exit was clean");
        let session = store.load_session().unwrap().unwrap();

        assert_eq!(session.tabs.len(), 2);
        assert_eq!(session.active_tab_index, 0);

        for tab in &session.tabs {
            match tab {
                SessionTabEntry::File {
                    path,
                    pinned,
                    tab_color,
                } => {
                    assert_eq!(path, "main.rs");
                    assert!(*pinned);
                    assert_eq!(tab_color.as_deref(), Some("green"));
                }
                SessionTabEntry::Unsaved {
                    session_id,
                    title,
                    pinned,
                    tab_color,
                } => {
                    assert_eq!(title, "Untitled 2");
                    assert!(!*pinned);
                    assert!(tab_color.is_none());
                    let content = store.load_content(session_id).unwrap().unwrap();
                    assert_eq!(content, "unsaved content here");
                }
            }
        }
    }
}

/// End-to-end regression for the reported power-loss bug: an autosave snapshot
/// (clean_shutdown = false) followed by a hard kill (no clean exit) must still
/// recover the unsaved content on the next launch, and the unclean shutdown
/// must be detectable.
#[test]
fn test_unclean_shutdown_recovers_content() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let sid = generate_session_id();

    // Running app autosaves (no clean exit afterwards = simulated power loss).
    {
        let store = SessionStore::open(&db_path).unwrap();
        store
            .save_snapshot(
                &unsaved_meta(&[&sid]),
                &[pair(&sid, "work in progress")],
                false,
            )
            .unwrap();
    }

    // Next launch: content survives and the crash is detectable.
    {
        let store = SessionStore::open(&db_path).unwrap();
        assert!(!store.was_clean_shutdown().unwrap(), "power loss = unclean");
        let session = store.load_session().unwrap().unwrap();
        assert_eq!(session.tabs.len(), 1);
        assert_eq!(
            store.load_content(&sid).unwrap().as_deref(),
            Some("work in progress"),
            "unsaved content must survive a crash, not come back empty",
        );
    }
}

#[test]
fn test_session_id_generation_unique() {
    let ids: Vec<String> = (0..100).map(|_| generate_session_id()).collect();
    // All IDs should be unique
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}
