# rust-pad — UI Guidelines

> Updated for the current redesign. Supersedes the earlier "Material-on-desktop"
> draft: rust-pad no longer targets Material's purple palette or mobile
> component set. It has its **own** teal-accented desktop identity, built to be
> implemented cleanly in **Rust + egui**, and shipped as user-loadable themes.

---

## 1. Executive Summary

rust-pad is a keyboard-first, multi-document desktop notepad. The redesign keeps
the existing mental model (menu bar, tabs, editor, status bar) and modernizes the
*styling* while adding three structural pieces borrowed from professional IDEs
(JetBrains Rider as the reference):

1. a slim **activity bar** on the far left,
2. a resizable **Workspace explorer** (folder/file tree),
3. a **breadcrumb** strip above the editor.

Two visual directions are defined and should not be mixed within a build:

| Direction                  | Feel                         | Corner radius | Density | Base bg (dark) | Accent (dark) |
|----------------------------|------------------------------|---------------|---------|----------------|---------------|
| **Aurora Teal** (`aurora`) | Soft, rounded, blue-charcoal | 6–7 px        | Roomy   | `#151A20`      | `#2DD4BF`     |
| **Graphite** (`graphite`)  | Sharp, dense, near-black     | 2–3 px        | Tight   | `#0E0F13`      | `#2FE3AE`     |

Both ship **full light + dark themes**, integrate with the OS light/dark
preference, meet **WCAG AA contrast (≥4.5:1)** for body text, support **full
keyboard + numpad** control, and animate sparingly (100–250 ms, honoring
"reduce motion"). Everything here is expressed as concrete egui values and as
entries in rust-pad's existing theme-JSON schema so a theme can be reproduced
without touching code.

---

## 2. Design Language

- **Accent:** teal/green. Aurora `#2DD4BF`, Graphite `#2FE3AE`. On light themes
  the accent is darkened (~0.6×) so text/icons on light surfaces keep contrast.
  Accent is used for: active tab top-bar, active tree/tab selection, cursor,
  primary buttons, focus rings, links, and "unread/active" counts. **Never** use
  it as a large fill behind body text.
- **Neutrals:** a single desaturated blue-charcoal ramp (Aurora) or a near-black
  gray ramp (Graphite). No pure `#000`/`#FFF` chrome; editor surfaces may reach
  `#FBFCFE` (light) / `#151A20`–`#0E0F13` (dark).
- **Typography:**
    - **UI:** IBM Plex Sans (400/500/600). Fallbacks by platform — Segoe UI
      (Windows), SF Pro (macOS), Inter/Roboto (Linux).
    - **Editor + code + timestamps:** JetBrains Mono (400/500).
    - Sizes: UI body **13 px**, section labels 11 px (uppercase, +0.13em tracking),
      dialog titles 14 px/600, editor **13 px** at line-height 1.62 (Aurora) /
      1.55 (Graphite). Never below 11 px for chrome, 12 px for content.
- **Grid & spacing:** 8-px base grid (4-px allowed for dense inline rows).
  Chrome row heights: title bar 38, menu bar 32, tab strip 38, breadcrumb 29,
  status bar 27, tree row 26, activity item 38.
- **Elevation:** flat panels separated by **1-px borders**, not shadows. Reserve
  soft shadows for floating surfaces only (menus, dialogs).
- **Iconography:** 1.6-px stroke line icons on a 24-px grid (18–19 px drawn in
  chrome, 14–15 px in the tree/dialogs). Outline style; color follows the
  surface's text/muted role, accent only when active.

---

## 3. App Structure & Layout

Left-to-right, the main window is:

```
┌────────────────────────────────────────────────────────────────┐
│ Title bar:  ● app · "Untitled 3 ● rust-pad"        _  ▢  ✕     │
├────────────────────────────────────────────────────────────────┤
│ Menu bar:  File Edit Search Encoding View Settings … Help ●    │
├────┬──────────────┬────────────────────────────────────────────┤
│ A  │ WORKSPACE  ⊞ │ Tabs:  ▸docs.txt  ●Untitled 3 ✕  +   ‹ ›   │
│ C  │ ▾ rust-pad   ├────────────────────────────────────── ─────┤
│ T  │   ▸ assets   │ Breadcrumb: rust-pad › documents › Untitled│
│ I  │   ▾ notes    ├────────────────────────────────────────────┤
│ V  │      plan.md │  1  About the treatment panel:           ↵ │
│ I  │    ●Untit… 3 │  2  1. The fields should take …          ↵ │
│ T  │   README.md  │  …                                         │
│ Y  │ ▾ archive    │ 12  This configuration allows … │          │
├────┴──────────────┴────────────────────────────────────────────┤
│ Status:  Ln 12, Col 96 │ UTF-8 │ CRLF │ Spaces: 4 │ … │ ●Auto  │
└────────────────────────────────────────────────────────────────┘
```

