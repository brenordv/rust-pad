# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.0] - 2026-02-15

### Added

#### Core Editor
- Text editing powered by the ropey rope data structure for efficient handling of large files.
- Multi-tab interface with tab switching (Ctrl+Tab / Ctrl+Shift+Tab) and close (Ctrl+W).
- Session restore: reopen previously open files on startup.
- Undo/Redo with persistent history that survives application restarts (backed by redb).
- Multi-cursor editing: Ctrl+Click to add cursors, Alt+Shift+Arrow to add cursors above/below, Alt+Shift+Period to select next occurrence of the current word.
- Custom editor widget rendering with egui primitives (not the built-in TextEdit).

#### Syntax Highlighting
- Syntax highlighting powered by syntect with support for 78+ languages.
- Automatic language detection based on file extension.

#### Find and Replace
- Find/Replace dialog with regex support (Ctrl+F / Ctrl+H).
- Search across all open tabs.
- Go to Line dialog (Ctrl+G).

#### Bookmarks
- Toggle bookmarks on any line (Ctrl+F2).
- Navigate between bookmarks (F2 / Shift+F2).
- Clear all bookmarks.

#### Line Operations
- Sort lines ascending or descending.
- Remove duplicate lines.
- Remove empty lines.
- Move line up/down (Alt+Up / Alt+Down).
- Duplicate current line.
- Delete current line (Ctrl+D).
- Increase/Decrease indent (Tab / Shift+Tab).

#### Case Conversion
- Convert selection to UPPERCASE.
- Convert selection to lowercase.
- Convert selection to Title Case.

#### File Handling
- New, Open, Save, Save As file operations.
- Automatic encoding detection on file open (chardetng).
- Encoding support: UTF-8, UTF-8 with BOM, UTF-16 LE, UTF-16 BE, ASCII.
- Line ending conversion: LF (Unix), CRLF (Windows), CR (classic Mac).
- Live file monitoring (tail -f mode) for watching log files.
- Auto-save for file-backed documents with configurable interval.
- Send files to the recycle bin / trash.

#### View and Display
- Customizable themes: built-in Dark, Light, and Wacky themes, plus custom themes via JSON.
- System theme mode: automatically follow OS dark/light preference.
- Zoom in/out/reset (Ctrl++ / Ctrl+- / Ctrl+0) with range 50% to 1500%.
- Word wrap toggle.
- Special character visualization (whitespace, line endings).
- Line number gutter with change tracking (orange for unsaved, green for saved edits).
- Show full file path in title bar (toggleable).
- Status bar displaying cursor position, encoding, line ending, indent style, character count, file size, zoom level, and last saved time.

#### Configuration
- JSON-based configuration file (rust-pad.json) next to the executable.
- All settings adjustable via the Settings/Preferences dialog.
- Configurable: theme, zoom, word wrap, special chars, line numbers, session restore, font size, default extension, working folder, auto-save.
- Custom theme definitions with full color control for editor and UI elements.
- Built-in themes are always available and re-added if removed from config.

#### Platform and CLI
- Cross-platform support: Windows, macOS, Linux.
- CLI: open files from the command line.
- CLI: `--new-file` flag to create a new tab with given text.
- Custom application icon.
- Native file dialogs via rfd.
- System clipboard integration via arboard.

#### Architecture
- Workspace organized into five crates: rust-pad (binary), rust-pad-core, rust-pad-ui, rust-pad-config, rust-pad-mod-history.
- Core crate is GUI-independent for testability.
- Structured logging with tracing and tracing-subscriber.
