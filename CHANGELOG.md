# Changelog

## [2.12.2]

### Fixed
- **Hovering the workspace sidebar stole the arrow keys from the editor.** After clicking into a document, moving the mouse over the workspace panel — without clicking anything — caused the next arrow-key press to move the selection in the file tree instead of the caret in the text. Keyboard ownership was tied to the pointer *hovering* the sidebar; it is now strictly **click-to-focus** — the sidebar only takes the arrow/Enter/F2 keys after you click one of its rows, and releases them the moment you click back in the editor. Pointer position no longer affects where keystrokes go. (This supersedes the narrower 2.12.1 fix, which only covered the arrow press immediately after opening a file with Enter.)
- **`Shift+Tab` flattened unevenly indented lines to the left margin.** Dedenting a multi-line selection removed "a tab's worth" of spaces from every line independently, so lines indented with fewer spaces than the tab width collapsed all the way to column 0 and their relative indentation was lost. Dedent is now a **block operation**: it removes the smallest common amount of leading whitespace (capped at one indent level) from every selected line at once, preserving the relative indentation between them, and repeated presses march the block left until it reaches the margin. Tab-indented files and single-line dedent are unchanged.

## [2.12.1]

### Fixed
- **Unsaved tab content lost after an unexpected shutdown (power loss / hard kill).** Unsaved buffers were written to the session store only on a *clean* exit. The restore step then wiped the stored content — so if the app was killed (power loss, OOM, panic) before the next clean exit, tabs came back present but empty, with nothing reported in the Problems menu. The session is now persisted as a single **atomic snapshot** (tab list + unsaved content committed together) on a periodic timer while you edit, immediately after a restore, and on clean exit. A hard abort now loses at most the last autosave interval of edits instead of everything, and on the next launch a recovery is reported in the Problems menu ("Recovered N unsaved document(s) after an unexpected shutdown"). Note: unsaved content is now persisted continuously while editing (bounded by the existing session content size limit), not only between restarts.

## [2.12.0]

### Added
- **Find All results panel (Notepad++ style).** The Find/Replace dialog has a new **Find All** button that lists every match — in the current tab or across all open tabs, following the Scope selector — in a dockable results panel above the status bar. Each result shows `tab:line  text`; double-clicking a result activates its tab, selects the match, and hands keyboard focus to the editor. The panel stays in sync with the dialog's case/whole-word/regex options.
- **"Dusk" theme — a low-glare light theme.** A warm, parchment-toned light theme (never pure white) paired with the Solarized-light syntax palette, for users who find the high-contrast `Light` theme harsh. Available in Settings → Appearance alongside Dark, Light, and Wacky, and auto-added to existing configurations on upgrade.
- **Hide-sidebar button in the workspace toolbar.** The workspace sidebar header has a new collapse button that hides the panel without closing the workspace (reopen with `Ctrl+B` or the Workspace menu) — distinct from the existing "Close workspace" action.

### Changed
- **Dark theme contrast.** The dark theme's text selection and occurrence-highlight colors are brighter and more opaque so selected and highlighted text stays legible.

### Fixed
- **macOS: "Too many open files" (EMFILE) after opening several workspace files.** The filesystem watcher used the kqueue backend, which consumes one file descriptor per watched directory; on a large workspace this exhausted the low (256) descriptor limit that macOS `.app` bundles inherit, causing failures when opening files, auto-saving, or saving config on exit. The watcher now uses the FSEvents backend (a single descriptor for the whole recursive watch), and on Unix the descriptor soft limit is raised to the hard limit at startup. The FSEvents path-coalescing behavior is already handled by the directory-reconciling tree refresh and the "Reload from disk" action.
- **Workspace sidebar kept the arrow keys after opening a file with Enter.** Pressing Enter on a file moved the cursor into the editor, but a pointer still resting over the sidebar reclaimed the arrow keys on the next press, so navigation drove the tree instead of the editor. The sidebar now only re-acquires keyboard ownership on a deliberate engagement (pointer movement within it, or a click), so focus stays with the editor after a file opens.