### 3.1 Activity bar
- Width **52 px** (Aurora) / **46 px** (Graphite). Filled with the chrome color.
- Vertical icons: **Files** (toggles the Workspace panel), **Search**,
  **Problems** (with unread count badge), **Source control**; **Settings**
  pinned to the bottom.
- **Active item** = accent icon. Aurora shows a rounded accent-tint pill;
  Graphite shows a 2-px accent **left bar**. Hover = accent-tint background.
- egui: `SidePanel::left("activity").exact_width(52.0).resizable(false)` with
  vertical `ImageButton`s; paint the active pill/bar via `ui.painter().rect_filled`.

### 3.2 Workspace explorer (the file tree)
This is a first-class feature — the panel between the activity bar and the editor.
- Width **248 px** (Aurora) / **232 px** (Graphite); **resizable** by dragging the
  right edge; toggled by the activity bar's Files icon. Persist width + collapsed
  state per workspace.
- **Header:** "WORKSPACE" label (11 px uppercase, muted) + right-aligned actions:
  New File, New Folder, Collapse All.
- **Rows:** 26 px tall. `indent = 12 + depth × 15 px`. Folders get a chevron
  (rotates 90° when open) + folder glyph; files get an aligned spacer + file
  glyph. Open folders may tint their glyph with the dimmed accent.
- **Selection = the open document.** The active file uses the *same* treatment as
  its active tab: accent text, accent-tint row background, and a 2-px accent
  left bar — so tree and tabs always agree.
- egui: a scrollable `ui.vertical` inside the side panel; each row is one
  `Response` (`sense(hover|click)`), painting its own hover/selected background;
  a `collapsing`-style chevron per folder. Double-click a file to open, single
  click to preview.

### 3.3 Tabs
- 38-px strip on the editor column. Active tab = editor-bg fill + **2-px accent
  top bar** + 600 weight. Inactive = transparent + muted text.
- **Modified indicator:** a filled accent **dot**, replacing the legacy `*`.
- Pinned tabs show a small pin glyph; a close `✕` appears on the active/hovered
  tab. Right side of the strip: overflow chevrons `‹ ›` + a `＋` new-tab button.
- egui: horizontal `ScrollArea` of custom selectable labels; paint the top bar
  and dot manually.

### 3.4 Breadcrumb
- 29-px strip above the editor: `folder › folder › file` using muted labels, a
  faint `›` separator, and the current file in full-strength text. Right-aligned
  file-type hint (e.g. `txt`). Purely informational in v1.

### 3.5 Editor
- Gutter: right-aligned line numbers (12 px, faint) with a 1-px separator; the
  **active line** number takes the accent.
- Body: JetBrains Mono, current-line highlight, selection tint from the accent,
  a blinking accent caret.
- **Line-end markers** replace the old chunky `CR`/`LF` chip pair:
    - Aurora → a single faint `↵` glyph (special-char color).
    - Graphite → one compact `CRLF` chip.
    - Both are toggleable under **View → Show line endings**.
- Soft-wrap long lines at the window edge (no horizontal document scroll by
  default); comfortable measure ~90 chars before wrap.

### 3.6 Status bar
- 27 px, segmented with 1-px dividers: `Ln, Col`, encoding, EOL, indentation,
  line count, char count, size — then right-aligned `Zoom` and `Auto-Save`
  (with a "saved" green dot).
- **Cursor segment emphasis:** Graphite paints the `Ln, Col` segment with a
  filled accent rect (dark text); Aurora simply colors that text with the accent.
- egui: `TopBottomPanel::bottom("status").exact_height(27.0)` with a horizontal
  layout; separators via 1-px `rect_filled`.

