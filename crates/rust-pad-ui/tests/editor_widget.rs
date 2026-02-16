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

// ── Rendering code-path coverage ─────────────────────────────────────

#[test]
fn test_render_with_selection_highlights() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello world foo bar");

    // Set up a selection spanning "world"
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 6));
    doc.cursor.position = Position::new(0, 11);
    harness.run();

    // Selection should be active after render
    assert!(harness
        .state()
        .tabs
        .active_doc()
        .cursor
        .selection_anchor
        .is_some());
}

#[test]
fn test_render_with_multiline_selection() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line one\nline two\nline three");

    // Select across lines (triggers selection highlight on multiple lines)
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 3));
    doc.cursor.position = Position::new(2, 5);
    harness.run();

    let sel = harness
        .state()
        .tabs
        .active_doc()
        .cursor
        .selection()
        .expect("should have selection");
    assert_eq!(sel.start().line, 0);
    assert_eq!(sel.end().line, 2);
}

#[test]
fn test_render_with_occurrence_highlights() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("foo bar foo baz foo");

    // Select "foo" to trigger occurrence highlight-all
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 0));
    doc.cursor.position = Position::new(0, 3);
    harness.run();

    // Text should still be intact, occurrences are rendered as background rects
    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "foo bar foo baz foo"
    );
}

#[test]
fn test_render_with_syntax_highlighting() {
    let mut harness = create_harness();
    // Set title with .rs extension to trigger Rust syntax highlighting
    harness.state_mut().tabs.active_doc_mut().title = "test.rs".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("fn main() {\n    let x = 42;\n    println!(\"hello\");\n}\n");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 5);
}

#[test]
fn test_render_syntax_highlight_python() {
    let mut harness = create_harness();
    harness.state_mut().tabs.active_doc_mut().title = "script.py".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("def hello():\n    print('world')\n");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 3);
}

#[test]
fn test_render_word_wrap_with_long_lines() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text(&"abcdefghij".repeat(50)); // 500 chars on one line
    harness.run();

    assert!(harness.state().word_wrap);
    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 1);
}

#[test]
fn test_render_word_wrap_multiline() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("first line that is quite long\nsecond line also lengthy\nthird line\n");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 4);
}

#[test]
fn test_render_word_wrap_with_selection() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello world this is a long line that should wrap around the editor area\n");

    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 0));
    doc.cursor.position = Position::new(0, 20);
    harness.run();

    assert!(harness
        .state()
        .tabs
        .active_doc()
        .cursor
        .selection_anchor
        .is_some());
}

#[test]
fn test_render_word_wrap_with_occurrence_highlights() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("word word word word word word word word word word word word word word word");

    // Select "word" to trigger occurrence highlighting
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 0));
    doc.cursor.position = Position::new(0, 4);
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 1);
}

#[test]
fn test_render_special_chars_with_tabs() {
    let mut harness = create_harness();
    harness.state_mut().show_special_chars = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("col1\tcol2\tcol3\n");
    harness.run();

    assert!(harness
        .state()
        .tabs
        .active_doc()
        .buffer
        .to_string()
        .contains('\t'));
}

#[test]
fn test_render_special_chars_with_nbsp_badges() {
    let mut harness = create_harness();
    harness.state_mut().show_special_chars = true;
    // Insert text with NBSP and ZWSP characters that render as badges
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("hello\u{00A0}world\u{200B}test\u{200C}end\u{200D}!");
    harness.run();

    let text = harness.state().tabs.active_doc().buffer.to_string();
    assert!(text.contains('\u{00A0}'));
    assert!(text.contains('\u{200B}'));
}

#[test]
fn test_render_special_chars_with_selection_over_eol() {
    let mut harness = create_harness();
    harness.state_mut().show_special_chars = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line1\nline2\nline3\n");

    // Selection spanning across line ending (triggers EOL badge selection extension)
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 0));
    doc.cursor.position = Position::new(1, 3);
    harness.run();

    let sel = harness
        .state()
        .tabs
        .active_doc()
        .cursor
        .selection()
        .expect("should have selection");
    assert_eq!(sel.start().line, 0);
    assert_eq!(sel.end().line, 1);
}

