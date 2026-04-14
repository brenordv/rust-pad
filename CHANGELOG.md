# Changelog

## [2.1.2]

### Fixed

- Fixed garbled text rendering after switching themes. The galley render cache was not invalidated when the syntax highlighting theme changed, causing stale cached galleys with old theme colors to be displayed.

## [2.1.1]

### Added

- Added packaging for Windows, macOS, and Linux (deb, AppImage, tar.gz) so the built app will have the proper icon.

## [2.1.0]

### Changed

#### Replace `printpdf` with `pdf-writer` for PDF generation
- Replaced the `printpdf 0.9.1` PDF backend with `pdf-writer 0.14.0` (from the typst team) to resolve RUSTSEC-2023-0019 (`kuchiki 0.8.1` unmaintained advisory).
- Added `ttf-parser 0.25.1` for font metric extraction and cmap parsing, enabling proper Type0/CIDFont embedding with a ToUnicode CMap (text is now selectable and searchable in generated PDFs).
- Eliminated 53 transitive dependency warnings from the azul/html5ever/kuchiki subtree. The new `pdf-writer` brings only 4 transitive deps (`bitflags`, `itoa`, `ryu`, `memchr`).
- PDF output is functionally identical: same A4 layout, same header/footer/gutter, same monospace DejaVu Sans Mono font, same Unicode coverage. No user-facing changes.

## [2.0.0]

### Added

#### Print / Export as PDF
- New **File → Print...** entry (Ctrl+P) renders the active document to a temporary PDF and opens it in the system's default PDF viewer, so the user can print from there. Works on Windows, macOS, and Linux with no per-platform configuration.
- New **File → Export as PDF...** entry uses the same pipeline but writes the PDF to a user-chosen path instead of opening a viewer.
- PDF output: A4 portrait, monospace (DejaVu Sans Mono, bundled — OFL-compatible license), header with filename + full path + generation timestamp, footer with `Page N of M`, optional line-number gutter.
- Pagination, tab expansion, CRLF handling, and soft-wrap of very long lines are all correct for documents of any size. Unicode content (Latin supplement, Cyrillic, CJK) renders natively; glyphs not in the bundled font fall back to `.notdef`.
- PDF generation runs on a dedicated background worker thread so the UI stays responsive during large jobs. A `Generating PDF…` indicator appears in the status bar while a job is in flight, and the menu entries plus shortcut gate on in-flight state to prevent duplicate jobs.
- On startup, any stale `rust-pad-print-*.pdf` temp files older than 24 hours are cleaned up best-effort.
- If the default viewer cannot be launched (rare), a "Print / Export Failed" dialog surfaces the error and offers a "Reveal in File Manager" button so the generated PDF is still reachable.
- New `print_show_line_numbers` config setting (default `true`) controls whether the line-number gutter appears in the generated PDF. Persisted in `rust-pad.json`.

#### Split View
- New **View → Split Vertically** (Ctrl+Alt+V) and **View → Split Horizontally** (Ctrl+Alt+H) entries divide the editor area into two panes separated by a draggable divider. **View → Remove Split** collapses back to a single pane.
- Each pane owns its own tab strip and active tab. The previously active document is moved into the right (or bottom) pane on enable; if only one tab is open, a fresh untitled tab is created so both panes have content immediately.
- The divider can be dragged to resize the panes (clamped so neither pane shrinks below ~80 px). Double-click the divider to reset to a 50/50 split.
- Right-click any tab in a pane and choose "Move to Other Pane" to reassign it without drag-and-drop. Closing the last tab in a pane automatically collapses the split.
- A 1px accent border around the focused pane shows which pane currently receives keyboard input and menu actions.
- Split layout (orientation, divider ratio, per-pane tab assignment, focused pane) is persisted across app restarts via the session store.