---

## 4. Color Tokens & Theme JSON

rust-pad already loads user themes. Every direction/mode below is expressible in
the existing `editor` schema. Map the same values into egui `Visuals` for chrome.

### 4.1 egui `Visuals` (chrome) mapping

| Role                 | egui field                         | Aurora dark   | Graphite dark |
|----------------------|------------------------------------|---------------|---------------|
| Window fill          | `window_fill`                      | `#151A20`     | `#0E0F13`     |
| Panel fill           | `panel_fill`                       | `#171C24`     | `#131519`     |
| Extreme bg (inputs)  | `extreme_bg_color`                 | `#0F141A`     | `#0A0B0D`     |
| Text                 | `override_text_color`              | `#C8D2DC`     | `#D7DCE2`     |
| Selection fill       | `selection.bg_fill`                | accent @ ~15% | accent @ ~14% |
| Hyperlink / accent   | `hyperlink_color`                  | `#2DD4BF`     | `#2FE3AE`     |
| Separators / borders | `widgets.noninteractive.bg_stroke` | `#242C36`     | `#1D2027`     |
| Corner radius        | `widgets.*.corner_radius`          | `6.0`         | `2.0`         |
| Window radius        | `window_corner_radius`             | `7.0`         | `3.0`         |

Light themes use the darkened accent, `window_fill` `#FBFCFE`/`#FFFFFF`,
`override_text_color` `#2B333C`/`#22262C`, and borders `#DEE5EC`/`#E4E7EC`.

### 4.2 Editor theme JSON (paste-ready)

**Aurora Teal — Dark**
```json
{
  "name": "Aurora Teal — Dark",
  "editor": {
    "bg_color": "#151A20",
    "current_line_highlight": "#1C232C",
    "cursor_color": "#2DD4BF",
    "gutter_separator_color": "#242C36",
    "line_number_bg": "#151A20",
    "line_number_color": "#5B6572",
    "matching_bracket_color": "#2DD4BF5A",
    "modified_line_color": "#E0A93C",
    "occurrence_highlight_color": "#2DD4BF33",
    "saved_line_color": "#4FB86A",
    "scrollbar_thumb_active": "#7E8B98",
    "scrollbar_thumb_hover": "#5E6A76",
    "scrollbar_thumb_idle": "#3A434E",
    "scrollbar_track_color": "#171C24",
    "selection_color": "#2DD4BF59",
    "special_char_color": "#5B6572B4",
    "text_color": "#C8D2DC"
  }
}
```

**Aurora Teal — Light**
```json
{
  "name": "Aurora Teal — Light",
  "editor": {
    "bg_color": "#FBFCFE",
    "current_line_highlight": "#EAF3F1",
    "cursor_color": "#12897B",
    "gutter_separator_color": "#DEE5EC",
    "line_number_bg": "#F3F6F9",
    "line_number_color": "#9BA7B2",
    "matching_bracket_color": "#12897B5A",
    "modified_line_color": "#C4881F",
    "occurrence_highlight_color": "#12897B33",
    "saved_line_color": "#3E9E5A",
    "scrollbar_thumb_active": "#9AA6B2",
    "scrollbar_thumb_hover": "#B4BEC8",
    "scrollbar_thumb_idle": "#CBD3DB",
    "scrollbar_track_color": "#EFF3F7",
    "selection_color": "#12897B59",
    "special_char_color": "#9BA7B2B4",
    "text_color": "#2B333C"
  }
}
```

**Graphite — Dark**
```json
{
  "name": "Graphite — Dark",
  "editor": {
    "bg_color": "#0E0F13",
    "current_line_highlight": "#15181D",
    "cursor_color": "#2FE3AE",
    "gutter_separator_color": "#1D2027",
    "line_number_bg": "#0E0F13",
    "line_number_color": "#4F5560",
    "matching_bracket_color": "#2FE3AE5A",
    "modified_line_color": "#E0A93C",
    "occurrence_highlight_color": "#2FE3AE33",
    "saved_line_color": "#42C088",
    "scrollbar_thumb_active": "#828994",
    "scrollbar_thumb_hover": "#5E656F",
    "scrollbar_thumb_idle": "#363C45",
    "scrollbar_track_color": "#131519",
    "selection_color": "#2FE3AE59",
    "special_char_color": "#4F5560B4",
    "text_color": "#D7DCE2"
  }
}
```

