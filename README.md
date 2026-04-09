<p align="center">
  <img src="assets/logo.png" alt="rust-pad logo" width="256" />
</p>
<h1 align="center">rust-pad</h1>
<p align="center">
  A cross-platform notepad application built using Rust, and inspired on Notepad++
</p>

---
[![Release](https://github.com/brenordv/rust-pad/actions/workflows/release.yml/badge.svg)](https://github.com/brenordv/rust-pad/actions/workflows/release.yml)
[![Quality Gate Status](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=alert_status)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Reliability Rating](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=reliability_rating)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Security Rating](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=security_rating)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Maintainability Rating](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=sqale_rating)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Bugs](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=bugs)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Vulnerabilities](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=vulnerabilities)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Technical Debt](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=sqale_index)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Code Smells](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=code_smells)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Coverage](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=coverage)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Duplicated Lines (%)](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=duplicated_lines_density)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
[![Lines of Code](https://sonarcloud.io/api/project_badges/measure?project=brenordv_rust-pad&metric=ncloc)](https://sonarcloud.io/summary/new_code?id=brenordv_rust-pad)
---

## Motivation
I absolutely love Notepad++, so whenever I consider moving away from Windows, I end up looking for a way to run it on Linux.
Since I prefer native applications, I decided to take the longer route and write my own text editor in Rust.
This isn’t a port of Notepad++, and it doesn’t include all of its features—nor am I trying to compete with it.
Instead, my goal is to build a cross-platform Notepad-like editor with a few neat features, keeping it as simple,
stable, and fast as possible.

## Features

### Editing
- **Multi-tab interface** with session restore (reopen files from last session) and horizontal tab scrolling when tabs overflow
- **Syntax highlighting** powered by syntect (78+ languages)
- **Find/Replace** with regex support and search across all open tabs
- **Multi-cursor editing**: `Ctrl+Click` to add cursors, `Alt+Shift+Arrow` to add the cursors above/below (with shrink support), `Alt+Shift+Period` to select next occurrence
- **Undo/Redo** with persistent history (survives application restart)
- **Auto-indent**: pressing Enter inherits the leading whitespace from the current line
- **Bracket matching**: highlights matching `()`, `[]`, `{}` pairs when the cursor is adjacent to a bracket
- **Bookmarks**: toggle (`Ctrl+F2`), navigate (`F2 / Shift+F2`), clear all
- **Context menu**: right-click for clipboard actions, selection operations, case conversion, and line operations scoped to selection or entire document
- **Go to Line** dialog (`Ctrl+G`)
- **Delete current line** (`Ctrl+D`)

### Line Operations
- Sort lines ascending/descending
- Remove duplicate lines
- Remove empty lines
- Move line up/down (Alt+Up / Alt+Down)
- Duplicate line
- Increase/Decrease indent (Tab / Shift+Tab)

### Selection
- **Invert Selection**: toggles selection (no selection -> select all, full selection -> clear, partial -> invert to unselected regions)

### Case Conversion
- UPPERCASE
- lowercase
- Title Case

### File Handling
- **Recent files**: quick reopen from the File > Open Recent submenu (configurable max count and cleanup strategy)
- **Live file monitoring** (`tail -f mode`): auto-refresh when file changes on disk
- **Auto-save** for file-backed documents (configurable interval)
- **Async file I/O**: file dialogs, reads, and writes run on background threads with a status bar spinner, keeping the UI responsive
- **File size validation**: configurable size limit (default 512 MB) prevents out-of-memory crashes from opening very large files
- **Encoding support**: UTF-8, UTF-8 with BOM, UTF-16 LE, UTF-16 BE, ASCII
- **Line ending conversion**: LF (Unix), CRLF (Windows), CR (classic Mac)
- **Indent style**: spaces (2/4/8) or tabs, with auto-detection
- **Move to Recycle Bin** support via the `trash` crate

### View
- **Customizable themes**: Dark, Light, and custom themes via JSON
- **Settings dialog** with five tabs: General, Editor, File Dialogs, Auto-Save, History
- **Status bar** displaying: cursor position, encoding, line ending, indent style, character count, file size, zoom level, and last saved time
- **Word wrap** toggle
- **Special character visualization** (whitespace, line endings)
- **Line number gutter** with change tracking (orange = unsaved changes, green = saved changes)
- **Zoom**: in (`Ctrl++`), out (`Ctrl+-`), reset (`Ctrl+0`): range 50% to 1500%

### Platform & CLI
- **Cross-platform**: Windows, macOS, Linux
- **CLI support**: open files from the command line, `--new-file` flag to create a tab with given text
- **Portable mode** (`--portable` flag): store all config and data next to the executable for USB/portable installs
- **Platform-standard directories**: config and data stored in OS-standard locations (`%APPDATA%`, `~/.config`, `~/Library/Application Support`) with automatic migration from older versions
- **Security hardening**: data directories (0700) and database files (0600) use restrictive permissions on Unix; bounded deserialization prevents OOM from corrupted databases
- **Release integrity**: SHA256 checksums published alongside release binaries for download verification
- **Custom application icon**

---

## Keyboard Shortcuts

### File Operations

| Shortcut     | Action    |
|--------------|-----------|
| Ctrl+N       | New tab   |
| Ctrl+O       | Open file |
| Ctrl+S       | Save      |
| Ctrl+Shift+S | Save As   |
| Ctrl+W       | Close tab |

### Editing

| Shortcut  | Action              |
|-----------|---------------------|
| Ctrl+Z    | Undo                |
| Ctrl+Y    | Redo                |
| Ctrl+X    | Cut                 |
| Ctrl+C    | Copy                |
| Ctrl+V    | Paste               |
| Ctrl+A    | Select all          |
| Ctrl+D    | Delete current line |
| Tab       | Increase indent     |
| Shift+Tab | Decrease indent     |

### Navigation & Search

| Shortcut | Action            |
|----------|-------------------|
| Ctrl+F   | Open Find/Replace |
| Ctrl+H   | Open Find/Replace |
| Ctrl+G   | Go to Line        |
| Ctrl+F2  | Toggle bookmark   |
| F2       | Next bookmark     |
| Shift+F2 | Previous bookmark |

### Multi-Cursor

| Shortcut         | Action                       |
|------------------|------------------------------|
| Ctrl+Click       | Add cursor at click position |
| Alt+Shift+Up     | Add cursor above             |
| Alt+Shift+Down   | Add cursor below             |
| Alt+Shift+Period | Select next occurrence       |
| Escape           | Clear secondary cursors      |

### Line Movement

| Shortcut | Action         |
|----------|----------------|
| Alt+Up   | Move line up   |
| Alt+Down | Move line down |

### Tabs

| Shortcut       | Action       |
|----------------|--------------|
| Ctrl+Tab       | Next tab     |
| Ctrl+Shift+Tab | Previous tab |

### Zoom

| Shortcut | Action     |
|----------|------------|
| Ctrl++   | Zoom in    |
| Ctrl+-   | Zoom out   |
| Ctrl+0   | Reset zoom |

---

## Configuration

rust-pad stores its configuration in a `rust-pad.json` file in the platform-standard config directory:

| Platform | Config directory                          |
|----------|-------------------------------------------|
| Windows  | `%APPDATA%\rust-pad\`                     |
| macOS    | `~/Library/Application Support/rust-pad/` |
| Linux    | `~/.config/rust-pad/`                     |

Data files (`history.redb`, `rust-pad-session.redb`) are stored in the platform-standard data directory (same as config on Windows/macOS; `~/.local/share/rust-pad/` on Linux).

When running with `--portable`, all files are stored next to the executable instead. The `RUST_PAD_DATA_DIR` environment variable can override the history data directory location.

The config file is created automatically on the first launch with default values. If upgrading from an older version that stored files next to the executable, they are automatically migrated (copied) to the new location.

### Configuration Fields

| Field                     | Type   | Default                       | Description                                                                                                           |
|---------------------------|--------|-------------------------------|-----------------------------------------------------------------------------------------------------------------------|
| `current_theme`           | string | `"System"`                    | Active theme name. `"System"` follows OS dark/light preference. Can be `"Dark"`, `"Light"`, or any custom theme name. |
| `current_zoom_level`      | float  | `1.0`                         | Current zoom multiplier (0.5 to `max_zoom_level`).                                                                    |
| `max_zoom_level`          | float  | `15.0`                        | Maximum allowed zoom level (minimum 1.0). Note: Values over 15 will start to degrade performance.                     |
| `word_wrap`               | bool   | `false`                       | Whether long lines wrap at the view edge.                                                                             |
| `show_special_chars`      | bool   | `false`                       | Show whitespace and line-ending markers.                                                                              |
| `show_line_numbers`       | bool   | `true`                        | Display the line number gutter.                                                                                       |
| `restore_open_files`      | bool   | `true`                        | Reopen files from the previous session on startup.                                                                    |
| `show_full_path_in_title` | bool   | `true`                        | Show the full file path in the window title bar.                                                                      |
| `font_size`               | float  | `16.0`                        | Base font size in points (6.0 to 72.0).                                                                               |
| `default_extension`       | string | `""`                          | Default file extension for new untitled tabs (e.g. `"txt"`, `"md"`). Empty means none.                                |
| `remember_last_folder`    | bool   | `true`                        | Remember the last folder used in open/save dialogs.                                                                   |
| `default_work_folder`     | string | `""`                          | Default starting folder for file dialogs. Empty uses the user's home directory.                                       |
| `last_used_folder`        | string | `""`                          | Persisted last folder from open/save dialogs (managed automatically).                                                 |
| `auto_save_enabled`       | bool   | `false`                       | Enable periodic auto-save for file-backed documents.                                                                  |
| `auto_save_interval_secs` | int    | `30`                          | Seconds between auto-saves (minimum 5).                                                                               |
| `recent_files_enabled`    | bool   | `true`                        | Enable the recent files feature.                                                                                      |
| `recent_files_max_count`  | int    | `10`                          | Maximum number of recent files to remember (1 to 50).                                                                 |
| `recent_files_cleanup`    | string | `"OnStartup"`                 | When to remove non-existent files: `"OnStartup"`, `"OnMenuOpen"`, or `"Both"`.                                        |
| `recent_files`            | array  | `[]`                          | Most-recently-opened file paths (managed automatically).                                                              |
| `max_file_size_mb`        | int    | `512`                         | Maximum file size in MB to open (0 = no limit, 1 to 10240).                                                           |
| `session_content_max_kb`  | int    | `10240`                       | Maximum KB of unsaved content to persist per tab (0 = unlimited). Tabs exceeding this are restored empty.             |
| `themes`                  | array  | (built-in Dark, Light, Wacky) | Array of theme definitions. See below.                                                                                |

### Custom Themes

Themes are defined as JSON objects within the `themes` array. Each theme has a `name` and color definitions for the editor and UI elements. Built-in `Dark` and `Light` themes are always present; if removed from the config file they will be re-added automatically.

#### Syntax Theme

Each theme includes a `syntax_theme` field that controls the color scheme used for **syntax highlighting** (keywords, strings, comments, types, etc.). This is separate from the editor/UI colors which control backgrounds, gutters, and scrollbars.

The value must be one of the theme names bundled with [syntect](https://github.com/trishume/syntect). The default themes are:

| Value                    | Style                                                                    |
|--------------------------|--------------------------------------------------------------------------|
| `"base16-eighties.dark"` | Warm, muted dark palette (default for Dark theme)                        |
| `"base16-ocean.dark"`    | Cool blue-tinted dark palette                                            |
| `"base16-mocha.dark"`    | Soft brown-tinted dark palette                                           |
| `"base16-ocean.light"`   | Cool blue-tinted light palette                                           |
| `"InspiredGitHub"`       | Light palette based on GitHub's code rendering (default for Light theme) |
| `"Solarized (dark)"`     | Ethan Schoonover's Solarized dark                                        |
| `"Solarized (light)"`    | Ethan Schoonover's Solarized light                                       |

You can also load custom `.tmTheme` files (TextMate/Sublime Text theme format) by placing them alongside the executable. See the [syntect documentation](https://docs.rs/syntect/latest/syntect/highlighting/struct.ThemeSet.html) for details on loading additional themes.

#### Theme Colors

Theme colors are specified as hex strings (`"#RRGGBB"` or `"#RRGGBBAA"`) and cover:

- **Editor colors**: background, text, cursor, selection, line numbers, line number background, current line highlight, modified/saved line indicators, gutter separator, scrollbar (track, thumb idle/hover/active), occurrence highlight, matching bracket highlight, special character color
- **UI colors**: inherited from egui's built-in visuals, overridden per-theme

Example custom theme entry:

```json
{
  "name": "My Theme",
  "editor": {
    "bg_color": "#1E1E1E",
    "text_color": "#D4D4D4",
    "cursor_color": "#FFFFFF",
    "selection_color": "#326EC864",
    "line_number_color": "#787878",
    "line_number_bg": "#252525",
    "current_line_highlight": "#2D2D2D",
    "modified_line_color": "#E6961E",
    "saved_line_color": "#50B450",
    "gutter_separator_color": "#3C3C3C",
    "scrollbar_track_color": "#232323",
    "scrollbar_thumb_idle": "#505050",
    "scrollbar_thumb_hover": "#6E6E6E",
    "scrollbar_thumb_active": "#8C8C8C",
    "occurrence_highlight_color": "#64643250",
    "matching_bracket_color": "#B4A03C5A",
    "special_char_color": "#646464B4"
  }
}
```

## Installation

### Download from Releases

Pre-built binaries are available on the [Releases](https://github.com/brenordv/rust-pad/releases) page for Windows, macOS, and Linux.

1. Download the archive for your platform.
2. Extract it.
3. Run the `rust-pad` executable.

### Build from Source

Requirements:
- [Rust](https://www.rust-lang.org/tools/install) (stable toolchain)
- **Linux only**: a C compiler and system development libraries:
  ```bash
  # Debian/Ubuntu
  sudo apt-get install -y build-essential libxcb-render0-dev libxcb-shape0-dev \
    libxcb-xfixes0-dev libxkbcommon-dev libssl-dev libgtk-3-dev
  ```

```bash
git clone https://github.com/brenordv/rust-pad.git
cd rust-pad
cargo build --release
```

The binary will be at `target/release/rust-pad` (or `rust-pad.exe` on Windows).

### CLI Usage

```bash
# Open files directly
rust-pad file1.txt file2.rs

# Create a new tab with initial text
rust-pad --new-file "Hello, world!"

# Portable mode: store config and data next to the executable
rust-pad --portable
```

## File Size Limits

By default, rust-pad refuses to open files larger than **512 MB** to prevent out-of-memory crashes. This limit is configurable via the `max_file_size_mb` setting (0 = no limit, max 10240 MB) or through the Settings dialog under the History tab.

Internally, rust-pad uses the **ropey** crate, which stores text in a B-tree of small chunks. This data structure provides:

- **O(log n)** random access to any character or line
- **O(log n)** insertions and deletions at any position
- **O(n)** memory usage where n is the text size

The practical limit beyond the configured cap is system memory. The rope data structure is far more efficient than a flat string buffer for large files, since edits do not require copying the entire document.

---

## Planned Features

The following features are planned for future releases, inspired by Notepad++ functionality:

### Editor
- [ ] Code folding (collapse/expand blocks by language rules)
- [ ] Auto-completion (keyword and word suggestions)
- [ ] Column editor (insert text/numbers into column selections)
- [ ] Smart backspace (remove full indent level)
- [ ] Edge/ruler line at a configurable column

### View
- [ ] Split view / dual panes (edit two files side by side)
- [ ] Document map (minimap overview of the file)
- [ ] Function list panel (outline of functions/methods)
- [ ] Distraction-free mode (full-screen, no UI chrome)
- [ ] Always on top mode
- [ ] Hide lines (temporarily collapse lines without deleting)
- [ ] Synchronized scrolling between two panes

### File
- [ ] Print support
- [ ] Open containing folder / open in terminal
- [ ] Reload from disk (discard unsaved changes)
- [ ] Save a Copy (save to a different path without changing the active file)
- [ ] Find in Files (search across files in a directory)

### Tabs
- [ ] Pin tabs (exclude from "Close All")
- [ ] Tab coloring
- [ ] Close All / Close All But Active / Close Unchanged
- [ ] Drag-and-drop tab reordering

### Macro Support
- [ ] Record/playback macros (capture sequences of edits)
- [ ] Save macros with names and hotkeys
- [ ] Run macro multiple times / until end of file

### Plugin System
- [ ] Plugin architecture for extending functionality
- [ ] User-defined language definitions

### Other
- [ ] Clipboard history panel
- [ ] Character panel (insert special characters)
- [ ] Date/time insertion
- [ ] Run external commands with file path placeholders
- [ ] Shortcut mapper (customizable keybindings)
- [ ] Right-to-left text support
- [ ] Import/export settings

---

## License

This project is licensed under the [GNU General Public License v3.0](LICENSE.md).