#[test]
fn test_render_no_line_numbers() {
    let mut harness = create_harness();
    harness.state_mut().show_line_numbers = false;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line1\nline2\nline3");
    harness.run();

    assert!(!harness.state().show_line_numbers);
}

#[test]
fn test_render_syntax_highlight_with_selection() {
    let mut harness = create_harness();
    harness.state_mut().tabs.active_doc_mut().title = "test.rs".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("fn main() {\n    let x = 42;\n}\n");

    // Select "let x = 42" across the highlighted code
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(1, 4));
    doc.cursor.position = Position::new(1, 15);
    harness.run();

    assert!(harness
        .state()
        .tabs
        .active_doc()
        .cursor
        .selection_anchor
        .is_some());
}

#[test]
fn test_render_word_wrap_with_syntax_highlight() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness.state_mut().tabs.active_doc_mut().title = "test.rs".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("fn very_long_function_name(param1: &str, param2: i32, param3: f64, param4: bool) -> Result<String, Error> {\n    println!(\"hello\");\n}\n");
    harness.run();

    assert!(harness.state().word_wrap);
}

#[test]
fn test_render_word_wrap_with_special_chars() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness.state_mut().show_special_chars = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("a b c d e f g h i j k l m n o p q r s t u v w x y z\n");
    harness.run();

    assert!(harness.state().word_wrap);
    assert!(harness.state().show_special_chars);
}

#[test]
fn test_render_large_document() {
    let mut harness = create_harness();
    let mut text = String::new();
    for i in 0..100 {
        text.push_str(&format!("Line {i}: some content here\n"));
    }
    harness.state_mut().tabs.active_doc_mut().insert_text(&text);
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 101);
}

#[test]
fn test_render_with_scrolled_position() {
    let mut harness = create_harness();
    let mut text = String::new();
    for i in 0..100 {
        text.push_str(&format!("Line {i}\n"));
    }
    harness.state_mut().tabs.active_doc_mut().insert_text(&text);

    // Move cursor to line 50 to force vertical scroll
    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(50, 0);
    harness.run();

    // Scroll should have adjusted
    assert!(harness.state().tabs.active_doc().scroll_y > 0.0);
}

#[test]
fn test_render_with_horizontal_scroll() {
    let mut harness = create_harness();
    // Very long line to trigger horizontal scroll
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text(&"x".repeat(500));

    // Move cursor to the end of the long line
    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(0, 500);
    harness.run();

    // Horizontal scroll should be non-zero
    assert!(harness.state().tabs.active_doc().scroll_x > 0.0);
}

#[test]
fn test_render_word_wrap_no_horizontal_scroll() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text(&"y".repeat(300));

    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(0, 300);
    harness.run();

    // In word wrap mode, horizontal scroll should be 0
    assert!((harness.state().tabs.active_doc().scroll_x - 0.0).abs() < f32::EPSILON);
}

#[test]
fn test_render_badges_with_selection() {
    let mut harness = create_harness();
    harness.state_mut().show_special_chars = true;
    // Text with badge characters and a selection spanning across them
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("abc\u{00A0}def\u{200B}ghi");

    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 2));
    doc.cursor.position = Position::new(0, 8);
    harness.run();

    assert!(harness
        .state()
        .tabs
        .active_doc()
        .cursor
        .selection_anchor
        .is_some());
}

#[test]
fn test_render_multiple_frames_with_content_change() {
    let mut harness = create_harness();
    harness.state_mut().tabs.active_doc_mut().title = "test.rs".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("fn foo() {}");
    harness.run();

    // Modify content between frames (invalidates galley cache)
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("\nfn bar() {}");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 2);
}

#[test]
fn test_render_cached_galley_reuse() {
    let mut harness = create_harness();
    harness.state_mut().tabs.active_doc_mut().title = "test.rs".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("let x = 42;\n");

    // Render twice without changing content — second render should use cached galley
    harness.run();
    harness.run();

    assert_eq!(
        harness.state().tabs.active_doc().buffer.to_string(),
        "let x = 42;\n"
    );
}