#### Synchronized Scrolling
- New **View → Synchronized Scrolling** entry (Ctrl+Alt+S) mirrors user-initiated scroll deltas from the focused pane to the other pane while split view is active. Useful for diffing two versions of the same file side by side.
- Only continuous, user-initiated scrolls (mouse wheel, scrollbar drag, keyboard navigation) propagate. Programmatic jumps from Go to Line, Find/Replace, and bookmark navigation keep the other pane in place.
- Each pane's viewport is independently clamped to its own content height, so deltas that would push one pane past its content boundary are silently capped without causing drift.
- New `sync_scroll_enabled` (default `false`) and `sync_scroll_horizontal` (default `true`) config settings persist the user's choice across runs. The menu entry is gated on split view being active.

#### Pin Tabs
- Right-click any tab → "Pin Tab" / "Unpin Tab" toggle. Pinned tabs are marked with the 📌 pushpin emoji prepended to the title.
- Pinned tabs are always rendered to the left of unpinned tabs. Pinning a tab moves it to the rightmost slot of the pinned section; unpinning moves it to the leftmost slot of the unpinned section. The active tab follows its document throughout the move.
- Bulk-close operations skip pinned tabs:
  - "Close Unchanged Tabs" keeps pinned tabs even if they are unchanged.
  - "Close Others" keeps pinned tabs other than the right-clicked one.
  - "Close All" leaves pinned tabs alone (modified or not) and only prompts for unpinned modified tabs.
- The × button and middle-click still close pinned tabs individually — pinning prevents accidental bulk close, not deliberate per-tab close.
- Pin state is persisted across app restarts via the session store.

#### Tab Coloring
- Right-click any tab → "Set Tab Color" submenu with a 9-color palette (Red, Orange, Yellow, Green, Cyan, Blue, Purple, Pink, Gray) plus "Clear Color".
- A tab with an assigned color always shows its colored accent stripe at the top, even when inactive — useful for visually grouping related tabs at a glance.
- Inactive tabs without an assigned color still show no accent. Active tabs without an assigned color continue to show the theme accent.
- Tab color is persisted across app restarts via the session store.

#### Drag-and-Drop Tab Reordering
- Click and hold any tab, then drag horizontally to reorder it in the tab bar. The dragged tab is dimmed in place and a vertical accent-colored indicator shows where it will be dropped on release.
- Pinned tabs are clamped to the pinned section and unpinned tabs to the unpinned section: drags cannot cross the boundary, preserving the pin/unpin layout.
- Pressing `Escape` during a drag cancels it and leaves the tab order unchanged.
- Moving the pointer vertically out of the tab bar does **not** cancel the drag (accessibility: users who cannot hold a perfectly horizontal line do not lose an in-progress reorder). The drag continues until the mouse button is released or `Escape` is pressed.
- The active tab always follows its document through the reorder.

#### One-Time Session Reset on Upgrade
- Adding pin and color metadata to the session store schema is a breaking change to the bincode-encoded `SessionTabEntry` format. On the first launch after upgrading to v2.0.0, the previous session file will fail to deserialize and the app will start with a fresh, empty session (a warning is logged). Open files will not be reopened automatically that one time. From v2.0.0 onward, the new fields are persisted and restored normally.

#### Reload from Disk
- File menu entry "Reload from Disk" to discard unsaved changes and reload the file from its on-disk state.
- Confirmation dialog when the document has unsaved modifications.
- Grayed out for untitled (unsaved) tabs.

#### Save a Copy
- File menu entry "Save a Copy As..." to save the current document to a different path without changing the active file.
- The document's path, title, and modified state remain unchanged after saving the copy.

#### Bulk Tab Close Operations
- "Close Unchanged Tabs" in the File menu and tab context menu: closes all tabs without unsaved changes.
- "Close All" in the tab context menu: closes all unmodified tabs, then prompts for modified tabs.
- "Close Others" in the tab context menu: closes all tabs except the right-clicked one.

#### Bookmark Visual Indication
- Bookmarked lines now display a blue circle indicator in the gutter (line number area).
- Visible in both light and dark themes, in both wrapped and non-wrapped modes.

#### File Size Validation
- Files are now validated against a configurable size limit before loading to prevent out-of-memory crashes. Default limit: 512 MB.
- New `max_file_size_mb` setting in `rust-pad.json` (0 = no limit, configurable 1-10240 MB). Also available in the Settings dialog under "File Size Limit".
- Applied to all file-open paths: file dialogs, recent files, CLI arguments, session restore, and live monitoring reloads.

