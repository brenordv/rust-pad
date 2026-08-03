/// Dialogs for Find/Replace, Go To Line, etc.
use egui::{Context, Key, Ui};
use rust_pad_core::search::{SearchEngine, SearchOptions};

use crate::app::chrome::{self, DialogOptions};
use crate::app::resolved_theme::{ChromeTheme, Metrics};

/// State for the Find/Replace dialog.
#[derive(Debug)]
pub struct FindReplaceDialog {
    pub visible: bool,
    pub find_text: String,
    pub replace_text: String,
    pub options: SearchOptions,
    pub engine: SearchEngine,
    /// Whether to search the current tab, all open tabs, or a folder.
    pub scope: SearchScope,
    /// Folder to search when `scope` is [`SearchScope::Folder`]. Set by the
    /// sidebar/tab "Search in folder" actions.
    pub folder_path: Option<std::path::PathBuf>,
    /// Status message shown in the dialog.
    pub status: String,
    /// Snapshot of options from the previous frame, used to detect checkbox changes.
    prev_options_key: String,
    /// When true, the find text field requests focus on the next frame.
    focus_requested: bool,
    /// Session-only search history (most recent first, max 20 entries).
    search_history: Vec<String>,
    /// True when any of the dialog's interactive widgets had focus this frame.
    pub has_focus: bool,
    /// `visible` on the previous `show` call, for edge-triggered open/close logs.
    was_visible: bool,
    /// When true, the dialog window is raised to the front and given OS focus on
    /// the next `show` (set when opening, or re-opening an already-open dialog).
    raise_requested: bool,
    /// Last measured content height, reused as the window's initial size so a
    /// reopened window appears already sized to fit instead of opening large and
    /// then shrinking. Seeded with a close estimate for the first open.
    last_content_height: f32,
}