## [2.11.0]

### Added
- **Shift+click to extend a selection.** Clicking to place the caret and then Shift+clicking elsewhere now selects the text in between, matching standard text-editor behavior. The anchor is taken from the caret's current position (or the existing selection's anchor), so repeated Shift+clicks keep extending from the *original* anchor rather than resetting to each new click. Works in both directions (forward and backward), and a Shift+click on the caret's own position leaves nothing highlighted. Plain click (clears selection and moves the caret) and Shift+drag selection are unchanged.
- **Workspace context menu: `Reload from disk` for folders.** Right-clicking a folder (including a workspace root) now offers a "Reload from disk" item that re-reads that directory and its currently-expanded sub-tree from the filesystem and reconciles the tree with on-disk changes — new entries appear, deleted ones disappear — while preserving which folders are expanded. It is a deterministic, cross-platform way to refresh the tree when the live filesystem watcher misses a change.

### Fixed
- **macOS: folders created while the workspace was open could stay unlisted.** The sidebar's tree was updated incrementally from filesystem-watcher events that assume each event names the exact file or folder that changed — true for the Windows and Linux backends, but not for macOS, where the `notify` FSEvents backend coalesces a burst of changes into a single event for the *containing* directory. Such an event found the directory already present and was treated as a no-op, so entries created inside it during the session never surfaced (most visible with a folder like `.mcp-vault` that is written to in the background). The watcher now re-reads and reconciles a directory (and its expanded sub-tree) when an event targets the directory itself, and the new `Reload from disk` context-menu item provides a guaranteed manual refresh in any remaining edge case. Windows and Linux keep their existing per-entry fast path unchanged.

## [2.10.0]

### Added
- **File-tab context menu: `Copy Path` submenu.** Right-clicking a file tab now exposes the same `Copy Path > {Name | Full Path | Relative Path}` submenu available in the workspace tree (single-pane and split-view tab bars alike). Relative paths are resolved against the open workspace folder that contains the file; the `Relative Path` item is disabled when the file lives outside every workspace folder, and the whole submenu is disabled for unsaved buffers that have no path yet. Copies go through the same control-character refusal gate as the workspace feature (LF, NUL, ANSI escape, DEL, C1: Trojan-filename clipboard-injection class, cf. CVE-2017-12424).

## [2.9.0]

### Added
- **Workspace context menu: `Copy Contents` for files.** Right-clicking a file in the workspace now offers a "Copy Contents" item that decodes the file (UTF-8 / UTF-16 / detected legacy encoding), normalises line endings to LF, and pushes the result to the system clipboard. Binary-looking files (decode errors or NULs in the decoded text) are refused with a Problems entry. Files containing Unicode bidirectional override characters (Trojan Source attack class, CVE-2021-42574) are copied with a non-blocking notice so the user can verify before pasting into code or commit messages.
- **Workspace context menu: `Open in File Explorer` for folders.** Right-clicking a folder (including a workspace root) now offers an "Open in File Explorer" item that reveals the directory in the OS file manager (Windows Explorer, macOS Finder, `xdg-open` on Linux).
- **Workspace context menu: `Copy Path` submenu.** Right-clicking any entry now exposes a `Copy Path > {Name | Full Path | Relative Path}` submenu. Relative paths are resolved against the workspace root that contains the entry. Paths containing control characters (LF, NUL, ANSI escape, DEL, C1) are refused with a Problems entry to block the Trojan-filename clipboard-injection attack class (cf. CVE-2017-12424).
- **Workspace sidebar: mouse selection and full keyboard navigation.** Tree rows are now selectable: a single click highlights an entry without opening it, and a double click opens a file or toggles a folder. With an entry selected (or the pointer over the sidebar) the tree is fully keyboard-drivable: `↑`/`↓` move the highlight, `→` expands a folder then steps into its first child, `←` collapses a folder then jumps to its parent, `Enter` opens a file or toggles a folder, and `F2` starts an inline rename. Keyboard ownership is isolated per panel, so `↑`/`↓`/`→`/`←`/`Enter`/`F2` act on whichever of the sidebar or editor you last engaged and never bleed across.
- **Settings: Copy Contents size limits.** Two rows under Settings → History: a hard **maximum copy size** (default 64 MB; `0` = no limit) above which a file's contents cannot be copied to the clipboard, and a **warn threshold** (default 5 MB) above which copying first shows a confirmation dialog with the file path, the size, and a caveat about OS clipboard history. Both are independent of the editor's maximum-file-to-open setting.

