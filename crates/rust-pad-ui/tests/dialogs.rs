/// Tests for dialog components (Find/Replace, Go to Line).
mod common;

use egui::{Key, Modifiers};
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use rust_pad_core::cursor::Position;
use rust_pad_ui::App;

use common::create_harness;

// ── Find/Replace Dialog ────────────────────────────────────────────────────

#[test]
fn test_find_replace_dialog_not_visible_initially() {
    let harness = create_harness();
    assert!(harness.query_by_label("Find and Replace").is_none());
}

#[test]
fn test_find_replace_dialog_opens_with_ctrl_f() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::F);
    harness.run();
    harness.get_by_label("Find and Replace");
}

#[test]
fn test_find_replace_dialog_opens_with_ctrl_h() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::H);
    harness.run();
    harness.get_by_label("Find and Replace");
}

#[test]
fn test_find_replace_buttons_exist() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::F);
    harness.run();

    harness.get_by_label("  Find Next  ");
    harness.get_by_label("  Find Prev  ");
    harness.get_by_label("  Replace  ");
    harness.get_by_label("  Replace All  ");
}

#[test]
fn test_find_replace_has_checkboxes() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::F);
    harness.run();

    harness.get_by_label("Case sensitive");
    harness.get_by_label("Whole word");
    harness.get_by_label("Regex");
}

#[test]
fn test_find_replace_closes_on_escape() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::F);
    harness.run();
    assert!(harness.query_by_label("Find and Replace").is_some());

    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Find and Replace").is_none());
}

// ── Go to Line Dialog ──────────────────────────────────────────────────────

#[test]
fn test_go_to_line_dialog_not_visible_initially() {
    let harness = create_harness();
    assert!(harness.query_by_label("Go to Line").is_none());
}

#[test]
fn test_go_to_line_dialog_opens_with_ctrl_g() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::G);
    harness.run();
    harness.get_by_label("Go to Line");
}

#[test]
fn test_go_to_line_buttons_exist() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::G);
    harness.run();

    harness.get_by_label("    Go    ");
    harness.get_by_label("  Cancel  ");
}

#[test]
fn test_go_to_line_closes_on_escape() {
    let mut harness = create_harness();
    let ctrl = Modifiers {
        ctrl: true,
        ..Default::default()
    };
    harness.key_press_modifiers(ctrl, Key::G);
    harness.run();
    assert!(harness.query_by_label("Go to Line").is_some());

    harness.key_press(Key::Escape);
    harness.run();
    assert!(harness.query_by_label("Go to Line").is_none());
}

// ── Search & Replace Functionality ────────────────────────────────────────

/// Helper: inserts text and sets up the find dialog with a query.
fn setup_search(harness: &mut Harness<'_, App>, text: &str, query: &str) {
    harness.state_mut().tabs.active_doc_mut().insert_text(text);
    // Reset cursor to start so searches begin from the top
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
    // Open the find dialog and set the query text
    harness.state_mut().find_replace.open();
    harness.state_mut().find_replace.find_text = query.to_string();
    harness.state_mut().find_replace.options.query = query.to_string();
    // Run a frame so the dialog renders and the change-detection kicks in
    harness.run();
}

#[test]
fn test_search_finds_matches_and_updates_status() {
    let mut harness = create_harness();
    setup_search(&mut harness, "Hello world Hello", "Hello");
    // The change-detection should have triggered a Search action
    harness.run();
    let app = harness.state();
    assert_eq!(app.find_replace.engine.match_count(), 2);
    assert!(
        app.find_replace.status.contains("2"),
        "Status should report 2 matches, got: {}",
        app.find_replace.status
    );
}

#[test]
fn test_search_case_sensitive_updates_count() {
    let mut harness = create_harness();
    setup_search(&mut harness, "Hello HELLO hello", "hello");
    harness.run();

    // Default is case-insensitive — should find all 3
    assert_eq!(harness.state().find_replace.engine.match_count(), 3);

    // Toggle case-sensitive on
    harness.state_mut().find_replace.options.case_sensitive = true;
    harness.run(); // change-detection triggers re-search
    harness.run(); // ensure action is processed

    assert_eq!(
        harness.state().find_replace.engine.match_count(),
        1,
        "Case-sensitive search for 'hello' should find exactly 1 match"
    );
    assert!(harness.state().find_replace.status.contains('1'));
}

#[test]
fn test_search_whole_word_updates_count() {
    let mut harness = create_harness();
    setup_search(&mut harness, "cat catch catapult cat", "cat");
    harness.run();

    // Case-insensitive, no whole-word: "cat" appears in "cat", "catch", "catapult", "cat"
    // regex finds non-overlapping: "cat" in "cat", "cat" in "catch", "cat" in "catapult", "cat"
    let initial_count = harness.state().find_replace.engine.match_count();
    assert!(
        initial_count >= 3,
        "Should find at least 3 'cat' occurrences, got {initial_count}"
    );

    // Toggle whole-word on — should only match standalone "cat"
    harness.state_mut().find_replace.options.whole_word = true;
    harness.run();
    harness.run();

    assert_eq!(
        harness.state().find_replace.engine.match_count(),
        2,
        "Whole-word search for 'cat' should find exactly 2 matches"
    );
}