impl Default for FindReplaceDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl FindReplaceDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            find_text: String::new(),
            replace_text: String::new(),
            options: SearchOptions::default(),
            engine: SearchEngine::new(),
            scope: SearchScope::default(),
            folder_path: None,
            status: String::new(),
            prev_options_key: String::new(),
            focus_requested: false,
            search_history: Vec::new(),
            has_focus: false,
            was_visible: false,
            raise_requested: false,
            last_content_height: 250.0,
        }
    }

    pub fn open(&mut self) {
        self.open_with_text(None);
    }

    /// Opens the dialog, optionally pre-populating the find field.
    ///
    /// If `text` is `Some` and non-empty, it replaces the current find text
    /// and clears the status. If `None` or empty, the previous find text is
    /// preserved.
    pub fn open_with_text(&mut self, text: Option<String>) {
        self.visible = true;
        self.focus_requested = true;
        self.raise_requested = true;
        if let Some(t) = text {
            if !t.is_empty() {
                self.find_text = t;
                self.status.clear();
            }
        }
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    /// Opens the dialog primed to search `folder` (Folder scope). The find text
    /// is preserved so a repeat search reuses the last query.
    pub fn open_folder_search(&mut self, folder: std::path::PathBuf) {
        self.visible = true;
        self.focus_requested = true;
        self.raise_requested = true;
        self.scope = SearchScope::Folder;
        self.folder_path = Some(folder);
        self.status.clear();
    }

    /// Records the current find text into the search history.
    ///
    /// Deduplicates by moving an existing entry to the front. Caps at 20 entries.
    pub fn record_search(&mut self) {
        let query = self.find_text.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.search_history.retain(|h| h != &query);
        self.search_history.insert(0, query);
        self.search_history.truncate(20);
    }

    /// Builds a key string from the current search parameters for change detection.
    fn options_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{:?}:{:?}",
            self.find_text,
            self.options.case_sensitive,
            self.options.whole_word,
            self.options.use_regex,
            self.scope,
            self.folder_path,
        )
    }

    /// Renders the dialog body (find/replace inputs, search options, scope,
    /// action buttons, status footer) bracketed by the focus sentinels. Static
    /// so the same layout drives both the visible render and the hidden
    /// measurement pass ([`measure_content_height`](Self::measure_content_height)).
    #[allow(clippy::too_many_arguments)]
    fn render_body_contents(
        ui: &mut Ui,
        find_text: &mut String,
        replace_text: &mut String,
        action: &mut Option<FindReplaceAction>,
        focus_requested: &mut bool,
        has_focus: &mut bool,
        options: &mut SearchOptions,
        scope: &mut SearchScope,
        folder_path: Option<&std::path::Path>,
        status: &str,
        search_history: &[String],
        chrome_theme: &ChromeTheme,
        metrics: &Metrics,
        sentinel_top: egui::Id,
        sentinel_bottom: egui::Id,
        find_id: &mut Option<egui::Id>,
        last_button_id: &mut Option<egui::Id>,
    ) {
        ui.spacing_mut().item_spacing.y = 8.0;
        chrome::use_input_fill(ui, chrome_theme);
        focus_sentinel(ui, sentinel_top);
        Self::show_find_input(
            ui,
            find_text,
            action,
            focus_requested,
            search_history,
            has_focus,
            chrome_theme,
            metrics,
            find_id,
        );
        Self::show_replace_input(ui, replace_text, has_focus, chrome_theme, metrics);
        ui.add_space(4.0);
        Self::show_search_options(ui, options, scope, folder_path, action, chrome_theme);
        ui.add_space(4.0);
        *last_button_id = Self::show_action_buttons(ui, action, chrome_theme);
        if !status.is_empty() {
            // Match-count footer.
            ui.add_space(2.0);
            ui.separator();
            ui.label(
                egui::RichText::new(status)
                    .small()
                    .color(chrome_theme.text_muted),
            );
        }
        focus_sentinel(ui, sentinel_bottom);
    }

    /// Measures the natural content height (header band + body) with a hidden
    /// sizing pass, so the window can be created already sized to fit instead of
    /// opening large and then shrinking. Runs off-screen and non-interactive; the
    /// sizing pass paints nothing and steals no focus (`focus_requested` is forced
    /// false). The status footer is always measured (a placeholder is used when
    /// the status is momentarily empty) so the height is stable once a search
    /// populates it.
    fn measure_content_height(
        &self,
        ctx: &Context,
        chrome_theme: &ChromeTheme,
        metrics: &Metrics,
    ) -> f32 {
        const WIDTH: f32 = 440.0;
        let status = if self.status.is_empty() {
            "0 matches"
        } else {
            self.status.as_str()
        };
        let mut height = self.last_content_height;
        egui::Area::new(egui::Id::new("find_replace_measure"))
            .order(egui::Order::Background)
            .fixed_pos(egui::pos2(-100_000.0, -100_000.0))
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(WIDTH);
                height = ui
                    .scope_builder(egui::UiBuilder::new().sizing_pass(), |ui| {
                        ui.set_max_width(WIDTH);
                        let mut find_text = self.find_text.clone();
                        let mut replace_text = self.replace_text.clone();
                        let mut action = None;
                        let mut focus_requested = false;
                        let mut has_focus = false;
                        let mut options = self.options.clone();
                        let mut scope = self.scope;
                        let mut find_id = None;
                        let mut last_button_id = None;
                        let _ = chrome::dialog_header_and_body(
                            ui,
                            "Find and Replace",
                            true,
                            chrome_theme,
                            1.0,
                            0,
                            |ui| {
                                Self::render_body_contents(
                                    ui,
                                    &mut find_text,
                                    &mut replace_text,
                                    &mut action,
                                    &mut focus_requested,
                                    &mut has_focus,
                                    &mut options,
                                    &mut scope,
                                    self.folder_path.as_deref(),
                                    status,
                                    &self.search_history,
                                    chrome_theme,
                                    metrics,
                                    egui::Id::new("find_replace_measure_sentinel_top"),
                                    egui::Id::new("find_replace_measure_sentinel_bottom"),
                                    &mut find_id,
                                    &mut last_button_id,
                                );
                            },
                        );
                        ui.min_rect().height()
                    })
                    .inner;
            });
        height
    }

    /// Shows the Find/Replace dialog as a separate OS window and returns an
    /// action to perform, if any.
    ///
    /// On native builds the dialog is its own draggable window (an egui child
    /// viewport that can be moved outside the main window); on web or in
    /// headless tests `show_viewport_immediate` falls back to an in-app egui
    /// window. The `_editor_focused` parameter is kept for call-site
    /// compatibility but is unused: a separate OS window does not dim based on
    /// the editor's focus.
    pub fn show(
        &mut self,
        ctx: &Context,
        chrome_theme: &ChromeTheme,
        metrics: &Metrics,
        _editor_focused: bool,
    ) -> Option<FindReplaceAction> {
        // Edge-triggered open/close logging. The window class (native vs the
        // embedded fallback) is logged from inside the closure on the opening
        // frame; the close edge is logged here before the early return.
        let opening = self.visible && !self.was_visible;
        if self.was_visible && !self.visible {
            tracing::debug!("find & replace window closed");
        }
        self.was_visible = self.visible;

        // Reset focus tracking unconditionally so that closing the dialog clears
        // `has_focus`; `suppress_editor_input` reads this flag.
        self.has_focus = false;

        if !self.visible {
            return None;
        }

        // On the opening frame, measure the content in a hidden pass so the
        // window is created already sized to fit, instead of opening at a guess
        // and resizing. The per-frame resize below still catches in-session
        // changes (a match-count footer appearing, switching to Folder scope).
        if opening {
            self.last_content_height = self.measure_content_height(ctx, chrome_theme, metrics);
        }

        let mut action = None;
        let mut close = false;
        // Raise + focus the window this frame (opening, or Ctrl+F while open).
        let raise = std::mem::take(&mut self.raise_requested);
        let mut find_text = std::mem::take(&mut self.find_text);
        let mut replace_text = std::mem::take(&mut self.replace_text);
        let mut focus_requested = self.focus_requested;
        let mut has_focus = false;
        // Sentinels bracket the body so Tab/Shift+Tab focus cannot escape to the
        // editor in the embedded/test path; in a real separate window there is
        // nothing to escape to, so they are simply inert.
        let sentinel_top = egui::Id::new("find_replace_focus_sentinel_top");
        let sentinel_bottom = egui::Id::new("find_replace_focus_sentinel_bottom");

        // Measured this frame; persisted into `last_content_height` afterwards so
        // the next open starts at the right size.
        let mut content_height = 0.0_f32;

        let viewport_id = egui::ViewportId::from_hash_of("rust_pad_find_replace");
        // Center over the main window, but only on the opening frame so the
        // window stays wherever the user drags it afterwards.
        let initial_pos = opening
            .then(|| ctx.input(|i| i.viewport().outer_rect))
            .flatten()
            .map(|main| main.center() - egui::vec2(440.0, self.last_content_height) / 2.0);
        let mut builder = egui::ViewportBuilder::default()
            .with_title("Find and Replace")
            .with_inner_size([440.0, self.last_content_height])
            .with_decorations(false)
            .with_resizable(false);
        if let Some(pos) = initial_pos {
            builder = builder.with_position(pos);
        }

        ctx.show_viewport_immediate(viewport_id, builder, |ui, class| {
            if opening {
                let native = matches!(class, egui::ViewportClass::Immediate);
                tracing::debug!(native, "find & replace window opened");
            }
            if ui.ctx().input(|i| i.viewport().close_requested()) {
                close = true;
            }

            // Fill the whole borderless window with the themed chrome: a square
            // card (dialog_bg + 1px border) carrying the shared dialog header
            // band and the body. Square corners because the window is opaque;
            // rounding would need a transparent viewport, and the per-viewport
            // clear colour cannot be made transparent independently here.
            let window_frame = egui::Frame::new()
                .fill(chrome_theme.dialog_bg)
                .stroke(egui::Stroke::new(1.0, chrome_theme.border))
                .inner_margin(egui::Margin::ZERO);
            let mut find_id: Option<egui::Id> = None;
            let mut last_button_id: Option<egui::Id> = None;
            let header_rect = egui::CentralPanel::default()
                .frame(window_frame)
                .show_inside(ui, |ui| {
                    let dc = chrome::dialog_header_and_body(
                        ui,
                        "Find and Replace",
                        true,
                        chrome_theme,
                        1.0,
                        0,
                        |ui| {
                            Self::render_body_contents(
                                ui,
                                &mut find_text,
                                &mut replace_text,
                                &mut action,
                                &mut focus_requested,
                                &mut has_focus,
                                &mut self.options,
                                &mut self.scope,
                                self.folder_path.as_deref(),
                                &self.status,
                                &self.search_history,
                                chrome_theme,
                                metrics,
                                sentinel_top,
                                sentinel_bottom,
                                &mut find_id,
                                &mut last_button_id,
                            );
                        },
                    );
                    if dc.close_clicked {
                        close = true;
                    }
                    content_height = ui.min_rect().height();
                    dc.header_rect
                })
                .inner;

            // Native window management, real separate window only. In the
            // embedded/test fallback the dialog renders in the ROOT viewport, so
            // these commands would move/resize/focus the main window; skip them.
            if matches!(class, egui::ViewportClass::Immediate) {
                // Drag the header band (minus the close-button area) to move it.
                let mut drag_rect = header_rect;
                drag_rect.max.x = (drag_rect.max.x - 44.0).max(drag_rect.min.x);
                let drag = ui.interact(
                    drag_rect,
                    egui::Id::new("find_replace_titlebar"),
                    egui::Sense::click_and_drag(),
                );
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }

                // Shrink the window to fit its content so there is no empty strip
                // below the controls. Only resize on a real difference, to avoid
                // a resize loop.
                let target_h = content_height.ceil();
                let current_h = ui.ctx().input(|i| i.content_rect().height());
                if target_h > 0.0 && (target_h - current_h).abs() > 1.0 {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                            440.0, target_h,
                        )));
                }

                // Raise + focus the window when (re)opened (Ctrl+F while open).
                if raise {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }

            // F3 / Shift+F3 navigation while the dialog window is focused. Only
            // in a real separate window (Immediate): in the single-viewport
            // embedded/test path the global shortcut handler owns F3, so acting
            // here too would advance the match twice.
            if matches!(class, egui::ViewportClass::Immediate) {
                let hotkey = ui.input(|i| {
                    i.events.iter().find_map(|e| match e {
                        egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } => find_hotkey_action(*key, modifiers.shift),
                        _ => None,
                    })
                });
                if let Some(a) = hotkey {
                    action = Some(a);
                    if let Some(id) = find_id {
                        ui.memory_mut(|m| m.request_focus(id));
                    }
                }
            }

            // Trap focus inside the dialog. A sentinel only holds focus for the
            // instant the user tabs past an end; redirect to the opposite
            // control and repaint so the invisible sentinel is never the
            // visibly-focused widget.
            let focused = ui.memory(|m| m.focused());
            if focused == Some(sentinel_bottom) {
                if let Some(id) = find_id {
                    ui.memory_mut(|m| m.request_focus(id));
                    ui.ctx().request_repaint();
                }
            } else if focused == Some(sentinel_top) {
                if let Some(id) = last_button_id {
                    ui.memory_mut(|m| m.request_focus(id));
                    ui.ctx().request_repaint();
                }
            }

            // Gate `has_focus` on the dialog window's OS focus in the native
            // path so editor shortcuts in the main window are not suppressed
            // while the user works there (per-viewport focus does not clear on
            // its own when the other window is clicked).
            if matches!(class, egui::ViewportClass::Immediate) {
                let win_focused = ui.ctx().input(|i| i.viewport().focused).unwrap_or(false);
                has_focus &= win_focused;
            }

            if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });

        self.find_text = find_text;
        self.replace_text = replace_text;
        self.focus_requested = focus_requested;
        self.has_focus = has_focus;
        // Remember the fitted height so the next open starts already sized.
        if content_height > 1.0 {
            self.last_content_height = content_height.ceil();
        }

        if close {
            self.visible = false;
        }

        self.options.query = self.find_text.clone();
        self.detect_parameter_change(&mut action);
        action
    }

    /// Renders the find text input field with optional search history dropdown.
    ///
    /// Writes the find field's widget id into `find_id` so `show` can redirect
    /// trapped focus back to it.
    #[allow(clippy::too_many_arguments)]
    fn show_find_input(
        ui: &mut Ui,
        find_text: &mut String,
        action: &mut Option<FindReplaceAction>,
        focus_requested: &mut bool,
        history: &[String],
        has_focus: &mut bool,
        chrome_theme: &ChromeTheme,
        metrics: &Metrics,
        find_id: &mut Option<egui::Id>,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label("Find:      ");
            let find_response = ui.text_edit_singleline(find_text);
            *find_id = Some(find_response.id);
            *has_focus |= find_response.has_focus();
            chrome::input_border(ui.painter(), find_response.rect, chrome_theme, metrics);
            if find_response.has_focus() {
                chrome::focus_ring(ui.painter(), find_response.rect, chrome_theme, metrics);
            }
            if *focus_requested {
                *focus_requested = false;
                find_response.request_focus();
            }
            if find_response.changed() {
                *action = Some(FindReplaceAction::Search);
            }
            if find_response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                *action = Some(FindReplaceAction::FindNext);
                // Keep focus in the find field so repeated Enter cycles matches
                // instead of the field losing focus after the first jump.
                find_response.request_focus();
            }
            if !history.is_empty() {
                egui::ComboBox::from_id_salt("search_history")
                    .width(20.0)
                    .selected_text("")
                    .show_ui(ui, |ui| {
                        for entry in history {
                            if ui.selectable_label(false, entry).clicked() {
                                *find_text = entry.clone();
                                *action = Some(FindReplaceAction::Search);
                            }
                        }
                    });
            }
        });
    }

    /// Renders the replace text input field.
    fn show_replace_input(
        ui: &mut Ui,
        replace_text: &mut String,
        has_focus: &mut bool,
        chrome_theme: &ChromeTheme,
        metrics: &Metrics,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label("Replace:");
            let replace_response = ui.text_edit_singleline(replace_text);
            *has_focus |= replace_response.has_focus();
            chrome::input_border(ui.painter(), replace_response.rect, chrome_theme, metrics);
            if replace_response.has_focus() {
                chrome::focus_ring(ui.painter(), replace_response.rect, chrome_theme, metrics);
            }
        });
    }

    /// Renders search option checkboxes and scope radio buttons.
    fn show_search_options(
        ui: &mut Ui,
        options: &mut SearchOptions,
        scope: &mut SearchScope,
        folder_path: Option<&std::path::Path>,
        action: &mut Option<FindReplaceAction>,
        chrome_theme: &ChromeTheme,
    ) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let case = ui.checkbox(&mut options.case_sensitive, "Case sensitive");
            contain_arrows(ui, &case);
            let word = ui.checkbox(&mut options.whole_word, "Whole word");
            contain_arrows(ui, &word);
            let regex = ui.checkbox(&mut options.use_regex, "Regex");
            contain_arrows(ui, &regex);
        });
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.label("Scope:");
            let current = ui.radio(*scope == SearchScope::CurrentTab, "Current tab");
            if current.clicked() {
                *scope = SearchScope::CurrentTab;
            }
            contain_arrows(ui, &current);
            let all = ui.radio(*scope == SearchScope::AllTabs, "All tabs");
            if all.clicked() {
                *scope = SearchScope::AllTabs;
            }
            contain_arrows(ui, &all);
            let folder = ui.radio(*scope == SearchScope::Folder, "Folder");
            if folder.clicked() {
                *scope = SearchScope::Folder;
            }
            contain_arrows(ui, &folder);
        });
        if *scope == SearchScope::Folder {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                let choose = chrome::secondary_button(ui, chrome_theme, "Choose Folder...");
                if choose.clicked() {
                    *action = Some(FindReplaceAction::ChooseFolder);
                }
                contain_arrows(ui, &choose);
                let text = match folder_path {
                    Some(p) => format!("Folder: {}", p.display()),
                    None => "no folder chosen yet".to_string(),
                };
                ui.label(egui::RichText::new(text).small());
            });
        }
    }

    /// Renders the Find Next / Find Prev / Replace / Replace All buttons.
    /// Find Next is the primary action; the rest render as secondary.
    ///
    /// Returns the id of the last button so `show` can redirect trapped focus
    /// (Shift+Tab past the top) onto it.
    fn show_action_buttons(
        ui: &mut Ui,
        action: &mut Option<FindReplaceAction>,
        chrome_theme: &ChromeTheme,
    ) -> Option<egui::Id> {
        let mut last_id = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let find_next = chrome::primary_button(ui, chrome_theme, "Find Next");
            if find_next.clicked() {
                *action = Some(FindReplaceAction::FindNext);
            }
            contain_arrows(ui, &find_next);

            let find_prev = chrome::secondary_button(ui, chrome_theme, "Find Prev");
            if find_prev.clicked() {
                *action = Some(FindReplaceAction::FindPrev);
            }
            contain_arrows(ui, &find_prev);

            let find_all = chrome::secondary_button(ui, chrome_theme, "Find All")
                .on_hover_text("List every match (current tab or all tabs) in a results panel");
            if find_all.clicked() {
                *action = Some(FindReplaceAction::FindAll);
            }
            contain_arrows(ui, &find_all);

            let replace = chrome::secondary_button(ui, chrome_theme, "Replace");
            if replace.clicked() {
                *action = Some(FindReplaceAction::Replace);
            }
            contain_arrows(ui, &replace);

            let replace_all = chrome::secondary_button(ui, chrome_theme, "Replace All");
            if replace_all.clicked() {
                *action = Some(FindReplaceAction::ReplaceAll);
            }
            contain_arrows(ui, &replace_all);

            last_id = Some(replace_all.id);
        });
        last_id
    }

    /// Detects parameter changes and triggers a re-search if needed.
    fn detect_parameter_change(&mut self, action: &mut Option<FindReplaceAction>) {
        let current_key = self.options_key();
        if current_key != self.prev_options_key {
            self.prev_options_key = current_key;
            if action.is_none() {
                *action = Some(FindReplaceAction::Search);
            }
        }
    }
}