**Graphite — Light**
```json
{
  "name": "Graphite — Light",
  "editor": {
    "bg_color": "#FFFFFF",
    "current_line_highlight": "#EFF6F3",
    "cursor_color": "#118466",
    "gutter_separator_color": "#E4E7EC",
    "line_number_bg": "#FAFBFC",
    "line_number_color": "#A3AAB3",
    "matching_bracket_color": "#1184665A",
    "modified_line_color": "#C4881F",
    "occurrence_highlight_color": "#11846633",
    "saved_line_color": "#2FA772",
    "scrollbar_thumb_active": "#9BA2AC",
    "scrollbar_thumb_hover": "#B6BCC4",
    "scrollbar_thumb_idle": "#CDD2D9",
    "scrollbar_track_color": "#F4F5F8",
    "selection_color": "#11846659",
    "special_char_color": "#A3AAB3B4",
    "text_color": "#22262C"
  }
}
```

> Semantic colors (shared): warning/modified `#E0A93C` (dark) / `#C4881F`
> (light), error `#E0443E`, success/saved as above. The 8-digit hex suffixes
> (`5A`, `33`, `59`, `B4`) are alpha — keep them when hand-editing.

---

## 5. Component Specs

- **Primary button:** accent fill, `onAccent` text (`#08120F` on dark accents,
  `#FFFFFF` on light accents), 600 weight, 6-px (Aurora) / 2-px (Graphite)
  radius, 7×14 padding. Used once per surface (e.g. Find Next).
- **Secondary button:** `btnBg` fill + 1-px border; hover raises the border and
  text to the accent. 5–7 px vertical padding.
- **Inputs:** `extreme_bg` fill, 1-px border, 6/2-px radius, 32-px height.
  **Focus:** 1.5-px accent border + a 3-px accent-tint outer ring (the focus
  indicator; never remove it).
- **Checkbox / radio:** 15 px; checked = accent fill + check / accent inner dot.
- **Tree row / list item:** 26 px, hover accent-tint, selected = accent-tint +
  2-px accent left bar.
- **Menu (dropdown):** floating surface with a soft shadow, 5-px inner padding,
  6-px item radius, hover accent-tint. Items show a trailing count/shortcut hint
  in the accent or muted color. One level of cascading; separators between
  groups; disabled items at ~50% opacity (never hidden).
- **Dialogs (Problems, Find & Replace, Find Results):** header uses `dialogHead`
  fill + 1-px divider + title 14 px/600 + a close `✕`. Body on `dialogBg`. Rows
  divided by 1-px borders. Find Results carries its own mini status bar footer.

**Material → rust-pad component map** (what replaces the old table):

| Old Material notion      | rust-pad desktop equivalent                                  |
|--------------------------|--------------------------------------------------------------|
| Top app bar              | Title bar + menu bar (+ optional toolbar)                    |
| Bottom nav / FAB         | **Removed** — use the activity bar + menu/toolbar actions    |
| Navigation drawer        | **Workspace explorer** side panel                            |
| Tabs                     | Document tab strip with accent top-bar + modified dot        |
| Snackbar / toast         | Status-bar messages; Problems dialog for anything actionable |
| Elevated cards           | Flat 1-px-bordered panels                                    |
| Purple `#6200EE` primary | Teal accent (`#2DD4BF` / `#2FE3AE`)                          |

---

## 6. Interaction & Keyboard

rust-pad is keyboard- and **numpad**-first; preserve those bindings:

- **Standard:** `Ctrl/Cmd + N/O/S` new/open/save, `Z/Y` undo/redo,
  `X/C/V` cut/copy/paste, `A` select-all, `F` find. Show the shortcut in each
  menu item.
- **Mnemonics:** `Alt+letter` on Windows/Linux menus; on macOS use the global
  menu bar with `⌘` symbols.
- **Numpad workflow (app-specific, keep intact):** numpad `*`, `-`, `.`,
  `Enter`, and `Esc` drive the domain forms; they must not be shadowed by new
  tree/tab shortcuts. Document these in Help.
- **Focus order:** Menu bar → Activity bar → Workspace tree → Tabs → Editor →
  Status bar. Every focus stop shows the visible accent focus ring.
