# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.2.0] - 2026-04-05

### Added

#### Auto-Indent on Enter
- Pressing Enter now inherits the leading whitespace (spaces or tabs) from the current line, matching the behavior of VS Code and Notepad++.
- Works with multi-cursor editing: each cursor inherits indent from its own line.

#### Bracket Matching Highlights
- When the cursor is on or adjacent to a bracket (`(`, `)`, `[`, `]`, `{`, `}`), both the bracket and its matching counterpart are highlighted with a background color.
- Supports nested and mixed bracket types. Search is capped at 10,000 characters to avoid freezing on large files.
- Highlight color is theme-aware and configurable via the `matching_bracket_color` field in custom themes.

#### Session Store Size Limit
- Added a configurable size limit (`session_content_max_kb`) for unsaved tab content persisted in the session store. Default: 10,240 KB (10 MB).
- Tabs exceeding the limit are saved as metadata only (title preserved, content skipped). On restore, these tabs appear empty with the original title.
- Setting the limit to 0 disables the check (unlimited, backward-compatible).
- Each tab is checked independently — one large tab does not block others.
- Configurable via the Settings dialog under the History tab ("Max unsaved content to restore (KB)").

#### Async File I/O
- File open dialogs, save-as dialogs, file reads, and file writes now run on background threads, keeping the UI responsive during slow I/O (network drives, USB sticks, large files).
- The status bar shows an activity indicator with a spinner ("Opening...", "Saving...", "Reading...") while I/O is in progress.
- Editing is blocked while a file dialog is open, preventing modifications to content that is being saved.
- Opening the same file concurrently is de-duplicated: the existing tab is activated instead.
- Content version tracking ensures the modified flag is preserved correctly if the user edits between save initiation and completion.

### Changed

#### Galley Cache Granularity
- The render cache no longer clears all cached galleys when the document content changes. Per-line content hashes already guard correctness, so unchanged lines now keep their cached galleys across edits.
- Added periodic pruning of cached galleys outside the visible range (±50-line margin) to bound memory usage.
- Net effect: editing a single line in a large file (50K+ lines) no longer forces re-highlighting of every visible line on the next frame.

#### Dependencies
- Updated `egui`/`eframe` to 0.34, `chardetng` to 1.0, and other dependencies. Adjusted API usage to align with upstream changes.

#### Architecture: Decompose App Struct
- Decomposed the monolithic `App` struct (which held ~40 fields and ~26 methods) into five focused, composable sub-structs. `App` is now a thin orchestrator that wires these components together in the update loop.
  - **ThemeController** — owns editor theme, theme mode, available themes, accent color, syntax highlighter, and zoom level. Provides `set_mode()`, `zoom_in/out/reset()`, and `apply_theme_visuals()`.
  - **RecentFilesManager** — owns the recent files list, enabled flag, max count, and cleanup strategy. Provides `track()`, `cleanup_on_menu_open()`, and config serialization.
  - **FileDialogState** — owns remember-last-folder preference, default work folder, last-used folder, and default extension. Provides `resolve_directory()` and `update_last_folder()`.
  - **AutoSaveController** — owns enabled flag, interval, and timer. Provides `tick(&mut TabManager)` which checks timing and saves all modified file-backed documents.
  - **LiveMonitorController** — owns the file-check timer. Provides `tick(&mut TabManager)` which polls for external file changes and reloads live-monitored documents.

## [1.1.0] - 2026-04-05

### Added

#### Context Menu
- Right-click the context menu in the editor area with clipboard actions (Cut, Copy, Paste, Delete), selection actions (Select All, Invert Selection), and scoped text operations.
- Scoped operations: Convert Case and Line Operations (sort, remove duplicates, remove empty lines) can now target either the entire document or just the current selection.
- Invert Selection: toggles selection (no selection → select all, full selection → clear, partial → invert to unselected regions).

#### Vertical Selection Improvements
- Alt+Shift+Up/Down now supports shrink behavior: pressing the opposite direction removes the furthest cursor instead of always adding.
- New vertical cursors inherit the primary cursor's selection column range.

#### Recent Files
- Recent files history with configurable max count (1–50, default 10).
- Automatic cleanup of non-existent files with three strategies: On Startup, When Menu Opens, or Both.
- "Open Recent" submenu in the File menu with tooltip showing full path.
- Recent files list persisted in the application configuration.

#### Settings Dialog
- Redesigned settings dialog with a two-column layout: left navigation sidebar and right scrollable content panel.
- New "History" settings tab for configuring recent files behavior.

### Changed
- Menu bar buttons now use `shortcut_text()` for consistent keyboard shortcut display.
- Editor widget returns a `Response` to support context menu attachment.
- `merge_overlapping_cursors` visibility changed from `pub(crate)` to `pub`.
- Find/Replace shortcut (Ctrl+H) added to the Edit menu in addition to the Search menu.
- Updated dependencies.


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