/// Maps a find-navigation hotkey to its action: F3 -> next, Shift+F3 ->
/// previous. Shared by the dialog window (native) and the global shortcut
/// handler (main window / embedded), so the mapping lives in one place.
pub fn find_hotkey_action(key: Key, shift: bool) -> Option<FindReplaceAction> {
    match (key, shift) {
        (Key::F3, false) => Some(FindReplaceAction::FindNext),
        (Key::F3, true) => Some(FindReplaceAction::FindPrev),
        _ => None,
    }
}

/// An invisible, focusable, zero-interaction widget used to trap Tab focus
/// inside the dialog. One is placed first and one last in the body; when the
/// user tabs past an end the sentinel gains focus for an instant, and `show`
/// redirects focus to the opposite control. Rendered via `interact`, so it
/// claims no layout space and paints nothing.
fn focus_sentinel(ui: &mut Ui, id: egui::Id) {
    let rect = egui::Rect::from_min_size(ui.min_rect().min, egui::vec2(1.0, 1.0));
    let _ = ui.interact(rect, id, egui::Sense::focusable_noninteractive());
}

/// Locks arrow keys to `response`'s widget while it holds focus, so egui does
/// not treat them as focus navigation and move focus out of the dialog. This is
/// only for the dialog's non-text controls (buttons, checkboxes, radios); text
/// fields are left alone because they own arrows for their cursor. Tab stays
/// free (the sentinel trap owns it) and Escape stays free (the global close
/// handler owns it).
fn contain_arrows(ui: &Ui, response: &egui::Response) {
    if response.has_focus() {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    tab: false,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: false,
                },
            );
        });
    }
}