#[test]
fn test_search_regex_toggle_updates_count() {
    let mut harness = create_harness();
    setup_search(&mut harness, "foo123 bar456 baz", r"\d+");
    harness.run();

    // With regex off, literal "\d+" won't match anything
    assert_eq!(
        harness.state().find_replace.engine.match_count(),
        0,
        r"Literal '\d+' should not match"
    );

    // Toggle regex on
    harness.state_mut().find_replace.options.use_regex = true;
    harness.run();
    harness.run();

    assert_eq!(
        harness.state().find_replace.engine.match_count(),
        2,
        r"Regex '\d+' should find 2 number sequences"
    );
}

#[test]
fn test_find_next_navigates_sequentially() {
    let mut harness = create_harness();
    setup_search(&mut harness, "asd\nasd\nasd", "asd");
    harness.run();

    assert_eq!(harness.state().find_replace.engine.match_count(), 3);

    // Cursor starts at (0,0). First FindNext should land on match 0 (line 0).
    // Simulate pressing Ctrl+F to make sure dialog is open, then we manually
    // trigger find_next through the dialog actions.
    // Since we can't easily click dialog buttons in kittest, we verify the
    // engine state directly by checking cursor position after each step.
    let app = harness.state_mut();
    let doc = app.tabs.active_doc_mut();
    let cursor_char =
        rust_pad_core::cursor::pos_to_char(&doc.buffer, doc.cursor.position).unwrap_or(0);
    let idx = app.find_replace.engine.find_next(cursor_char).unwrap();
    assert_eq!(idx, 0, "First FindNext from (0,0) should find match 0");

    // Simulate cursor moving to end of match 0
    let mat_end = app.find_replace.engine.matches[0].end;
    let end_pos = rust_pad_core::cursor::char_to_pos(&doc.buffer, mat_end);
    doc.cursor.move_to(end_pos, &doc.buffer);

    // Second FindNext from mat.end should find match 1
    let cursor_char =
        rust_pad_core::cursor::pos_to_char(&doc.buffer, doc.cursor.position).unwrap_or(0);
    let idx = app.find_replace.engine.find_next(cursor_char).unwrap();
    assert_eq!(idx, 1, "Second FindNext should find match 1");

    // Move cursor to end of match 1
    let mat_end = app.find_replace.engine.matches[1].end;
    let end_pos = rust_pad_core::cursor::char_to_pos(&doc.buffer, mat_end);
    doc.cursor.move_to(end_pos, &doc.buffer);

    // Third FindNext from mat.end should find match 2
    let cursor_char =
        rust_pad_core::cursor::pos_to_char(&doc.buffer, doc.cursor.position).unwrap_or(0);
    let idx = app.find_replace.engine.find_next(cursor_char).unwrap();
    assert_eq!(idx, 2, "Third FindNext should find match 2");
}

#[test]
fn test_replace_current_updates_buffer() {
    let mut harness = create_harness();
    setup_search(&mut harness, "hello world hello", "hello");
    harness.run();

    assert_eq!(harness.state().find_replace.engine.match_count(), 2);

    // Replace the first match
    let app = harness.state_mut();
    let options = app.find_replace.options.clone();
    let doc = app.tabs.active_doc_mut();
    app.find_replace
        .engine
        .replace_current(&mut doc.buffer, "hi", &options)
        .unwrap();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "hi world hello"
    );
    assert_eq!(harness.state().find_replace.engine.match_count(), 1);
}

#[test]
fn test_replace_all_updates_buffer() {
    let mut harness = create_harness();
    setup_search(&mut harness, "foo bar foo baz foo", "foo");
    harness.run();

    assert_eq!(harness.state().find_replace.engine.match_count(), 3);

    let app = harness.state_mut();
    let options = app.find_replace.options.clone();
    let doc = app.tabs.active_doc_mut();
    let count = app
        .find_replace
        .engine
        .replace_all(&mut doc.buffer, "qux", &options)
        .unwrap();

    assert_eq!(count, 3);
    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "qux bar qux baz qux"
    );
    assert_eq!(harness.state().find_replace.engine.match_count(), 0);
}

#[test]
fn test_empty_search_clears_matches() {
    let mut harness = create_harness();
    setup_search(&mut harness, "some text", "some");
    harness.run();
    assert_eq!(harness.state().find_replace.engine.match_count(), 1);

    // Clear the search text
    harness.state_mut().find_replace.find_text.clear();
    harness.state_mut().find_replace.options.query.clear();
    harness.run();
    harness.run();

    assert_eq!(harness.state().find_replace.engine.match_count(), 0);
}

#[test]
fn test_no_matches_status() {
    let mut harness = create_harness();
    setup_search(&mut harness, "hello world", "xyz");
    harness.run();

    assert_eq!(harness.state().find_replace.engine.match_count(), 0);
    assert!(
        harness.state().find_replace.status.contains("No matches"),
        "Status should say 'No matches', got: {}",
        harness.state().find_replace.status
    );
}