#### UTF-16 Odd-Byte Validation
- UTF-16 LE and BE files with an odd number of bytes after BOM removal are now rejected with a clear error instead of silently dropping the last byte. This prevents silent data corruption when opening truncated or malformed UTF-16 files.

#### Bincode Deserialization Size Limits
- Deserialization of undo history entries, document metadata, and session data from redb databases is now bounded to prevent out-of-memory crashes from corrupted database files.
- Corrupted records are now gracefully skipped with a warning log instead of preventing the app from starting. History limits: 50 MB per edit group, 1 KB for metadata, 10 MB for session data.

#### Data Directory Permissions
- The history data directory is now created with owner-only permissions (0700 on Unix) to prevent other local users from listing database files.

#### History Database File Permissions
- The `history.redb` file is now created with owner-only read/write permissions (0600 on Unix) to prevent other local users from reading undo history containing deleted sensitive text.

#### Session Database File Permissions
- The `rust-pad-session.redb` file is now created with owner-only read/write permissions (0600 on Unix) to prevent other local users from reading unsaved tab content.

#### GitHub Actions SHA Pinning
- All third-party GitHub Actions in CI, release, and SonarCloud workflows are now pinned to full commit SHAs instead of mutable version tags. This prevents supply chain attacks where a compromised action could inject malicious code into builds or release artifacts.

#### Release Artifact SHA256 Checksums
- Release builds now generate a `SHA256SUMS.txt` file containing SHA256 checksums for all release artifacts (Windows .zip, macOS .zip, Linux .tar.gz, .deb). The checksum file is uploaded alongside the binaries in GitHub Releases, allowing users to verify download integrity.

#### Rust Toolchain Version Pinning
- Added `rust-toolchain.toml` pinning the Rust compiler to version 1.93.1 with `rustfmt` and `clippy` components. This ensures reproducible builds across all developer machines and CI environments, preventing surprise failures from new Rust releases.

#### Platform-Standard Config and Data Directories
- Configuration (`rust-pad.json`) is now stored in the platform-standard config directory: `~/.config/rust-pad/` (Linux), `~/Library/Application Support/rust-pad/` (macOS), `%APPDATA%\rust-pad\` (Windows).
- Data files (`history.redb`, `rust-pad-session.redb`) are now stored in the platform-standard data directory: `~/.local/share/rust-pad/` (Linux), `~/Library/Application Support/rust-pad/` (macOS), `%APPDATA%\rust-pad\` (Windows).
- On first launch, existing config and data files next to the executable are automatically migrated (copied) to the new locations. Originals are preserved for downgrade safety.
- If the platform directory cannot be determined, falls back to the executable directory with a warning.
- New `--portable` CLI flag stores all config and data next to the executable (useful for USB/portable installs).
- The `RUST_PAD_DATA_DIR` environment variable continues to override the history data directory location.
- All new directories and files are created with restrictive permissions (directories: 0700, files: 0600 on Unix).

## [1.3.0] - 2026-04-06

### Added

#### Tab Bar Horizontal Scrolling
- When open tabs exceed the available width, the tab bar now scrolls horizontally. Mouse wheel over the tab area scrolls tabs left/right, matching Notepad++ behavior.
- Left (`◀`) and right (`▶`) arrow buttons appear at the right side of the tab bar when tabs overflow. Buttons are disabled (faded) when at the scroll boundary.
- Auto-scroll: switching tabs (click, Ctrl+Tab, keyboard), opening files, or creating new tabs automatically scrolls the tab bar to keep the active tab visible.
- Closing tabs adjusts the scroll offset to avoid blank space at the end.

### Fixed

#### Tab Bar Width Jitter
- Tab width no longer shifts when hovering or switching between active and inactive tabs. Each tab is now rendered as a single allocated rect with a fixed layout (`padding + title + gap + close_area + padding`), replacing the previous two-widget approach (separate `Button` for title and conditional `Button` for close).
- The close button (`×`) is now drawn inside the tab background. It is visible on the active tab and on hover of inactive tabs, matching VS Code behavior. The close button area is always reserved in the layout so that tab width stays constant regardless of state.

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