/// Actions that the Find/Replace dialog can request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FindReplaceAction {
    Search,
    FindNext,
    FindPrev,
    Replace,
    ReplaceAll,
    /// Collect every match in the current scope into the results panel.
    FindAll,
    /// Open a folder picker so the user can choose the folder to search
    /// (Folder scope). Handled by the app, which owns the file dialog.
    ChooseFolder,
}

/// Which set of content the find operation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SearchScope {
    /// Search only in the active tab.
    #[default]
    CurrentTab,
    /// Search across all open tabs.
    AllTabs,
    /// Search files under a chosen folder (Find All only; runs off-thread).
    Folder,
}

/// Result of parsing a "Go to" input string.
///
/// Both `line` and `column` are 0-indexed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoToTarget {
    pub line: usize,
    pub column: usize,
}

/// Parses a go-to input string.
///
/// Accepted formats (all 1-indexed):
///   - `"42"`      → line 42, column 1
///   - `"42:10"`   → line 42, column 10
///   - `":10"`     → current line (None), column 10: rejected (line required)
///
/// Returns `None` if the input is empty, non-numeric, or the line is out of
/// range. The column is clamped to `1..=max_col` (never rejected).
pub fn parse_goto_input(input: &str, total_lines: usize) -> Option<GoToTarget> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let (line_str, col_str) = if let Some((l, c)) = input.split_once(':') {
        (l.trim(), c.trim())
    } else {
        (input, "")
    };

    let line_1based: usize = line_str.parse().ok()?;
    if line_1based < 1 || line_1based > total_lines {
        return None;
    }

    let col_1based: usize = if col_str.is_empty() {
        1
    } else {
        col_str.parse::<usize>().ok()?.max(1)
    };

    Some(GoToTarget {
        line: line_1based - 1,
        column: col_1based - 1,
    })
}