#[test]
fn test_render_modified_document_change_tracking() {
    let mut harness = create_harness();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("line1\nline2\nline3\n");
    // Mark as saved to establish baseline
    harness.state_mut().tabs.active_doc_mut().modified = false;
    harness.run();

    // Now modify line 2 — this should trigger change tracking markers in the gutter
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.position = Position::new(1, 5);
    doc.insert_text(" modified");
    harness.run();

    assert!(harness.state().tabs.active_doc().modified);
}

#[test]
fn test_render_word_wrap_with_scrolled_position() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    let mut text = String::new();
    for i in 0..50 {
        text.push_str(&format!("This is line number {i} with some extra content to cause wrapping in the editor widget\n"));
    }
    harness.state_mut().tabs.active_doc_mut().insert_text(&text);

    // Move cursor to a line that requires vertical scroll in wrap mode
    harness.state_mut().tabs.active_doc_mut().cursor.position = Position::new(40, 0);
    harness.run();

    assert!(harness.state().tabs.active_doc().scroll_y > 0.0);
}

#[test]
fn test_render_word_wrap_first_and_continuation_rows() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    // Insert a line that will wrap multiple times
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text(&"ABCDE ".repeat(100));
    harness.run();

    // The single logical line should still be 1 line
    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 1);
}

#[test]
fn test_render_plain_text_no_extension() {
    let mut harness = create_harness();
    // Title without extension — still uses syntax highlighter but detects plain text
    harness.state_mut().tabs.active_doc_mut().title = "Untitled".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("Just plain text with no syntax highlighting rules.\nSecond line.\n");
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 3);
}

#[test]
fn test_render_with_all_features_combined() {
    let mut harness = create_harness();
    harness.state_mut().show_special_chars = true;
    harness.state_mut().show_line_numbers = true;
    harness.state_mut().tabs.active_doc_mut().title = "test.rs".to_string();
    harness
        .state_mut()
        .tabs
        .active_doc_mut()
        .insert_text("fn test() {\n    let x\u{00A0}= 42;\n    println!(\"{x}\");\n}\n");

    // Set up a selection and render
    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(1, 8));
    doc.cursor.position = Position::new(1, 14);
    harness.run();

    assert!(harness.state().show_special_chars);
    assert!(harness.state().show_line_numbers);
}

#[test]
fn test_render_word_wrap_with_all_features() {
    let mut harness = create_harness();
    harness.state_mut().word_wrap = true;
    harness.state_mut().show_special_chars = true;
    harness.state_mut().show_line_numbers = true;
    harness.state_mut().tabs.active_doc_mut().title = "long.rs".to_string();
    harness.state_mut().tabs.active_doc_mut().insert_text(
        "fn very_long_function() { let result = some_computation(param1, param2, param3, param4, param5); result }\n\
         fn another() { let x = 1; let y = 2; let z = x + y; println!(\"{z}\"); }\n"
    );

    let doc = harness.state_mut().tabs.active_doc_mut();
    doc.cursor.selection_anchor = Some(Position::new(0, 10));
    doc.cursor.position = Position::new(0, 30);
    harness.run();

    assert!(harness.state().word_wrap);
}

#[test]
fn test_render_empty_document_with_line_numbers() {
    let mut harness = create_harness();
    harness.state_mut().show_line_numbers = true;
    // Empty document — gutter should show line 1
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 1);
}

#[test]
fn test_render_document_with_many_lines_for_gutter_width() {
    let mut harness = create_harness();
    harness.state_mut().show_line_numbers = true;
    // 1000+ lines to test multi-digit gutter width computation
    let mut text = String::new();
    for i in 0..1001 {
        text.push_str(&format!("{i}\n"));
    }
    harness.state_mut().tabs.active_doc_mut().insert_text(&text);
    harness.run();

    assert_eq!(harness.state().tabs.active_doc().buffer.len_lines(), 1002);
}