### Fixed
- **Crash when multi-selecting around multi-byte characters.** Selecting the next occurrence of a word (`Alt+Shift+.`) in text containing a multi-byte codepoint (e.g. an em dash `—`) could panic with a char-boundary error and lose the affected tabs' contents. Occurrence search now converts between character and byte offsets correctly.
- **Renaming a folder that appears in the workspace more than once.** When the same physical folder was present both as a workspace root and nested under another root (possible since overlapping roots were allowed in 2.8.1), an inline rename could land on the wrong copy and leave the workspace root pointing at the old name. The rename now targets the exact row you selected, and the new name propagates to every other row for that physical folder — updating and persisting the affected workspace root(s).
- **macOS: pointer stuck as a hand cursor after switching windows.** Returning to the window via Cmd/Alt-Tab could leave the cursor showing the folder-row hand until it crossed between panels a few times; the cursor is now reset when the window regains focus.

### Changes
- **Workspace sidebar icon refresh.** Replaced the placeholder emoji glyphs in the workspace tree, sidebar toolbar (close / add / hidden-files toggle / collapse-all / expand-all), workspace-rename context menu, and the inline new-file / new-folder / rename fields with a coherent set of monochrome Phosphor icons. The icons scale with the active text size and recolour with the current theme. The new vocabulary lives in `crates/rust-pad-ui/src/icons.rs` so any future swap is a one-file change.
- **Workspace tree rendering refactored around a `RenderCtx`.** The recursive `render_entry_list` and per-entry helpers now share a small context struct instead of nine positional arguments. The new struct also threads the owning workspace-root path down to every entry so the new Copy Path / Copy Contents handlers have it without re-walking the tree.

## [2.8.1]

### Added
- **Per-file view-state persistence**: cursor position and scroll offset are now remembered for every file you open and restored the next time the file is opened, across tab close, app restart, and session reopen.
- **Expand All / Collapse All** toolbar buttons in the Workspace sidebar that fold or unfold every workspace root in one click.
- **Stem selection on rename / new entry** in the Workspace sidebar: the part of the name before the last `.` is pre-selected (matching VS Code / IntelliJ behavior) so you can rewrite the base while keeping the extension.