/// State for the Go To Line dialog.
#[derive(Debug)]
pub struct GoToLineDialog {
    pub visible: bool,
    pub line_text: String,
    /// When true, the text field requests focus on the next frame.
    focus_requested: bool,
}

impl Default for GoToLineDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl GoToLineDialog {
    pub fn new() -> Self {
        Self {
            visible: false,
            line_text: String::new(),
            focus_requested: false,
        }
    }

    pub fn open(&mut self) {
        self.visible = true;
        self.line_text.clear();
        self.focus_requested = true;
    }

    /// Attempts to navigate: parses input, stores result, and closes the dialog.
    fn try_navigate(
        line_text: &str,
        total_lines: usize,
        result: &mut Option<GoToTarget>,
        visible: &mut bool,
    ) {
        if let Some(target) = parse_goto_input(line_text, total_lines) {
            *result = Some(target);
            *visible = false;
        }
    }

    /// Shows the Go To Line dialog. Returns a target position if confirmed.
    pub fn show(
        &mut self,
        ctx: &Context,
        total_lines: usize,
        chrome_theme: &ChromeTheme,
        metrics: &Metrics,
    ) -> Option<GoToTarget> {
        if !self.visible {
            return None;
        }

        let mut result = None;
        let mut open = true;

        chrome::show_dialog(
            ctx,
            "Go to Line",
            &mut open,
            chrome_theme,
            metrics,
            DialogOptions {
                default_width: 280.0,
                ..Default::default()
            },
            |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                chrome::use_input_fill(ui, chrome_theme);
                ui.label(format!("Line[:Column] (1-{total_lines}):"));
                ui.add_space(4.0);

                let response = ui.text_edit_singleline(&mut self.line_text);
                chrome::input_border(ui.painter(), response.rect, chrome_theme, metrics);
                if response.has_focus() {
                    chrome::focus_ring(ui.painter(), response.rect, chrome_theme, metrics);
                }
                if self.focus_requested {
                    self.focus_requested = false;
                    response.request_focus();
                }

                if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                    Self::try_navigate(
                        &self.line_text,
                        total_lines,
                        &mut result,
                        &mut self.visible,
                    );
                }

                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if chrome::primary_button(ui, chrome_theme, "Go").clicked() {
                        Self::try_navigate(
                            &self.line_text,
                            total_lines,
                            &mut result,
                            &mut self.visible,
                        );
                    }
                    if chrome::secondary_button(ui, chrome_theme, "I'm not going anywhere")
                        .clicked()
                    {
                        self.visible = false;
                    }
                });
            },
        );

        if !open {
            self.visible = false;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Find & Replace focus flag ─────────────────────────────────────

    /// `has_focus` drives editor-shortcut suppression (`shortcuts.rs`), so a
    /// hidden dialog must clear it even though `show()` skips all rendering.
    /// Without the unconditional reset, closing the dialog with a text field
    /// focused would suppress editor clipboard shortcuts forever.
    #[test]
    fn hidden_dialog_clears_stale_focus_flag() {
        let mut dialog = FindReplaceDialog::new();
        dialog.visible = false;
        dialog.has_focus = true;

        let action = dialog.show(
            &Context::default(),
            &ChromeTheme::default(),
            &Metrics::default(),
            false,
        );

        assert!(action.is_none());
        assert!(
            !dialog.has_focus,
            "a hidden dialog must release editor-shortcut suppression"
        );
    }

    // ── find_hotkey_action ────────────────────────────────────────────

    #[test]
    fn find_hotkey_action_maps_f3_and_shift_f3() {
        assert_eq!(
            find_hotkey_action(Key::F3, false),
            Some(FindReplaceAction::FindNext)
        );
        assert_eq!(
            find_hotkey_action(Key::F3, true),
            Some(FindReplaceAction::FindPrev)
        );
        assert_eq!(find_hotkey_action(Key::Enter, false), None);
        assert_eq!(find_hotkey_action(Key::G, false), None);
    }

    /// The hidden measurement pass returns a plausible content height (taller
    /// than the header band, within a sane bound) without panicking, so the
    /// window can be opened already sized to fit.
    #[test]
    fn measure_content_height_is_plausible() {
        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let dialog = FindReplaceDialog::new();
        let chrome = ChromeTheme::default();
        let metrics = Metrics::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        };
        let mut measured = 0.0_f32;
        let _ = ctx.run_ui(raw, |ui| {
            measured = dialog.measure_content_height(ui.ctx(), &chrome, &metrics);
        });
        // Taller than the 36px header band, and not implausibly large.
        assert!(measured > 100.0, "measured height {measured} too small");
        assert!(
            measured < 600.0,
            "measured height {measured} implausibly large"
        );
    }

    // ── parse_goto_input ──────────────────────────────────────────────

    #[test]
    fn test_parse_line_only() {
        let target = parse_goto_input("42", 100).unwrap();
        assert_eq!(target.line, 41); // 0-indexed
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_line_and_column() {
        let target = parse_goto_input("10:5", 100).unwrap();
        assert_eq!(target.line, 9);
        assert_eq!(target.column, 4);
    }

    #[test]
    fn test_parse_with_whitespace() {
        let target = parse_goto_input("  10 : 5  ", 100).unwrap();
        assert_eq!(target.line, 9);
        assert_eq!(target.column, 4);
    }

    #[test]
    fn test_parse_first_line() {
        let target = parse_goto_input("1", 100).unwrap();
        assert_eq!(target.line, 0);
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_last_line() {
        let target = parse_goto_input("100", 100).unwrap();
        assert_eq!(target.line, 99);
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_line_zero_rejected() {
        assert!(parse_goto_input("0", 100).is_none());
    }

    #[test]
    fn test_parse_line_exceeds_total() {
        assert!(parse_goto_input("101", 100).is_none());
    }

    #[test]
    fn test_parse_empty_input() {
        assert!(parse_goto_input("", 100).is_none());
    }

    #[test]
    fn test_parse_whitespace_only() {
        assert!(parse_goto_input("   ", 100).is_none());
    }

    #[test]
    fn test_parse_non_numeric() {
        assert!(parse_goto_input("abc", 100).is_none());
    }

    #[test]
    fn test_parse_column_zero_clamped_to_one() {
        // Column 0 in input is clamped to 1 (minimum), so result column is 0 (0-indexed)
        let target = parse_goto_input("5:0", 100).unwrap();
        assert_eq!(target.line, 4);
        assert_eq!(target.column, 0); // max(1,0) = 1, then 1-1 = 0
    }

    #[test]
    fn test_parse_large_column() {
        // Large column is accepted (will be clamped by cursor move_to)
        let target = parse_goto_input("1:999", 100).unwrap();
        assert_eq!(target.line, 0);
        assert_eq!(target.column, 998);
    }

    #[test]
    fn test_parse_negative_rejected() {
        // Negative numbers can't parse as usize
        assert!(parse_goto_input("-5", 100).is_none());
    }

    #[test]
    fn test_parse_colon_without_column() {
        // "10:" → empty column string → defaults to column 1
        let target = parse_goto_input("10:", 100).unwrap();
        assert_eq!(target.line, 9);
        assert_eq!(target.column, 0);
    }

    #[test]
    fn test_parse_non_numeric_column() {
        assert!(parse_goto_input("10:abc", 100).is_none());
    }

    // ── GoToLineDialog state ──────────────────────────────────────────

    #[test]
    fn test_dialog_open_clears_text() {
        let mut dialog = GoToLineDialog::new();
        dialog.line_text = "42".to_string();
        dialog.open();
        assert!(dialog.visible);
        assert!(dialog.line_text.is_empty());
    }

    #[test]
    fn test_dialog_default_not_visible() {
        let dialog = GoToLineDialog::new();
        assert!(!dialog.visible);
        assert!(dialog.line_text.is_empty());
    }

    // ── FindReplaceDialog: focus ─────────────────────────────────────

    #[test]
    fn test_find_dialog_new_no_focus() {
        let dialog = FindReplaceDialog::new();
        assert!(!dialog.focus_requested);
    }

    #[test]
    fn test_find_dialog_open_sets_focus_requested() {
        let mut dialog = FindReplaceDialog::new();
        dialog.open();
        assert!(dialog.visible);
        assert!(dialog.focus_requested);
    }

    // ── FindReplaceDialog: open_with_text ────────────────────────────

    #[test]
    fn test_open_with_text_populates_find_field() {
        let mut dialog = FindReplaceDialog::new();
        dialog.open_with_text(Some("hello".to_string()));
        assert!(dialog.visible);
        assert_eq!(dialog.find_text, "hello");
    }

    #[test]
    fn test_open_with_text_none_preserves_previous() {
        let mut dialog = FindReplaceDialog::new();
        dialog.find_text = "previous".to_string();
        dialog.open_with_text(None);
        assert_eq!(dialog.find_text, "previous");
    }

    #[test]
    fn test_open_with_text_empty_preserves_previous() {
        let mut dialog = FindReplaceDialog::new();
        dialog.find_text = "previous".to_string();
        dialog.open_with_text(Some(String::new()));
        assert_eq!(dialog.find_text, "previous");
    }

    #[test]
    fn test_open_with_text_sets_focus() {
        let mut dialog = FindReplaceDialog::new();
        dialog.open_with_text(Some("test".to_string()));
        assert!(dialog.focus_requested);
    }

    // ── FindReplaceDialog: search history ────────────────────────────

    #[test]
    fn test_record_search_adds_to_history() {
        let mut dialog = FindReplaceDialog::new();
        dialog.find_text = "hello".to_string();
        dialog.record_search();
        assert_eq!(dialog.search_history, vec!["hello"]);
    }

    #[test]
    fn test_record_search_deduplicates() {
        let mut dialog = FindReplaceDialog::new();
        dialog.find_text = "hello".to_string();
        dialog.record_search();
        dialog.record_search();
        assert_eq!(dialog.search_history, vec!["hello"]);
    }

    #[test]
    fn test_record_search_moves_to_front() {
        let mut dialog = FindReplaceDialog::new();
        dialog.find_text = "alpha".to_string();
        dialog.record_search();
        dialog.find_text = "beta".to_string();
        dialog.record_search();
        dialog.find_text = "alpha".to_string();
        dialog.record_search();
        assert_eq!(dialog.search_history, vec!["alpha", "beta"]);
    }

    #[test]
    fn test_record_search_caps_at_20() {
        let mut dialog = FindReplaceDialog::new();
        for i in 0..25 {
            dialog.find_text = format!("query_{i}");
            dialog.record_search();
        }
        assert_eq!(dialog.search_history.len(), 20);
        assert_eq!(dialog.search_history[0], "query_24");
    }

    #[test]
    fn test_record_search_ignores_empty() {
        let mut dialog = FindReplaceDialog::new();
        dialog.find_text = "   ".to_string();
        dialog.record_search();
        assert!(dialog.search_history.is_empty());
    }

    #[test]
    fn test_record_search_trims_whitespace() {
        let mut dialog = FindReplaceDialog::new();
        dialog.find_text = "  foo  ".to_string();
        dialog.record_search();
        assert_eq!(dialog.search_history, vec!["foo"]);
    }

    // ── FindReplaceDialog: has_focus ─────────────────────────────────

    #[test]
    fn test_find_replace_has_focus_default_false() {
        let dialog = FindReplaceDialog::new();
        assert!(!dialog.has_focus);
    }

    // ── SearchScope::Folder priming ──────────────────────────────────

    #[test]
    fn open_folder_search_sets_scope_and_folder() {
        let mut dialog = FindReplaceDialog::new();
        dialog.open_folder_search(std::path::PathBuf::from("some_dir"));
        assert!(dialog.visible);
        assert!(dialog.focus_requested);
        assert_eq!(dialog.scope, SearchScope::Folder);
        assert_eq!(
            dialog.folder_path.as_deref(),
            Some(std::path::Path::new("some_dir"))
        );
    }

    #[test]
    fn options_key_reflects_folder_change() {
        let mut dialog = FindReplaceDialog::new();
        dialog.open_folder_search(std::path::PathBuf::from("dir_a"));
        let key_a = dialog.options_key();
        dialog.folder_path = Some(std::path::PathBuf::from("dir_b"));
        assert_ne!(
            key_a,
            dialog.options_key(),
            "switching folders re-keys the search"
        );
    }

    // ── FindReplaceDialog: keyboard focus trap ───────────────────────

    /// Maps the named semibold UI family onto the default proportional list so
    /// the dialog header/primary button lay out in a bare test context.
    fn register_semibold_alias(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let proportional = fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        fonts.families.insert(
            egui::FontFamily::Name(crate::app::FONT_FAMILY_SEMIBOLD.into()),
            proportional,
        );
        ctx.set_fonts(fonts);
    }

    const BG_EDITOR: &str = "bg_editor_for_focus_test";

    /// Runs one frame: a focusable background "editor" (as the real app renders
    /// before its dialogs) plus the visible dialog, feeding `events`.
    fn drive_dialog(ctx: &egui::Context, dialog: &mut FindReplaceDialog, events: Vec<egui::Event>) {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let chrome = ChromeTheme::default();
        let metrics = Metrics::default();
        let _ = ctx.run_ui(raw, |ui| {
            // The editor is focusable (Sense::click_and_drag) in the app and is
            // registered before the dialog, so it is a genuine spatial-nav / Tab
            // escape target.
            let _ = ui.interact(
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 120.0)),
                egui::Id::new(BG_EDITOR),
                egui::Sense::click_and_drag(),
            );
            dialog.show(ui.ctx(), &chrome, &metrics, false);
        });
    }

    fn key_event(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        }
    }

    /// Opens the dialog and lets the initial focus request settle onto the find
    /// field (two frames: request lands next frame).
    fn open_and_settle(ctx: &egui::Context, dialog: &mut FindReplaceDialog) {
        dialog.open();
        drive_dialog(ctx, dialog, Vec::new());
        drive_dialog(ctx, dialog, Vec::new());
    }

    fn focused(ctx: &egui::Context) -> Option<egui::Id> {
        ctx.memory(|m| m.focused())
    }

    /// Tab and Shift+Tab cycle within the dialog and never land on the
    /// background editor, and focus never rests on an invisible sentinel.
    #[test]
    fn tab_focus_never_escapes_the_dialog() {
        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let mut dialog = FindReplaceDialog::new();
        open_and_settle(&ctx, &mut dialog);

        let bg = egui::Id::new(BG_EDITOR);
        let sent_top = egui::Id::new("find_replace_focus_sentinel_top");
        let sent_bot = egui::Id::new("find_replace_focus_sentinel_bottom");

        for (shift, label) in [(false, "Tab"), (true, "Shift+Tab")] {
            let mods = if shift {
                egui::Modifiers::SHIFT
            } else {
                egui::Modifiers::NONE
            };
            let mut seen = std::collections::HashSet::new();
            for _ in 0..14 {
                drive_dialog(&ctx, &mut dialog, vec![key_event(egui::Key::Tab, mods)]);
                drive_dialog(&ctx, &mut dialog, Vec::new()); // settle the redirect
                let f = focused(&ctx);
                assert_ne!(f, Some(bg), "{label} escaped to the background editor");
                assert_ne!(f, Some(sent_top), "{label} left focus on the top sentinel");
                assert_ne!(
                    f,
                    Some(sent_bot),
                    "{label} left focus on the bottom sentinel"
                );
                if let Some(id) = f {
                    seen.insert(id);
                }
            }
            assert!(
                seen.len() > 1,
                "{label} did not move focus through multiple dialog widgets"
            );
        }
    }

    /// Arrow keys never move focus out of the dialog (spatial nav is contained).
    #[test]
    fn arrow_keys_never_escape_the_dialog() {
        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let mut dialog = FindReplaceDialog::new();
        open_and_settle(&ctx, &mut dialog);

        let bg = egui::Id::new(BG_EDITOR);

        // Tab into the controls and hold focus a frame so the arrow lock engages.
        for _ in 0..9 {
            drive_dialog(
                &ctx,
                &mut dialog,
                vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            drive_dialog(&ctx, &mut dialog, Vec::new());
        }
        for key in [
            egui::Key::ArrowDown,
            egui::Key::ArrowUp,
            egui::Key::ArrowRight,
            egui::Key::ArrowLeft,
        ] {
            drive_dialog(
                &ctx,
                &mut dialog,
                vec![key_event(key, egui::Modifiers::NONE)],
            );
            drive_dialog(&ctx, &mut dialog, Vec::new());
            assert_ne!(
                focused(&ctx),
                Some(bg),
                "arrow {key:?} escaped the dialog to the background editor"
            );
        }
    }

    /// The Folder scope renders its Choose Folder button and folder label, and
    /// tabbing through that extra control still never escapes the dialog.
    #[test]
    fn folder_scope_renders_and_stays_trapped() {
        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let mut dialog = FindReplaceDialog::new();
        dialog.open_folder_search(std::path::PathBuf::from("some_dir"));
        drive_dialog(&ctx, &mut dialog, Vec::new());
        drive_dialog(&ctx, &mut dialog, Vec::new());

        let bg = egui::Id::new(BG_EDITOR);
        for _ in 0..14 {
            drive_dialog(
                &ctx,
                &mut dialog,
                vec![key_event(egui::Key::Tab, egui::Modifiers::NONE)],
            );
            drive_dialog(&ctx, &mut dialog, Vec::new());
            assert_ne!(
                focused(&ctx),
                Some(bg),
                "Tab escaped the folder-scope dialog"
            );
        }
    }
}
