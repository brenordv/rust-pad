use rust_pad_config::session::{generate_session_id, SessionData, SessionStore, SessionTabEntry};

#[test]
fn test_session_store_save_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    let data = SessionData {
        tabs: vec![
            SessionTabEntry::File {
                path: "/home/user/file.rs".to_string(),
            },
            SessionTabEntry::Unsaved {
                session_id: "sess-0".to_string(),
                title: "Untitled".to_string(),
            },
        ],
        active_tab_index: 1,
    };

    store.save_session(&data).unwrap();

    // Close and reopen the database
    drop(store);
    let store2 = SessionStore::open(&db_path).unwrap();
    let loaded = store2.load_session().unwrap().unwrap();

    assert_eq!(loaded.tabs.len(), 2);
    assert_eq!(loaded.active_tab_index, 1);
    match &loaded.tabs[0] {
        SessionTabEntry::File { path } => assert_eq!(path, "/home/user/file.rs"),
        _ => panic!("expected File entry"),
    }
}

#[test]
fn test_session_store_content_survives_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");

    let store = SessionStore::open(&db_path).unwrap();
    store.save_content("tab-1", "Hello, world!").unwrap();
    store.save_content("tab-2", "Second tab content").unwrap();
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
        }],
        active_tab_index: 0,
    };
    store.save_session(&data1).unwrap();

    // Overwrite with new session
    let data2 = SessionData {
        tabs: vec![
            SessionTabEntry::File {
                path: "new1.rs".to_string(),
            },
            SessionTabEntry::File {
                path: "new2.rs".to_string(),
            },
        ],
        active_tab_index: 1,
    };
    store.save_session(&data2).unwrap();

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
    store.save_content("unicode", unicode_content).unwrap();

    let loaded = store.load_content("unicode").unwrap().unwrap();
    assert_eq!(loaded, unicode_content);
}

#[test]
fn test_session_store_empty_content() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    store.save_content("empty", "").unwrap();
    let loaded = store.load_content("empty").unwrap().unwrap();
    assert_eq!(loaded, "");
}

#[test]
fn test_session_store_delete_then_load() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    store.save_content("to-delete", "some content").unwrap();
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
fn test_session_store_clear_all_content() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");
    let store = SessionStore::open(&db_path).unwrap();

    store.save_content("a", "alpha").unwrap();
    store.save_content("b", "bravo").unwrap();
    store.save_content("c", "charlie").unwrap();

    store.clear_all_content().unwrap();

    assert!(store.load_content("a").unwrap().is_none());
    assert!(store.load_content("b").unwrap().is_none());
    assert!(store.load_content("c").unwrap().is_none());

    // Session metadata should be unaffected
    let data = SessionData {
        tabs: vec![SessionTabEntry::File {
            path: "test.rs".to_string(),
        }],
        active_tab_index: 0,
    };
    store.save_session(&data).unwrap();
    store.clear_all_content().unwrap();
    assert!(store.load_session().unwrap().is_some());
}

#[test]
fn test_session_store_full_workflow() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("session.redb");

    // Simulate app exit: save session + unsaved content
    {
        let store = SessionStore::open(&db_path).unwrap();
        let sid = generate_session_id();

        let data = SessionData {
            tabs: vec![
                SessionTabEntry::File {
                    path: "main.rs".to_string(),
                },
                SessionTabEntry::Unsaved {
                    session_id: sid.clone(),
                    title: "Untitled 2".to_string(),
                },
            ],
            active_tab_index: 0,
        };
        store.save_session(&data).unwrap();
        store.save_content(&sid, "unsaved content here").unwrap();
    }

    // Simulate app startup: load session + restore content
    {
        let store = SessionStore::open(&db_path).unwrap();
        let session = store.load_session().unwrap().unwrap();

        assert_eq!(session.tabs.len(), 2);
        assert_eq!(session.active_tab_index, 0);

        // Restore content for unsaved tabs
        for tab in &session.tabs {
            match tab {
                SessionTabEntry::File { path } => {
                    assert_eq!(path, "main.rs");
                }
                SessionTabEntry::Unsaved { session_id, title } => {
                    assert_eq!(title, "Untitled 2");
                    let content = store.load_content(session_id).unwrap().unwrap();
                    assert_eq!(content, "unsaved content here");
                }
            }
        }

        // After restoring, clear old content
        store.clear_all_content().unwrap();
    }
}

#[test]
fn test_session_id_generation_unique() {
    let ids: Vec<String> = (0..100).map(|_| generate_session_id()).collect();
    // All IDs should be unique
    let unique: std::collections::HashSet<&String> = ids.iter().collect();
    assert_eq!(unique.len(), ids.len());
}
