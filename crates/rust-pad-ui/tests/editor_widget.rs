/// Tests for the editor widget behavior through the full app harness.
///
/// Since the editor uses custom painting (not egui widgets), we test behavior
/// through state changes after interactions.
mod common;

use egui::Key;
use rust_pad_core::cursor::Position;

use common::create_harness;

#[test]
fn test_editor_renders_with_text() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("Hello, editor!");
    harness.run();

    // Verify text is in the buffer
    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "Hello, editor!"
    );
}

#[test]
fn test_editor_multiline_text() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line1\nline2\nline3");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 3);
}

#[test]
fn test_editor_zoom_changes_effective_font() {
    let harness = create_harness();
    let base_font_size = harness.state().theme.font_size;
    let zoom = harness.state().zoom_level;
    let effective = base_font_size * zoom;
    // Default zoom is 1.0, so effective == base
    assert!((effective - base_font_size).abs() < f32::EPSILON);
}

#[test]
fn test_editor_zoom_scales_font_at_different_levels() {
    let mut harness = create_harness();
    let base = harness.state().theme.font_size;

    // Zoom to 2.0
    harness.state_mut().zoom_level = 2.0;
    harness.run();

    let effective = harness.state().theme.font_size * harness.state().zoom_level;
    assert!((effective - base * 2.0).abs() < f32::EPSILON);
}

#[test]
fn test_editor_cursor_at_origin() {
    let harness = create_harness();
    let cursor = &harness.state().tabs.active_doc().cursor;
    assert_eq!(cursor.position.line, 0);
    assert_eq!(cursor.position.col, 0);
}

#[test]
fn test_editor_cursor_moves_after_insert() {
    let mut harness = create_harness();
    harness.state_mut().tabs.active_doc_mut().insert_text("abc");
    harness.run();

    let cursor = &harness.state().tabs.active_doc().cursor;
    assert_eq!(cursor.position.line, 0);
    assert_eq!(cursor.position.col, 3);
}

#[test]
fn test_editor_cursor_after_newline() {
    let mut harness = create_harness();
    harness.state_mut().tabs.active_doc_mut().insert_text("abc");
    harness.state_mut().tabs.active_doc_mut().insert_newline();
    harness.run();

    let cursor = &harness.state().tabs.active_doc().cursor;
    assert_eq!(cursor.position.line, 1);
    assert_eq!(cursor.position.col, 0);
}

// ── Backspace key ────────────────────────────────────────────────────────

#[test]
fn test_editor_backspace_removes_last_char() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello");
    harness.run();

    harness.key_press(Key::Backspace);
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.to_string(), "hell");
    assert_eq!(harness.state().tabs.active_doc().cursor.position.col, 4);
}

#[test]
fn test_editor_backspace_merges_lines() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line1\nline2");
    // Place cursor at start of line 2
    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(1, 0);
    harness.run();

    harness.key_press(Key::Backspace);
    harness.run();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "line1line2"
    );
    assert_eq!(harness.state().tabs.active_doc().cursor.position.line, 0);
    assert_eq!(harness.state().tabs.active_doc().cursor.position.col, 5);
}

#[test]
fn test_editor_backspace_at_start_is_noop() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello");
    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(0, 0);
    harness.run();

    harness.key_press(Key::Backspace);
    harness.run();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "hello"
    );
}

// ── Delete key ───────────────────────────────────────────────────────────

#[test]
fn test_editor_delete_removes_next_char() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello");
    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(0, 0);
    harness.run();

    harness.key_press(Key::Delete);
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.to_string(), "ello");
    assert_eq!(harness.state().tabs.active_doc().cursor.position.col, 0);
}

#[test]
fn test_editor_delete_merges_lines() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line1\nline2");
    // Place cursor at end of line 1
    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(0, 5);
    harness.run();

    harness.key_press(Key::Delete);
    harness.run();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "line1line2"
    );
}

#[test]
fn test_editor_delete_at_end_is_noop() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello");
    // Cursor is already at the end (0, 5) after insert
    harness.run();

    harness.key_press(Key::Delete);
    harness.run();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "hello"
    );
}

// ── Selection state ─────────────────────────────────────────────────────