### Fixed
- External-change prompt no longer surfaces for inactive tabs. The "file changed on disk" dialog only appears for the currently active document; switching to a flagged tab raises the prompt on the next frame. Previously, a stale flag on any background tab could pop the dialog out from under unrelated editing.
- Fixed Ctrl+V doing nothing in inline workspace text fields on macOS. egui's TextEdit only paste-handles `Cmd+V` on macOS, so Ctrl-only paste was dropped silently in rename / new-file / new-folder fields. Ctrl+V now synthesizes a Paste event into the focused field. Pasted text is sanitized, CR/LF/NUL are stripped and the result is rejected (with a Problems entry) if it is not a valid single-segment filename.
- Fixed Ctrl+A / Ctrl+C / Ctrl+V / Ctrl+X / Ctrl+Z / Ctrl+Y in inline workspace fields also affecting the editor. The editor-input suppression that already covered the workspace rename field is now extended to the new-entry and rename-entry fields too, so shortcuts in those text inputs no longer bleed through to the underlying document.
- Workspace folders that overlap (nested or parent of an existing root) are now allowed. Only exact-path duplicates are rejected. The previous "no overlap" rule blocked legitimate setups like adding both a monorepo root and one of its subprojects.
- Added symlink-loop detection when adding a workspace folder. Folders that walk back into themselves through symlinks (within the first 64 levels) are now rejected with a Problems entry instead of hanging the lazy scan.
- Filename validation on a new file, new folder, and rename in the workspace sidebar. Empty names, `.`, `..`, names containing `/` `\` NUL or control characters, absolute paths, and Windows drive prefixes (`C:`) are now rejected and surfaced in the Problems dialog, instead of silently creating files outside the intended directory.
- Capped workspace filesystem events at 1000 per UI tick. During large refactors or rapid bulk operations, an unbounded event queue could starve the UI. Surplus events stay queued for the next tick, and an overflow warning is logged at most once per minute.
- Coalesce duplicate `(kind, path)` filesystem events in a single drain. Overlapping recursive watchers can emit the same notification multiple times; deduplication bounds per-tick work amplification at N=1.
- Fixed the Workspace sidebar freezing when toggling Expand All on a large tree. The action was traversing every recursively rendered directory, which forced lazy-load of the entire reachable filesystem on a single frame. Bulk expand/collapse now only flips the workspace-root flags; descendants are expanded normally on click.
- Fixed syntax highlighting not refreshing existing text when an untitled tab is saved under a recognized extension (e.g., typing Markdown in a new tab and saving it as `*.md`). The galley cache only invalidated on colour-theme changes, so lines whose textual content had not changed kept their old Plain Text styling until they were retyped. The cache now also invalidates when the detected syntax language changes, while staying stable across same-language renames.

### Changes
- Updated dependencies egui, eframe, serde_json, egui_kittest, and pdf-writer to their latest stable versions.
- Internal refactor (no user-visible behaviour change): collapsed the duplicated bincode-deserialize-with-corruption-fallback pattern across the workspace, session, and view-state stores into a shared `deserialize_record` helper that preserves the per-store size limits, and collapsed the paired `tracing::warn!` + problem-log write pattern across the UI layer into `warn_problem` / `info_problem` helpers.

## [2.8.0]

### Added
- New **Trim Whitespace** line operations (Edit menu): trim trailing whitespace, leading whitespace, or both from every line in the selection (or the current line when there is no selection).
- New **Join Lines** operation (Edit menu): collapses the selected lines into a single line, with exactly one space at each junction (trailing/leading whitespace at the joins is stripped, while the leading whitespace of the first line and trailing whitespace of the last line are preserved).

### Fixed
- Fixed multi-cursor same-line replacement breaking the line. Selecting several instances of a word on one line (Alt+Shift+.) and typing a replacement scrambled the text because the per-cursor edits shifted each other's character offsets. Edits are now applied so earlier replacements no longer corrupt the positions of later ones.
- Fixed Tab / Shift+Tab on a multi-line selection only changing the line containing the cursor. Indent/dedent now applies to every line covered by the selection.
- Fixed pressing Tab with an active selection deleting the selected text instead of indenting it. A selection is now indented (or dedented with Shift+Tab) line-by-line, preserving its contents.
- Fixed the beginning of a multi-line selection escaping when indenting/dedenting. The selection's top endpoint is now pinned to the start of the first line so its growing/shrinking indentation stays inside the selection across repeated presses; the bottom endpoint keeps tracking its text.
- Fixed arrow Up/Down skipping whole logical lines when word-wrap is on. Vertical navigation now moves one *visual* (wrapped) line at a time, treating soft-wrapped segments like hard line breaks, with a sticky column that survives passing through shorter segments. Behavior is unchanged when word-wrap is off.

## [2.7.0]

### Added
- Drag-and-drop folders onto the application window to add them to the active workspace. If no workspace exists, one is created automatically. The sidebar is shown after the drop so the user can see the result.

### Fixed
- Suppressed false-positive SonarCloud security finding about missing `Cargo.lock` in workspace member crates. Cargo workspaces produce a single lock file at the root by design.

## [2.6.0]

### Added
- Added the ability to the **Workspace** sidebar to include hidden folders.

### Fixed
- Updated minimum window dimensions for the settings dialog to accommodate new content and stop it from wiggling around.
- Fixed toggle menu items (e.g., View -> Synchronized Scrolling) showing a square instead of a checkmark and being misaligned when off. Replaced manual text prefix with a proper checkbox widget.

## [2.5.0]

### Added

#### Workspace Sidebar
- New **Workspace** menu and sidebar panel (Ctrl+B to toggle) that provides a file explorer for organizing project folders into named workspaces.
- **Named workspaces**: Create, rename, switch, and delete workspaces. Each workspace groups a set of root folders and is persisted across app restarts in a dedicated database (`rust-pad-workspaces.redb`).
- **Folder management**: Add folders to a workspace via the menu or sidebar toolbar. Duplicate and overlapping (parent/child) folders are detected and rejected with a user-visible message. Remove folders from the workspace without deleting them from disk.
- **File tree**: Collapsible folder tree with lazy-loaded children (scanned on first expand, cached afterwards). Directories are listed before files, both sorted alphabetically (case-insensitive). Hidden files (starting with `.`) are filtered out. Large directories are capped at 10,000 entries.
- **File operations from sidebar**: Double-click a file to open it in the editor. Right-click context menus offer New File, New Folder, Rename, and Delete (send to trash) for files and directories. Inline text fields for naming new entries and renaming existing ones.
- **Real-time filesystem monitoring**: Workspace directories are watched via the `notify` crate with 500ms debouncing. New, modified, and deleted files update the tree incrementally without full re-scans.
- **Session persistence**: Sidebar visibility, width, and the active workspace are saved to configuration and restored on startup. The sidebar width is resizable (150–500px) via config.
- **Keyboard shortcut**: Ctrl+B toggles sidebar visibility. Enter confirms inline rename/create; Escape cancels. Key input is suppressed from the editor during inline edits to prevent bleed-through.
- **Inaccessible folder handling**: Root folders that become unavailable (unmounted drives, deleted directories) are shown with a warning icon and "(unavailable)" label instead of crashing.

## [2.4.4]

### Fixed

- Fixed syntax highlighting breaking across wrapped lines. Each wrapped segment was highlighted independently without context from the rest of the logical line, causing incorrect colors at wrap boundaries. The full line is now highlighted once and clipped to each segment's byte range.
- Fixed word-wrap flickering caused by `chars_per_line` oscillating between frames. The initial wrap map used `SCROLLBAR_WIDTH` while the rebuild path used `layout.vscroll_width`, which could differ. Both now consistently use the `SCROLLBAR_WIDTH` constant.
- Enabled galley caching for wrapped lines. Previously wrapped segments always bypassed the cache (`cache_line_idx` was `None`), causing redundant layout work every frame.
- Fixed galley cache pruning using logical line indices instead of visual line indices, which caused cache misses in wrapped mode.
- Added external file change detection for all open documents (not just live-monitored ones). When a file is modified outside the editor, a dialog now prompts the user to reload or keep their version.
- Added horizontal scroll support (overflow arrows, auto-scroll to active tab) to the split-pane tab bar, matching the main tab bar behavior.

## [2.4.3]

### Fixed

- Fixed pinned tabs not moving to the beginning of the tab bar in split view. The pin/unpin operations reordered the global document vector but did not reorder the per-pane order vectors, so pinned tabs stayed in place visually. Added a stable-partition step that reorders the affected pane's tab order after each pin/unpin.

## [2.4.2]

### Fixed

- Fixed text appearing in both panels when typing in split view. Both pane editors were unconditionally grabbing focus and processing the same frame-global keyboard events. Added an `auto_focus` flag to `EditorWidget` so only the focused pane's editor requests focus and processes keyboard input.
- Fixed zoom being global instead of per-file/panel. Moved the zoom level from `ThemeController` (single global value) to each `Document`, so each tab maintains its own independent zoom. Zoom changes via Ctrl+Scroll, Ctrl+Plus/Minus, and the View menu now target the active document only.

### Changed

- `ThemeController::zoom_level` renamed to `default_zoom_level` — it now only sets the initial zoom for newly-created documents and is persisted to config on exit. The per-document zoom starts at this default.
- Removed `zoom_in()`, `zoom_out()`, and `zoom_reset()` methods from `ThemeController`; zoom mutations are now applied directly to `Document::zoom_level`.
- Removed the `zoom_request` output field from `EditorWidget` and the associated propagation logic in the single-pane and split-pane render paths.
- Status bar now displays the active document's zoom level rather than the global zoom.

## [2.4.1]

### Fixed

- Fixed a crash when clicking on an empty line (or empty document) with word wrap enabled. The wrap-row offset could exceed the line length, causing a slice-out-of-bounds panic in `screen_to_position`.
- Panics are now captured by the problem logger (Help > Problems) via a custom panic hook, so crash information is visible to users after a restart instead of being lost.

### Changed

- Extracted shared database-opening boilerplate (`open_or_create_db`) into a reusable helper, eliminating code duplication between `ProblemStore::open()` and `SessionStore::open()`.
- Removed leftover mock problem entries added during development.

## [2.4.0]

### Added

#### Problems Panel (Help → Problems)
- New **Help → Problems** menu entry opens a dialog listing all application errors and warnings in reverse chronological order (newest first). Each entry shows a timestamp and a human-readable error message.
- Users can **mark individual entries as read**, **mark all as read**, **copy** an error message to the clipboard, or **clear** the entire log.
- The **Help** menu shows a warning indicator (`Help ⚠`) and an unread count badge (e.g. `Problems (3)`) whenever there are unread entries, providing at-a-glance visibility into background failures.
- Problem entries are stored in a dedicated crash-safe database (`rust-pad-problems.redb`) so they survive unexpected termination. The database is created in the platform-standard data directory (or next to the executable in portable mode).
- The following errors are now captured in the problem log:
  - File open failures (encoding errors, I/O errors)
  - Auto-save failures
  - Live reload failures
  - Print / Export-as-PDF failures
  - Document encoding failures on save
  - Reload-from-disk failures
  - File recovery failures
  - Undo history database open failures (startup)
  - Session store open failures (startup)
  - Session tab restore failures (startup)
  - CLI file open failures (startup)
  - History flush/delete failures
  - Session and config save failures (shutdown)
- The Problems dialog can be dismissed with the Escape key, consistent with all other dialogs.

## [2.3.0]

### Added

- **Auto-focus find input**: The Find/Replace dialog now automatically focuses the find text field when opened, so the user can start typing immediately without clicking.
- **Copy selection into find field**: Pressing Ctrl+F or Ctrl+H with text selected automatically populates the find field with the selected text (single-line selections only).
- **Scroll to match on Find Next/Prev**: The viewport now scrolls to keep the current match visible when cycling through results with Find Next or Find Prev.
- **Search history dropdown**: A small dropdown button next to the find field lets the user recall recent search queries (session-only, up to 20 entries, deduplicated). History is recorded on Find Next, Find Prev, Replace, and Replace All actions.
- **Non-modal Find/Replace**: The Find/Replace dialog no longer blocks editing. Users can click into the editor to type, use shortcuts, and make changes while the dialog stays open. The dialog becomes semi-transparent when it loses focus, providing a clear visual cue of which surface is active.
- **Default line ending setting**: New "Default Line Ending" option in Settings → Editor. Choose between System (OS default), Unix (LF), or Windows (CRLF) for new documents. Files opened from disk keep their detected line ending.

## [2.2.0]

### Added

- **Drag and Drop**: Files can now be dragged and dropped onto the editor window to open them. A translucent overlay with a "Drop file(s) to open" hint is shown while hovering. Multiple files can be dropped at once; duplicates are detected and switched to automatically.
- **Escape closes all dialogues**: All dialogs (Settings, About, Unsaved Changes, Reload from Disk, File Too Large, File Open Error, Print Error) can now be dismissed with the Escape key. Dialogs close one per keypress in priority order, from most-modal to least-modal.

### Changed

- Renamed the "Cancel" button in the Go to Line dialog to "I'm not going anywhere".

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