- **Tree keys:** ↑/↓ move, →/← expand/collapse, Enter opens, `F2` rename,
  `Delete` deletes (with confirm).
- **Drag & drop:** drop a file onto the window to open it; drag files within the
  tree to move; drag text to move a selection. Show a valid-drop cursor.
- **Unsaved close:** prompt "Save changes?" before closing a modified tab/window;
  remember window size/position and reopen on the active monitor.

---

## 7. Accessibility

- **Contrast:** body text ≥ 4.5:1 on its surface, UI/borders ≥ 3:1. The token
  sets above are tuned for this; light-mode accent is darkened specifically so
  accent-on-light passes. Verify any custom theme before shipping.
- **Focus visibility:** the accent focus ring is mandatory on every interactive
  control; do not rely on color alone — pair the ring with the selected bar/fill.
- **Screen readers:** label every icon button and tree row (name + kind, e.g.
  "notes, folder, expanded"; "Untitled 3, file, modified"). Announce dialog
  titles and Problems entries.
- **Scaling:** support egui `zoom_factor` (Ctrl+`+`/`-`) and OS scale at 125% /
  150%; the whole layout must reflow — the tree and editor keep min widths and
  the window minimum is **800×600**.
- **Hit targets:** ≥ 20×20 px clickable, ≥ 8-px spacing between interactive
  elements (tree rows and status segments already satisfy this).
- **Reduced motion:** honor the OS setting — drop caret blink to a steady caret
  and disable panel fades.

---

## 8. Theming & System Integration

- Ship all four themes (2 directions × light/dark) and a **"Follow system"**
  option that switches light/dark with the OS.
- On Windows, optionally tint the focus ring with the system accent; on macOS,
  sync with the system appearance; on Linux, respect the GTK/portal color-scheme
  preference.
- Fonts load via egui `FontDefinitions`: register IBM Plex Sans → `Proportional`
  and JetBrains Mono → `Monospace`, then set `FontId` sizes (13 editor / 13 UI /
  11 labels). Fall back to the platform UI font if a face is missing.
- Keep all colors in the theme file; code reads tokens, never hard-codes hex.

---

## 9. Motion

- Panel/menu/dialog show-hide: 120–200 ms cross-fade. Tree expand/collapse: a
  quick height/opacity ease (≤200 ms). No slides or page transitions.
- Instant press feedback (<100 ms) on buttons and tree rows.
- All non-essential motion is gated behind the reduced-motion check.

---

## 10. Testing & QA

**Design QA:** contrast pass on all four themes; focus ring visible on every
control; tree selection matches the active tab; line-ending marker toggle works;
status cursor segment renders per direction; both directions never mixed.

**Functional:** file open/save per OS; undo/redo, cut/copy/paste, find/replace;
numpad workflow intact; drag-drop open; unsaved-close prompt; theme + system
switch; high-DPI reflow at 125/150%.

**OS × feature matrix:**

| Feature / OS         | Windows                       | macOS              | Linux (GTK)           |
|----------------------|-------------------------------|--------------------|-----------------------|
| File open/save       | native dialog                 | native dialog      | GTK/portal dialog     |
| Menus                | in-window bar + Alt mnemonics | global menu bar, ⌘ | in-window bar         |
| Preferences location | Settings menu                 | App menu           | Settings menu         |
| Dark/light           | follows system                | follows system     | follows portal scheme |
| System accent tint   | optional focus ring           | —                  | —                     |
| High-DPI 125/150%    | reflows                       | reflows            | reflows               |
| Numpad workflow      | ✓                             | ✓                  | ✓                     |

---

## 11. Trade-offs & Notes

- **Native menus vs custom chrome:** rust-pad draws its own menu bar/tabs/tree in
  egui (immediate-mode has no native menu bar). Keep the *behavior* native —
  mnemonics, ordering, "Preferences" placement — even though pixels are custom.
- **Direction choice:** pick **one** direction to ship as the default; the other
  can live as an alternate theme pair. Aurora reads friendlier for general use;
  Graphite reads more "pro tool" and denser for power users.
- **Scope:** the activity bar, workspace tree, and breadcrumb are the only
  structural additions — everything else is restyling, so migration is mostly a
  `Visuals`/tokens swap plus three new panels.