#[test]
fn test_editor_no_selection_initially() {
    let harness = create_harness();
    assert!(harness
        .state()
        .tabs
        .active_doc()
        .cursor
        .selection_anchor
        .is_none());
}

#[test]
fn test_editor_selection_set_programmatically() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello world");

    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 0));
    doc.cursor.position = Position::new(0, 5);
    harness.run();

    let doc = harness.state().tabs.active_doc();
    let sel = doc.cursor.selection().expect("should have selection");
    assert_eq!(sel.start(), Position::new(0, 0));
    assert_eq!(sel.end(), Position::new(0, 5));
}

// ── Empty document ──────────────────────────────────────────────────────

#[test]
fn test_editor_empty_document_renders() {
    let harness = create_harness();
    assert_eq!(harness.state().tabs.active_doc().buffer.to_string(), "");
    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 1);
}

// ── Undo/Redo through widget ────────────────────────────────────────────

#[test]
fn test_editor_undo_after_typing() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello");
    harness.run();

    // Undo via document method
    harness.state_mut().tabs.active_doc_mut().undo();
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.to_string(), "");
}

#[test]
fn test_editor_redo_after_undo() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello");
    harness.run();

    harness.state_mut().tabs.active_doc_mut().undo();
    harness.run();
    assert_eq!(harness.state().tabs.active_doc().buffer.to_string(), "");

    harness.state_mut().tabs.active_doc_mut().redo();
    harness.run();
    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "hello"
    );
}

// ── Modified flag ───────────────────────────────────────────────────────

#[test]
fn test_editor_modified_after_insert() {
    let mut harness = create_harness();
    assert!(!harness.state().tabs.active_doc().modified);

    harness.state_mut().tabs.active_doc_mut().insert_text("x");
    harness.run();

    assert!(harness.state().tabs.active_doc().modified);
}

#[test]
fn test_editor_modified_after_backspace() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello");
    // Reset modified flag (simulates a "saved" state)
    harness.state_mut().tabs.active_doc_mut().modified = false;
    harness.run();

    harness.key_press(Key::Backspace);
    harness.run();

    assert!(harness.state().tabs.active_doc().modified);
}

// ── Unicode text ────────────────────────────────────────────────────────

#[test]
fn test_editor_unicode_text() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("日本語 🦀");
    harness.run();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "日本語 🦀"
    );
    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 1);
}

// ── Multiple newlines ───────────────────────────────────────────────────

#[test]
fn test_editor_multiple_newlines() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("a\n\n\nb");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 4);
    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "a\n\n\nb"
    );
}

// ── Word wrap toggle ────────────────────────────────────────────────────

#[test]
fn test_editor_word_wrap_default_off() {
    let harness = create_harness();
    assert!(!harness.state().word_wrap);
}

#[test]
fn test_editor_word_wrap_toggle() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness.run();
    assert!(harness.state().word_wrap);
}

// ── Show line numbers ───────────────────────────────────────────────────

#[test]
fn test_editor_show_line_numbers_default_on() {
    let harness = create_harness();
    assert!(harness.state().show_line_numbers);
}

#[test]
fn test_editor_show_line_numbers_toggle() {
    let mut harness = create_harness();
    harness.state_mut().show_line_numbers = false;
    harness.run();
    assert!(!harness.state().show_line_numbers);
}

// ── Special characters mode ─────────────────────────────────────────────

#[test]
fn test_special_chars_mode_renders_multiline() {
    let mut harness = create_harness();
    harness.state_mut().show_special_chars = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line1\nline2\nline3");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 3);
    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "line1\nline2\nline3"
    );
}

#[test]
fn test_special_chars_with_badge_characters() {
    let mut harness = create_harness();
    harness.state_mut().show_special_chars = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello\u{00A0}world\u{200B}!");
    harness.run();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "hello\u{00A0}world\u{200B}!"
    );
}

#[test]
fn test_special_chars_toggle() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("test content");

    // Enable special chars and render
    harness.state_mut().show_special_chars = true;
    harness.run();
    assert!(harness.state().show_special_chars);

    // Disable and render again
    harness.state_mut().show_special_chars = false;
    harness.run();
    assert!(!harness.state().show_special_chars);
}
