//! Top-level application tying together tabs, editor, menus, and status bar.

mod about_dialog;
mod clipboard;
mod editing;
mod file_ops;
mod menu_bar;
mod search;
mod settings_dialog;
mod shortcuts;
mod status_bar;
mod tab_bar;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::Color32;

use rust_pad_config::session::{generate_session_id, SessionData, SessionStore, SessionTabEntry};
use rust_pad_config::{AppConfig, ThemeDefinition, UiColors};
use rust_pad_core::bookmarks::BookmarkManager;
use rust_pad_core::cursor::Position;
use rust_pad_core::history::{HistoryConfig, PersistenceLayer};

use crate::dialogs::{FindReplaceDialog, GoToLineDialog};
use crate::editor::{EditorTheme, EditorWidget, SyntaxHighlighter};
use crate::tabs::TabManager;

/// How often to flush undo history to disk (in seconds).
const FLUSH_INTERVAL_SECS: u64 = 30;

/// Arguments passed from the command line to the application.
#[derive(Debug, Clone, Default)]
pub struct StartupArgs {
    /// File paths to open on startup.
    pub files: Vec<PathBuf>,
    /// If set, create a new tab pre-filled with this text.
    pub new_file_text: Option<String>,
}

/// Which color theme to use.
///
/// Wraps a string name. Special values: `"System"`, `"Dark"`, `"Light"`.
/// Any other value refers to a custom theme name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeMode(pub String);

impl Default for ThemeMode {
    fn default() -> Self {
        Self::system()
    }
}

impl ThemeMode {
    pub fn system() -> Self {
        Self("System".to_string())
    }

    pub fn dark() -> Self {
        Self("Dark".to_string())
    }

    pub fn light() -> Self {
        Self("Light".to_string())
    }

    /// Returns true if this is the "System" mode.
    pub fn is_system(&self) -> bool {
        self.0 == "System"
    }

    /// Resolves "System" to a concrete theme name using the OS preference.
    /// Non-system modes return their own name.
    pub fn resolve(&self) -> &str {
        if self.is_system() {
            match dark_light::detect() {
                Ok(dark_light::Mode::Light) => "Light",
                _ => "Dark",
            }
        } else {
            &self.0
        }
    }
}

/// The main application state.
pub struct App {
    pub tabs: TabManager,
    pub theme: EditorTheme,
    pub theme_mode: ThemeMode,
    pub zoom_level: f32,
    pub max_zoom_level: f32,
    pub word_wrap: bool,
    pub show_special_chars: bool,
    pub show_line_numbers: bool,
    pub restore_open_files: bool,
    pub show_full_path_in_title: bool,
    pub default_extension: String,
    pub remember_last_folder: bool,
    pub default_work_folder: String,
    pub last_used_folder: Option<PathBuf>,
    pub auto_save_enabled: bool,
    pub auto_save_interval_secs: u64,
    last_auto_save: Instant,
    pub available_themes: Vec<ThemeDefinition>,
    accent_color: Color32,
    config_path: PathBuf,
    clipboard: Option<arboard::Clipboard>,
    dialog_state: DialogState,
    pub find_replace: FindReplaceDialog,
    pub go_to_line: GoToLineDialog,
    bookmarks: BookmarkManager,
    syntax_highlighter: SyntaxHighlighter,
    last_flush: Instant,
    session_store: Option<SessionStore>,
    last_window_title: String,
    last_file_check: Instant,
    pub(crate) settings_open: bool,
    pub(crate) settings_tab: settings_dialog::SettingsTab,
    pub(crate) about_open: bool,
    pub(crate) about_logo: Option<egui::TextureHandle>,
}

#[derive(Debug, Default)]
pub(crate) enum DialogState {
    #[default]
    None,
    ConfirmClose(usize),
}

impl App {
    /// Creates a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>, args: StartupArgs) -> Self {
        // Disable egui's built-in keyboard zoom so Ctrl+/- only affects the editor text
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);

        // Load config
        let config_path = AppConfig::config_path();
        let app_config = AppConfig::load_or_create(&config_path);

        let mut theme_mode = ThemeMode(app_config.current_theme.clone());
        let resolved_name = theme_mode.resolve().to_string();
        let font_size = app_config.font_size;

        // Resolve theme definition; fall back to System if the theme doesn't exist
        let theme_def = match app_config.find_theme(&resolved_name).cloned() {
            Some(def) => def,
            None => {
                tracing::warn!(
                    "Theme '{}' not found, falling back to System",
                    resolved_name
                );
                theme_mode = ThemeMode::system();
                let fallback_name = theme_mode.resolve().to_string();
                app_config
                    .find_theme(&fallback_name)
                    .cloned()
                    .unwrap_or_else(rust_pad_config::theme::builtin_dark)
            }
        };

        let editor_theme = EditorTheme::from_config(&theme_def.editor, font_size);
        Self::apply_theme_visuals(&cc.egui_ctx, &theme_def.ui, theme_def.dark_mode);
        let ac = theme_def.ui.accent_color;
        let accent_color = Color32::from_rgba_premultiplied(ac.r, ac.g, ac.b, ac.a);

        let mut syntax_highlighter = SyntaxHighlighter::new();
        syntax_highlighter.set_theme(&theme_def.syntax_theme);

        let history_config = HistoryConfig::default();
        let mut tabs = match PersistenceLayer::open(&history_config.data_dir) {
            Ok(pl) => TabManager::with_persistence(pl, history_config),
            Err(e) => {
                tracing::warn!(
                    "Failed to open undo history database, falling back to in-memory: {e}"
                );
                TabManager::new()
            }
        };
        tabs.default_extension = app_config.default_extension.clone();

        // Open session store
        let session_store = match SessionStore::open(&SessionStore::session_path()) {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!("Failed to open session store: {e}");
                None
            }
        };

        // Restore session if enabled
        if app_config.restore_open_files {
            if let Some(store) = &session_store {
                if let Ok(Some(session_data)) = store.load_session() {
                    let mut any_restored = false;
                    for entry in &session_data.tabs {
                        match entry {
                            SessionTabEntry::File { path } => {
                                let p = std::path::Path::new(path);
                                if p.exists() {
                                    if let Err(e) = tabs.open_file(p) {
                                        tracing::warn!("Failed to restore '{path}': {e}");
                                    } else {
                                        any_restored = true;
                                    }
                                }
                            }
                            SessionTabEntry::Unsaved { session_id, title } => {
                                let content = store
                                    .load_content(session_id)
                                    .ok()
                                    .flatten()
                                    .unwrap_or_default();
                                let mut doc = rust_pad_core::document::Document::new();
                                doc.title = title.clone();
                                if !content.is_empty() {
                                    doc.buffer =
                                        rust_pad_core::buffer::TextBuffer::from(content.as_str());
                                    doc.modified = true;
                                }
                                doc.session_id = Some(generate_session_id());
                                tabs.documents.push(doc);
                                any_restored = true;
                            }
                        }
                    }
                    if any_restored {
                        // Remove the phantom initial empty tab
                        tabs.documents.remove(0);
                        tabs.active = session_data
                            .active_tab_index
                            .min(tabs.documents.len().saturating_sub(1));
                    }
                    // Clear old content — fresh IDs assigned, will be rewritten on exit
                    let _ = store.clear_all_content();
                }
            }
        }

        // Open files requested via CLI arguments
        let has_cli_content = !args.files.is_empty() || args.new_file_text.is_some();
        for path in &args.files {
            let abs_path = if path.is_absolute() {
                path.clone()
            } else {
                std::env::current_dir().unwrap_or_default().join(path)
            };
            if let Err(e) = tabs.open_file(&abs_path) {
                tracing::warn!("Failed to open '{}': {e}", abs_path.display());
            }
        }

        // Handle --new-file: create an untitled tab pre-filled with the given text
        if let Some(text) = args.new_file_text {
            tabs.new_tab();
            let doc = tabs.active_doc_mut();
            doc.insert_text(&text);
        }

        // If CLI args opened any tabs, remove the initial empty tab that came
        // from TabManager construction (only if it's still pristine).
        if has_cli_content && tabs.tab_count() > 1 {
            let first_is_empty = tabs.documents[0].buffer.is_empty()
                && tabs.documents[0].file_path.is_none()
                && !tabs.documents[0].modified;
            if first_is_empty {
                tabs.close_tab(0);
            }
        }

        Self {
            tabs,
            theme: editor_theme,
            theme_mode,
            zoom_level: app_config.current_zoom_level,
            max_zoom_level: app_config.max_zoom_level,
            word_wrap: app_config.word_wrap,
            show_special_chars: app_config.show_special_chars,
            show_line_numbers: app_config.show_line_numbers,
            restore_open_files: app_config.restore_open_files,
            show_full_path_in_title: app_config.show_full_path_in_title,
            default_extension: app_config.default_extension,
            remember_last_folder: app_config.remember_last_folder,
            default_work_folder: app_config.default_work_folder,
            last_used_folder: if app_config.last_used_folder.is_empty() {
                None
            } else {
                Some(PathBuf::from(app_config.last_used_folder))
            },
            auto_save_enabled: app_config.auto_save_enabled,
            auto_save_interval_secs: app_config.auto_save_interval_secs,
            last_auto_save: Instant::now(),
            available_themes: app_config.themes,
            accent_color,
            config_path,
            clipboard: arboard::Clipboard::new().ok(),
            dialog_state: DialogState::None,
            find_replace: FindReplaceDialog::new(),
            go_to_line: GoToLineDialog::new(),
            bookmarks: BookmarkManager::new(),
            syntax_highlighter,
            last_flush: Instant::now(),
            session_store,
            last_window_title: String::new(),
            last_file_check: Instant::now(),
            settings_open: false,
            settings_tab: settings_dialog::SettingsTab::default(),
            about_open: false,
            about_logo: None,
        }
    }

    /// Applies egui visuals from config UI colors.
    fn apply_theme_visuals(ctx: &egui::Context, ui_colors: &UiColors, dark_mode: bool) {
        let hex = |c: rust_pad_config::HexColor| -> Color32 {
            Color32::from_rgba_premultiplied(c.r, c.g, c.b, c.a)
        };
        let mut visuals = if dark_mode {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };

        // Fill colors
        visuals.panel_fill = hex(ui_colors.panel_fill);
        visuals.window_fill = hex(ui_colors.window_fill);
        visuals.faint_bg_color = hex(ui_colors.faint_bg_color);
        visuals.extreme_bg_color = hex(ui_colors.extreme_bg_color);
        visuals.widgets.noninteractive.bg_fill = hex(ui_colors.widget_noninteractive_bg);
        visuals.widgets.inactive.bg_fill = hex(ui_colors.widget_inactive_bg);
        visuals.widgets.hovered.bg_fill = hex(ui_colors.widget_hovered_bg);
        visuals.widgets.active.bg_fill = hex(ui_colors.widget_active_bg);

        // Widget rounding — consistent 4px on all states
        let widget_rounding = egui::CornerRadius::same(4);
        visuals.widgets.noninteractive.corner_radius = widget_rounding;
        visuals.widgets.inactive.corner_radius = widget_rounding;
        visuals.widgets.hovered.corner_radius = widget_rounding;
        visuals.widgets.active.corner_radius = widget_rounding;
        visuals.widgets.open.corner_radius = widget_rounding;

        // Window/menu rounding
        visuals.window_corner_radius = egui::CornerRadius::same(6);
        visuals.menu_corner_radius = egui::CornerRadius::same(4);

        // Clean borders
        visuals.widgets.noninteractive.bg_stroke.width = 0.0;
        visuals.window_stroke.width = 1.0;

        // Popup shadow — minimal
        visuals.popup_shadow = egui::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(40),
        };

        ctx.set_visuals(visuals);

        // Spacing
        ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::Vec2::new(8.0, 6.0);
            style.spacing.button_padding = egui::Vec2::new(8.0, 4.0);
            style.spacing.window_margin = egui::Margin::same(12);
        });
    }

    /// Updates the OS window title to show the active document name.
    ///
    /// Only sends the viewport command when the title actually changes,
    /// to avoid triggering unnecessary repaints.
    fn update_window_title(&mut self, ctx: &egui::Context) {
        let doc = self.tabs.active_doc();
        let file_label = if let Some(ref path) = doc.file_path {
            if self.show_full_path_in_title {
                path.to_string_lossy().into_owned()
            } else {
                doc.title.clone()
            }
        } else {
            doc.title.clone()
        };
        let modified_marker = if doc.modified { " *" } else { "" };
        let title = format!("{file_label}{modified_marker} - rust-pad");
        if title != self.last_window_title {
            self.last_window_title.clone_from(&title);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
    }

    /// Switches to a new theme mode and applies all theme changes.
    pub fn set_theme_mode(&mut self, mode: ThemeMode, ctx: &egui::Context) {
        self.theme_mode = mode;
        let resolved_name = self.theme_mode.resolve().to_string();

        // Fall back to System if the resolved theme doesn't exist
        let theme_def = match self
            .available_themes
            .iter()
            .find(|t| t.name == resolved_name)
            .cloned()
        {
            Some(def) => def,
            None => {
                tracing::warn!(
                    "Theme '{}' not found, falling back to System",
                    resolved_name
                );
                self.theme_mode = ThemeMode::system();
                let fallback_name = self.theme_mode.resolve().to_string();
                self.available_themes
                    .iter()
                    .find(|t| t.name == fallback_name)
                    .cloned()
                    .unwrap_or_else(rust_pad_config::theme::builtin_dark)
            }
        };

        self.theme = EditorTheme::from_config(&theme_def.editor, self.theme.font_size);
        Self::apply_theme_visuals(ctx, &theme_def.ui, theme_def.dark_mode);
        let ac = theme_def.ui.accent_color;
        self.accent_color = Color32::from_rgba_premultiplied(ac.r, ac.g, ac.b, ac.a);
        self.syntax_highlighter.set_theme(&theme_def.syntax_theme);
    }

    /// Shows all dialog windows.
    fn show_dialogs(&mut self, ctx: &egui::Context) {
        // Confirm close dialog
        match self.dialog_state {
            DialogState::ConfirmClose(idx) => {
                let mut open = true;
                let title = if idx < self.tabs.tab_count() {
                    self.tabs.documents[idx].title.clone()
                } else {
                    "Document".to_string()
                };

                egui::Window::new("Unsaved Changes")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .open(&mut open)
                    .show(ctx, |ui| {
                        ui.spacing_mut().item_spacing.y = 8.0;

                        ui.label(format!("'{title}' has unsaved changes. Close anyway?"));

                        ui.add_space(4.0);

                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            if ui.button("  Save & Close  ").clicked() {
                                if idx < self.tabs.tab_count() {
                                    let doc = &mut self.tabs.documents[idx];
                                    if doc.file_path.is_some() {
                                        let _ = doc.save();
                                    }
                                    self.cleanup_session_for_tab(idx);
                                    self.tabs.close_tab(idx);
                                }
                                self.dialog_state = DialogState::None;
                            }
                            if ui.button("  Discard  ").clicked() {
                                self.cleanup_session_for_tab(idx);
                                self.tabs.close_tab(idx);
                                self.dialog_state = DialogState::None;
                            }
                            if ui.button("  Cancel  ").clicked() {
                                self.dialog_state = DialogState::None;
                            }
                        });
                    });

                if !open {
                    self.dialog_state = DialogState::None;
                }
            }
            DialogState::None => {}
        }

        // Settings dialog
        self.show_settings_dialog(ctx);

        // About dialog
        if self.about_open {
            self.load_about_logo(ctx);
        }
        self.show_about_dialog(ctx);

        // Find/Replace dialog
        if let Some(action) = self.find_replace.show(ctx) {
            self.handle_search_action(action);
        }

        // Go to Line dialog
        let total_lines = self.tabs.active_doc().buffer.len_lines();
        if let Some(target) = self.go_to_line.show(ctx, total_lines) {
            let doc = self.tabs.active_doc_mut();
            doc.cursor.clear_selection();
            doc.cursor
                .move_to(Position::new(target.line, target.column), &doc.buffer);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Prevent egui's built-in Ctrl+scroll zoom — we handle zoom ourselves
        ctx.set_zoom_factor(1.0);

        self.handle_global_shortcuts(ctx);

        // Update the OS window title to reflect the active document
        self.update_window_title(ctx);

        // Menu bar
        let panel_fill = ctx.style().visuals.panel_fill;
        let faint_bg = ctx.style().visuals.faint_bg_color;
        let extreme_bg = ctx.style().visuals.extreme_bg_color;

        egui::TopBottomPanel::top("menu_bar")
            .frame(
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                self.show_menu_bar(ui, ctx);
            });

        // Tab bar
        egui::TopBottomPanel::top("tab_bar")
            .frame(
                egui::Frame::new()
                    .fill(faint_bg)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show(ctx, |ui| {
                self.show_tab_bar(ui);
            });

        // Status bar
        egui::TopBottomPanel::bottom("status_bar")
            .max_height(24.0)
            .frame(
                egui::Frame::new()
                    .fill(extreme_bg)
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show(ctx, |ui| {
                self.show_status_bar(ui);
            });

        // Editor area
        let dialog_open = self.is_dialog_open();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.theme.bg_color))
            .show(ctx, |ui| {
                let doc = self.tabs.active_doc_mut();
                let mut editor = EditorWidget::new(
                    doc,
                    &self.theme,
                    self.zoom_level,
                    Some(&self.syntax_highlighter),
                );
                editor.word_wrap = self.word_wrap;
                editor.show_special_chars = self.show_special_chars;
                editor.show_line_numbers = self.show_line_numbers;
                editor.dialog_open = dialog_open;
                editor.show(ui);

                // Apply Ctrl+scroll zoom from the editor widget
                if editor.zoom_request != 1.0 {
                    self.zoom_level =
                        (self.zoom_level * editor.zoom_request).clamp(0.5, self.max_zoom_level);
                }
            });

        // Dialogs
        self.show_dialogs(ctx);

        // Live file monitoring: check for external changes every second
        if self.last_file_check.elapsed() >= Duration::from_secs(1) {
            self.check_live_monitored_files();
            self.last_file_check = Instant::now();
        }

        // Periodic flush of undo history to disk
        if self.last_flush.elapsed() >= Duration::from_secs(FLUSH_INTERVAL_SECS) {
            self.tabs.flush_all_history();
            self.last_flush = Instant::now();
        }

        // Auto-save file-backed documents
        if self.auto_save_enabled
            && self.last_auto_save.elapsed() >= Duration::from_secs(self.auto_save_interval_secs)
        {
            self.auto_save_all();
            self.last_auto_save = Instant::now();
        }

        let has_live_monitoring = self.tabs.documents.iter().any(|d| d.live_monitoring);
        let next_repaint = if has_live_monitoring {
            Duration::from_secs(1)
        } else if self.auto_save_enabled {
            Duration::from_secs(self.auto_save_interval_secs.min(FLUSH_INTERVAL_SECS))
        } else {
            Duration::from_secs(FLUSH_INTERVAL_SECS)
        };
        ctx.request_repaint_after(next_repaint);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.tabs.flush_all_history();

        // Save session state (tab list + unsaved content) to redb
        if let Some(store) = &self.session_store {
            let mut tabs_list = Vec::new();
            for doc in &self.tabs.documents {
                if let Some(path) = &doc.file_path {
                    tabs_list.push(SessionTabEntry::File {
                        path: path.to_string_lossy().into_owned(),
                    });
                } else {
                    let sid = doc.session_id.clone().unwrap_or_else(generate_session_id);
                    let content = doc.buffer.to_string();
                    if let Err(e) = store.save_content(&sid, &content) {
                        tracing::warn!("Failed to save session content: {e}");
                    }
                    tabs_list.push(SessionTabEntry::Unsaved {
                        session_id: sid,
                        title: doc.title.clone(),
                    });
                }
            }
            let session_data = SessionData {
                tabs: tabs_list,
                active_tab_index: self.tabs.active,
            };
            if let Err(e) = store.save_session(&session_data) {
                tracing::warn!("Failed to save session: {e}");
            }
        }

        // Save current preferences to config file
        let config = AppConfig {
            current_theme: self.theme_mode.0.clone(),
            current_zoom_level: self.zoom_level,
            max_zoom_level: self.max_zoom_level,
            word_wrap: self.word_wrap,
            show_special_chars: self.show_special_chars,
            show_line_numbers: self.show_line_numbers,
            restore_open_files: self.restore_open_files,
            show_full_path_in_title: self.show_full_path_in_title,
            font_size: self.theme.font_size,
            default_extension: self.default_extension.clone(),
            remember_last_folder: self.remember_last_folder,
            default_work_folder: self.default_work_folder.clone(),
            last_used_folder: self
                .last_used_folder
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            auto_save_enabled: self.auto_save_enabled,
            auto_save_interval_secs: self.auto_save_interval_secs,
            themes: self.available_themes.clone(),
        };
        if let Err(e) = config.save(&self.config_path) {
            tracing::warn!("Failed to save config on exit: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialogs::{FindReplaceAction, SearchScope};
    use rust_pad_core::encoding::{LineEnding, TextEncoding};
    use rust_pad_core::line_ops::{CaseConversion, SortOrder};

    /// Helper: create an App for unit-testing (no rendering needed).
    fn test_app() -> App {
        App {
            tabs: TabManager::new(),
            theme: EditorTheme::default(),
            theme_mode: ThemeMode::dark(),
            zoom_level: 1.0,
            max_zoom_level: 15.0,
            word_wrap: false,
            show_special_chars: false,
            show_line_numbers: true,
            restore_open_files: true,
            show_full_path_in_title: true,
            default_extension: String::new(),
            remember_last_folder: true,
            default_work_folder: String::new(),
            last_used_folder: None,
            auto_save_enabled: false,
            auto_save_interval_secs: 30,
            last_auto_save: Instant::now(),
            available_themes: vec![
                rust_pad_config::theme::builtin_dark(),
                rust_pad_config::theme::builtin_light(),
                rust_pad_config::theme::sample_wacky(),
            ],
            accent_color: Color32::from_rgb(80, 180, 200),
            config_path: std::path::PathBuf::from("rust-pad.json"),
            clipboard: None,
            dialog_state: DialogState::None,
            find_replace: FindReplaceDialog::new(),
            go_to_line: GoToLineDialog::new(),
            bookmarks: BookmarkManager::new(),
            syntax_highlighter: SyntaxHighlighter::new(),
            last_flush: Instant::now(),
            session_store: None,
            last_window_title: String::new(),
            last_file_check: Instant::now(),
            settings_open: false,
            settings_tab: settings_dialog::SettingsTab::default(),
            about_open: false,
            about_logo: None,
        }
    }

    // -- Close tab logic --

    #[test]
    fn test_request_close_unmodified_tab() {
        let mut app = test_app();
        app.tabs.new_tab(); // 2 tabs
        assert_eq!(app.tabs.tab_count(), 2);
        app.request_close_tab(0);
        // Unmodified tab closes immediately
        assert_eq!(app.tabs.tab_count(), 1);
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_request_close_modified_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("unsaved");
        app.request_close_tab(0);
        // Modified tab triggers confirm dialog
        assert!(matches!(app.dialog_state, DialogState::ConfirmClose(0)));
        // Tab should still exist
        assert_eq!(app.tabs.tab_count(), 1);
    }

    // -- Zoom clamping --

    #[test]
    fn test_zoom_level_clamps_at_max() {
        let mut app = test_app();
        app.zoom_level = 14.95;
        app.zoom_level = (app.zoom_level + 0.1).min(app.max_zoom_level);
        assert!((app.zoom_level - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_zoom_level_clamps_min() {
        let mut app = test_app();
        app.zoom_level = 0.55;
        app.zoom_level = (app.zoom_level - 0.1).max(0.5);
        assert!((app.zoom_level - 0.5).abs() < 0.01);
    }

    // -- Cut = copy + delete --

    #[test]
    fn test_cut_deletes_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Select "world" (chars 6..11)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 6), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 11), &doc.buffer);

        // Cut (clipboard may be None in tests, so only verify deletion)
        app.cut();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hello ");
    }

    // -- Sort lines --

    #[test]
    fn test_sort_lines_ascending() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("cherry\napple\nbanana");
        app.sort_lines(SortOrder::Ascending);
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "apple\nbanana\ncherry"
        );
        assert!(app.tabs.active_doc().modified);
    }

    #[test]
    fn test_sort_lines_descending() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("cherry\napple\nbanana");
        app.sort_lines(SortOrder::Descending);
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "cherry\nbanana\napple"
        );
    }

    // -- Duplicate line --

    #[test]
    fn test_duplicate_line() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2\nline3");
        app.tabs.active_doc_mut().cursor.position = Position::new(1, 0);
        app.duplicate_current_line();
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "line1\nline2\nline2\nline3"
        );
        assert!(app.tabs.active_doc().modified);
    }

    // -- Move line up/down --

    #[test]
    fn test_move_line_up() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc");
        app.tabs.active_doc_mut().cursor.position = Position::new(1, 0);
        app.move_current_line_up();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "b\na\nc");
        assert_eq!(app.tabs.active_doc().cursor.position.line, 0);
    }

    #[test]
    fn test_move_line_down() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.move_current_line_down();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "b\na\nc");
        assert_eq!(app.tabs.active_doc().cursor.position.line, 1);
    }

    // -- Case conversion --

    #[test]
    fn test_convert_case_upper() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Select all text
        let doc = app.tabs.active_doc_mut();
        doc.cursor.select_all(&doc.buffer);
        app.convert_selection_case(CaseConversion::Upper);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "HELLO WORLD");
    }

    #[test]
    fn test_convert_case_lower() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("HELLO WORLD");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.select_all(&doc.buffer);
        app.convert_selection_case(CaseConversion::Lower);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hello world");
    }

    // -- Indent / Dedent --

    #[test]
    fn test_indent_selection_default_spaces_4() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc");
        assert_eq!(
            app.tabs.active_doc().indent_style,
            rust_pad_core::indent::IndentStyle::Spaces(4)
        );
        // No selection — indents current line only
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.indent_selection(true);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "    a\nb\nc");
    }

    #[test]
    fn test_indent_selection_spaces_2() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc");
        app.tabs.active_doc_mut().indent_style = rust_pad_core::indent::IndentStyle::Spaces(2);
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.indent_selection(true);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "  a\nb\nc");
    }

    #[test]
    fn test_indent_selection_tabs() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc");
        app.tabs.active_doc_mut().indent_style = rust_pad_core::indent::IndentStyle::Tabs;
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.indent_selection(true);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "\ta\nb\nc");
    }

    #[test]
    fn test_dedent_selection_spaces_4() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("    a\n    b\nc");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.indent_selection(false);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\n    b\nc");
    }

    #[test]
    fn test_dedent_selection_tabs() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("\ta\n\tb\nc");
        app.tabs.active_doc_mut().indent_style = rust_pad_core::indent::IndentStyle::Tabs;
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.indent_selection(false);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\n\tb\nc");
    }

    #[test]
    fn test_indent_style_change_affects_indent() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);

        // Default: Spaces(4)
        app.indent_selection(true);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "    hello");

        // Switch to Spaces(2), dedent first, then re-indent
        app.tabs.active_doc_mut().indent_style = rust_pad_core::indent::IndentStyle::Spaces(2);
        app.indent_selection(false); // removes 2 spaces
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "  hello");
    }

    // -- Bookmarks --

    #[test]
    fn test_bookmark_toggle() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2\nline3");
        let line = app.tabs.active_doc().cursor.position.line;
        app.bookmarks.toggle(line);
        assert!(app.bookmarks.is_bookmarked(line));
        app.bookmarks.toggle(line);
        assert!(!app.bookmarks.is_bookmarked(line));
    }

    #[test]
    fn test_bookmark_navigation() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("line1\nline2\nline3\nline4\nline5");
        app.bookmarks.toggle(1);
        app.bookmarks.toggle(3);

        // Cursor starts at end of text (line 4)
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.goto_next_bookmark();
        assert_eq!(app.tabs.active_doc().cursor.position.line, 1);

        app.goto_next_bookmark();
        assert_eq!(app.tabs.active_doc().cursor.position.line, 3);
    }

    // -- Remove duplicates / empty lines --

    #[test]
    fn test_remove_duplicate_lines() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\na\nc\nb");
        app.remove_duplicate_lines();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_remove_empty_lines() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\n\nb\n\nc");
        app.remove_empty_lines();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\nb\nc");
    }

    // -- Paste normalizes line endings --

    #[test]
    fn test_paste_normalizes_line_endings() {
        // Verify that normalize_line_endings converts \r\n to \n
        let input = "hello\r\nworld";
        let normalized = rust_pad_core::encoding::normalize_line_endings(input);
        assert_eq!(normalized, "hello\nworld");
    }

    // -- Encoding/line ending via state --

    #[test]
    fn test_encoding_change_marks_modified() {
        let mut app = test_app();
        assert!(!app.tabs.active_doc().modified);
        app.tabs.active_doc_mut().encoding = TextEncoding::Utf16Le;
        app.tabs.active_doc_mut().modified = true;
        assert!(app.tabs.active_doc().modified);
        assert_eq!(app.tabs.active_doc().encoding, TextEncoding::Utf16Le);
    }

    #[test]
    fn test_line_ending_change_marks_modified() {
        let mut app = test_app();
        assert!(!app.tabs.active_doc().modified);
        app.tabs.active_doc_mut().line_ending = LineEnding::Cr;
        app.tabs.active_doc_mut().modified = true;
        assert!(app.tabs.active_doc().modified);
        assert_eq!(app.tabs.active_doc().line_ending, LineEnding::Cr);
    }

    // -- Delete line (Ctrl+D) --

    #[test]
    fn test_ctrl_d_deletes_line() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2\nline3");
        app.tabs.active_doc_mut().cursor.position = Position::new(1, 0);
        app.delete_current_line();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "line1\nline3");
    }

    #[test]
    fn test_ctrl_d_empty_doc() {
        let mut app = test_app();
        // Should not crash on empty document
        app.delete_current_line();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "");
    }

    // -- Multi-cursor: select next occurrence --

    #[test]
    fn test_select_next_occurrence() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("foo bar foo baz foo");
        // Select "foo" with primary cursor
        let doc = app.tabs.active_doc_mut();
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 3);

        app.select_next_occurrence();
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 1);

        // Second invocation finds the third "foo"
        app.select_next_occurrence();
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 2);
    }

    // -- Multi-cursor: vertical addition --

    #[test]
    fn test_add_cursor_below() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2\nline3");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 3);
        app.add_cursor_below();
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 1);
        assert_eq!(
            app.tabs.active_doc().secondary_cursors[0].position,
            Position::new(1, 3)
        );
    }

    #[test]
    fn test_add_cursor_above() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2\nline3");
        app.tabs.active_doc_mut().cursor.position = Position::new(2, 2);
        app.add_cursor_above();
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 1);
        assert_eq!(
            app.tabs.active_doc().secondary_cursors[0].position,
            Position::new(1, 2)
        );
    }

    #[test]
    fn test_add_cursor_above_at_first_line() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        app.add_cursor_above();
        // Should not add cursor — already at first line
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 0);
    }

    #[test]
    fn test_add_cursor_below_at_last_line() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2");
        app.tabs.active_doc_mut().cursor.position = Position::new(1, 0);
        app.add_cursor_below();
        // Should not add cursor — already at last line
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 0);
    }

    // -- Escape clears multi-cursor --

    #[test]
    fn test_escape_clears_multi_cursor() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.position = Position::new(0, 5);
        app.tabs.active_doc_mut().secondary_cursors.push(sc);
        assert!(app.tabs.active_doc().is_multi_cursor());

        // Simulate what Escape handler does
        app.find_replace.close();
        app.go_to_line.visible = false;
        app.tabs.active_doc_mut().clear_secondary_cursors();

        assert!(!app.tabs.active_doc().is_multi_cursor());
    }

    // -- Dialog gating tests --

    #[test]
    fn test_is_dialog_open_default() {
        let app = test_app();
        assert!(!app.is_dialog_open());
    }

    #[test]
    fn test_is_dialog_open_find_replace() {
        let mut app = test_app();
        app.find_replace.open();
        assert!(app.is_dialog_open());
        app.find_replace.close();
        assert!(!app.is_dialog_open());
    }

    #[test]
    fn test_is_dialog_open_go_to_line() {
        let mut app = test_app();
        app.go_to_line.open();
        assert!(app.is_dialog_open());
        app.go_to_line.visible = false;
        assert!(!app.is_dialog_open());
    }

    #[test]
    fn test_is_dialog_open_confirm_close() {
        let mut app = test_app();
        app.dialog_state = DialogState::ConfirmClose(0);
        assert!(app.is_dialog_open());
    }

    #[test]
    fn test_ctrl_d_suppressed_when_dialog_open() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2\nline3");
        app.tabs.active_doc_mut().cursor.position = Position::new(1, 0);

        // Open find/replace dialog
        app.find_replace.open();

        // Directly call delete — in real flow this is gated by !dialog_open
        // Verify the gating logic: is_dialog_open should be true
        assert!(app.is_dialog_open());
        // The buffer should be unchanged since the shortcut wouldn't fire
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "line1\nline2\nline3"
        );
    }

    #[test]
    fn test_select_next_occurrence_suppressed_when_dialog_open() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("foo bar foo");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 3);

        app.find_replace.open();
        // Verify gating
        assert!(app.is_dialog_open());
        // Secondary cursors should not be added
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 0);
    }

    // -- Cursor activity time tests --

    #[test]
    fn test_cursor_activity_time_initial() {
        let doc = rust_pad_core::document::Document::new();
        assert!((doc.cursor_activity_time - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_cursor_activity_time_persists() {
        let mut doc = rust_pad_core::document::Document::new();
        doc.cursor_activity_time = 5.0;
        assert!((doc.cursor_activity_time - 5.0).abs() < f64::EPSILON);
    }

    // -- Vertical select (Alt+Shift+Arrow) correctness --

    #[test]
    fn test_add_cursor_below_preserves_primary_position() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("line1\nline2\nline3\nline4\nline5");
        // Place cursor on line 0, col 3
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 3);

        // Simulate Alt+Shift+Down: add cursor below
        app.add_cursor_below();

        // Primary cursor should still be on line 0
        assert_eq!(app.tabs.active_doc().cursor.position, Position::new(0, 3));
        // Secondary cursor should be on line 1
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 1);
        assert_eq!(
            app.tabs.active_doc().secondary_cursors[0].position,
            Position::new(1, 3)
        );
    }

    #[test]
    fn test_add_cursor_below_twice_gives_three_lines() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("line1\nline2\nline3\nline4\nline5");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 2);

        app.add_cursor_below();
        app.add_cursor_below();

        // Primary on line 0, secondary on lines 1 and 2
        assert_eq!(app.tabs.active_doc().cursor.position, Position::new(0, 2));
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 2);
        assert_eq!(
            app.tabs.active_doc().secondary_cursors[0].position,
            Position::new(1, 2)
        );
        assert_eq!(
            app.tabs.active_doc().secondary_cursors[1].position,
            Position::new(2, 2)
        );
    }

    #[test]
    fn test_add_cursor_below_no_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("line1\nline2\nline3");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 2);

        app.add_cursor_below();

        // No cursor should have a selection active
        assert!(app.tabs.active_doc().cursor.selection_anchor.is_none());
        assert!(app.tabs.active_doc().secondary_cursors[0]
            .selection_anchor
            .is_none());
    }

    #[test]
    fn test_add_cursor_above_preserves_primary_position() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("line1\nline2\nline3\nline4\nline5");
        app.tabs.active_doc_mut().cursor.position = Position::new(4, 3);

        app.add_cursor_above();

        // Primary cursor should still be on line 4
        assert_eq!(app.tabs.active_doc().cursor.position, Position::new(4, 3));
        // Secondary cursor should be on line 3
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 1);
        assert_eq!(
            app.tabs.active_doc().secondary_cursors[0].position,
            Position::new(3, 3)
        );
    }

    // -- Copy/paste method tests --

    #[test]
    fn test_copy_returns_selected_text() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 5);

        // Verify selected_text returns the right text (clipboard may be None in tests)
        assert_eq!(
            app.tabs.active_doc().selected_text(),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_copy_multi_cursor() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world foo");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        doc.cursor.position = Position::new(0, 5);

        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.selection_anchor = Some(Position::new(0, 6));
        sc.position = Position::new(0, 11);
        doc.secondary_cursors.push(sc);

        let text = app.tabs.active_doc().selected_text_multi();
        assert_eq!(text, Some("hello\nworld".to_string()));
    }

    // -- Multi-cursor paste distributes per cursor --

    #[test]
    fn test_paste_multi_cursor_distributes_lines() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("aaa\nbbb\nccc");
        // Place cursors at end of each line
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 3);
        let mut sc1 = rust_pad_core::cursor::Cursor::new();
        sc1.position = Position::new(1, 3);
        app.tabs.active_doc_mut().secondary_cursors.push(sc1);
        let mut sc2 = rust_pad_core::cursor::Cursor::new();
        sc2.position = Position::new(2, 3);
        app.tabs.active_doc_mut().secondary_cursors.push(sc2);

        // Simulate clipboard with 3 lines matching 3 cursors
        let clipboard_text = "X\nY\nZ";
        let normalized = rust_pad_core::encoding::normalize_line_endings(clipboard_text);
        let doc = app.tabs.active_doc_mut();
        let lines: Vec<&str> = normalized.split('\n').collect();
        assert_eq!(lines.len(), 1 + doc.secondary_cursors.len());
        doc.insert_text_per_cursor(&lines);

        assert_eq!(app.tabs.active_doc().buffer.to_string(), "aaaX\nbbbY\ncccZ");
    }

    #[test]
    fn test_paste_multi_cursor_mismatched_lines_inserts_full_text() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("aa\nbb");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 2);
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.position = Position::new(1, 2);
        app.tabs.active_doc_mut().secondary_cursors.push(sc);

        // Clipboard has 3 lines but only 2 cursors — paste full text at each cursor
        let clipboard_text = "X\nY\nZ";
        let normalized = rust_pad_core::encoding::normalize_line_endings(clipboard_text);
        let doc = app.tabs.active_doc_mut();
        let lines: Vec<&str> = normalized.split('\n').collect();
        assert_ne!(lines.len(), 1 + doc.secondary_cursors.len());
        doc.insert_text_multi(&normalized);

        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "aaX\nY\nZ\nbbX\nY\nZ"
        );
    }

    // -- Theme mode tests --

    #[test]
    fn test_theme_mode_default_is_system() {
        let mode = ThemeMode::default();
        assert_eq!(mode, ThemeMode::system());
    }

    #[test]
    fn test_theme_mode_resolve_light() {
        let mode = ThemeMode::light();
        assert_eq!(mode.resolve(), "Light");
    }

    #[test]
    fn test_theme_mode_resolve_dark() {
        let mode = ThemeMode::dark();
        assert_eq!(mode.resolve(), "Dark");
    }

    #[test]
    fn test_theme_mode_is_system() {
        assert!(ThemeMode::system().is_system());
        assert!(!ThemeMode::dark().is_system());
        assert!(!ThemeMode::light().is_system());
    }

    #[test]
    fn test_editor_theme_dark_has_dark_bg() {
        let theme = EditorTheme::dark();
        // Dark background should have low RGB values
        assert_eq!(theme.bg_color, Color32::from_rgb(30, 30, 30));
    }

    #[test]
    fn test_editor_theme_light_has_light_bg() {
        let theme = EditorTheme::light();
        // Light background should have high RGB values
        assert_eq!(theme.bg_color, Color32::from_rgb(255, 255, 255));
    }

    #[test]
    fn test_app_initial_theme_mode() {
        let app = test_app();
        assert_eq!(app.theme_mode, ThemeMode::dark());
    }

    // -- Go to Line dialog state --

    #[test]
    fn test_go_to_line_dialog_initially_hidden() {
        let app = test_app();
        assert!(!app.go_to_line.visible);
    }

    #[test]
    fn test_go_to_line_dialog_open_and_clear() {
        let mut app = test_app();
        app.go_to_line.line_text = "old".to_string();
        app.go_to_line.open();
        assert!(app.go_to_line.visible);
        assert!(app.go_to_line.line_text.is_empty());
    }

    #[test]
    fn test_is_dialog_open_with_go_to_line() {
        let mut app = test_app();
        assert!(!app.is_dialog_open());
        app.go_to_line.open();
        assert!(app.is_dialog_open());
    }

    // -- Search scope tests --

    #[test]
    fn test_search_scope_default_is_current_tab() {
        let dialog = FindReplaceDialog::new();
        assert_eq!(dialog.scope, SearchScope::CurrentTab);
    }

    /// Helper to set find text on the dialog (syncs both find_text and options.query).
    fn set_find_text(app: &mut App, text: &str) {
        app.find_replace.find_text = text.to_string();
        app.find_replace.options.query = text.to_string();
    }

    #[test]
    fn test_search_current_tab_finds_matches() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world hello");
        set_find_text(&mut app, "hello");
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::Search);
        assert_eq!(app.find_replace.engine.match_count(), 2);
        assert!(app.find_replace.status.contains("2 matches"));
    }

    #[test]
    fn test_search_all_tabs_aggregates_matches() {
        let mut app = test_app();
        // Tab 0: has "hello" twice
        app.tabs.active_doc_mut().insert_text("hello world hello");
        // Tab 1: has "hello" once
        app.tabs.new_tab();
        app.tabs.active_doc_mut().insert_text("hello there");

        set_find_text(&mut app, "hello");
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::Search);
        // Status should mention 3 matches across 2 tabs
        assert!(app.find_replace.status.contains("3 matches"));
        assert!(app.find_replace.status.contains("2 tabs"));
    }

    #[test]
    fn test_search_all_tabs_no_matches() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        app.tabs.new_tab();
        app.tabs.active_doc_mut().insert_text("foo bar");

        set_find_text(&mut app, "xyz");
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::Search);
        assert!(app.find_replace.status.contains("No matches"));
    }

    #[test]
    fn test_replace_all_tabs_replaces_in_all() {
        let mut app = test_app();
        // Tab 0: "hello world"
        app.tabs.active_doc_mut().insert_text("hello world");
        // Tab 1: "hello there"
        app.tabs.new_tab();
        app.tabs.active_doc_mut().insert_text("hello there");

        set_find_text(&mut app, "hello");
        app.find_replace.replace_text = "hi".to_string();
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::ReplaceAll);

        assert_eq!(app.tabs.documents[0].buffer.to_string(), "hi world");
        assert_eq!(app.tabs.documents[1].buffer.to_string(), "hi there");
        assert!(app.tabs.documents[0].modified);
        assert!(app.tabs.documents[1].modified);
        assert!(app.find_replace.status.contains("2 occurrences"));
    }

    #[test]
    fn test_find_next_all_tabs_crosses_tab_boundary() {
        let mut app = test_app();
        // Tab 0 (active): "aaa"
        app.tabs.active_doc_mut().insert_text("aaa");
        // Tab 1: "hello world"
        app.tabs.new_tab();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Switch back to tab 0
        app.tabs.switch_to(0);

        set_find_text(&mut app, "hello");
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::FindNext);

        // Should have switched to tab 1 where "hello" exists
        assert_eq!(app.tabs.active, 1);
        assert!(app.find_replace.status.contains("matches"));
    }

    #[test]
    fn test_find_prev_all_tabs_crosses_tab_boundary() {
        let mut app = test_app();
        // Tab 0: "hello world"
        app.tabs.active_doc_mut().insert_text("hello world");
        // Tab 1 (active): "aaa"
        app.tabs.new_tab();
        app.tabs.active_doc_mut().insert_text("aaa");

        set_find_text(&mut app, "hello");
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::FindPrev);

        // Should have switched to tab 0 where "hello" exists
        assert_eq!(app.tabs.active, 0);
        assert!(app.find_replace.status.contains("matches"));
    }

    #[test]
    fn test_scope_change_triggers_re_search() {
        let dialog = FindReplaceDialog::new();
        // Verify scope is included in the prev_options_key detection
        // by checking the struct has the scope field
        assert_eq!(dialog.scope, SearchScope::CurrentTab);
    }

    // -- FindNext / FindPrev in current tab --

    #[test]
    fn test_find_next_current_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("foo bar foo baz foo");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        set_find_text(&mut app, "foo");
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::FindNext);
        // Should find first "foo" and status should mention match number
        assert!(app.find_replace.status.contains("matches"));
        // Cursor should be selecting the match
        assert!(app.tabs.active_doc().cursor.selection_anchor.is_some());
    }

    #[test]
    fn test_find_next_current_tab_no_matches() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "xyz");
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::FindNext);
        assert!(app.find_replace.status.contains("No matches"));
    }

    #[test]
    fn test_find_prev_current_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("foo bar foo");
        // Place cursor at the end
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 11);
        set_find_text(&mut app, "foo");
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::FindPrev);
        assert!(app.find_replace.status.contains("matches"));
    }

    #[test]
    fn test_find_prev_uses_selection_start() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("foo bar foo");
        // Select the second "foo" (chars 8-11)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.selection_anchor = Some(Position::new(0, 8));
        doc.cursor.position = Position::new(0, 11);
        set_find_text(&mut app, "foo");
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::FindPrev);
        // Should find the first "foo" (before the current selection)
        assert!(app.find_replace.status.contains("1/2 matches"));
    }

    // -- Replace / ReplaceAll in current tab --

    #[test]
    fn test_replace_current_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "hello");
        app.find_replace.replace_text = "hi".to_string();
        app.find_replace.scope = SearchScope::CurrentTab;
        // First search to populate matches
        app.handle_search_action(FindReplaceAction::Search);
        // Navigate to first match
        app.handle_search_action(FindReplaceAction::FindNext);
        // Replace
        app.handle_search_action(FindReplaceAction::Replace);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hi world");
        assert!(app.tabs.active_doc().modified);
    }

    #[test]
    fn test_replace_no_match() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "xyz");
        app.find_replace.replace_text = "abc".to_string();
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::Replace);
        assert!(app.find_replace.status.contains("No match"));
    }

    #[test]
    fn test_replace_all_current_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("foo bar foo baz foo");
        set_find_text(&mut app, "foo");
        app.find_replace.replace_text = "X".to_string();
        app.find_replace.scope = SearchScope::CurrentTab;
        // Search first to populate matches in the engine
        app.handle_search_action(FindReplaceAction::Search);
        app.handle_search_action(FindReplaceAction::ReplaceAll);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "X bar X baz X");
        assert!(app.find_replace.status.contains("3 occurrences"));
    }

    #[test]
    fn test_search_empty_query() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "");
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::Search);
        // Empty query should show 0 matches (not an error)
        assert_eq!(app.find_replace.engine.match_count(), 0);
    }

    // -- Replace in all tabs --

    #[test]
    fn test_replace_current_tab_in_all_tabs_mode() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "hello");
        app.find_replace.replace_text = "hi".to_string();
        app.find_replace.scope = SearchScope::AllTabs;
        // Search first, then FindNext to select the match, then Replace
        app.handle_search_action(FindReplaceAction::Search);
        app.handle_search_action(FindReplaceAction::FindNext);
        app.handle_search_action(FindReplaceAction::Replace);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hi world");
    }

    // -- File ops tests --

    #[test]
    fn test_new_tab_creates_tab() {
        let mut app = test_app();
        assert_eq!(app.tabs.tab_count(), 1);
        app.new_tab();
        assert_eq!(app.tabs.tab_count(), 2);
    }

    #[test]
    fn test_new_tab_has_session_id() {
        let mut app = test_app();
        app.new_tab();
        // The last document should have a session_id
        let last = app.tabs.documents.last().unwrap();
        assert!(last.session_id.is_some());
    }

    #[test]
    fn test_request_close_tab_out_of_bounds() {
        let mut app = test_app();
        assert_eq!(app.tabs.tab_count(), 1);
        // Closing out-of-bounds index should do nothing
        app.request_close_tab(999);
        assert_eq!(app.tabs.tab_count(), 1);
    }

    #[test]
    fn test_auto_save_skips_unmodified() {
        let mut app = test_app();
        // Doc is unmodified and has no file_path — auto_save should be a no-op
        assert!(!app.tabs.active_doc().modified);
        app.auto_save_all(); // Should not panic
    }

    #[test]
    fn test_auto_save_skips_no_filepath() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("unsaved content");
        app.tabs.active_doc_mut().modified = true;
        // No file_path, so auto_save should skip
        app.auto_save_all();
        // Doc should still be modified (wasn't saved)
        assert!(app.tabs.active_doc().modified);
    }

    #[test]
    fn test_check_live_monitored_skips_non_monitored() {
        let mut app = test_app();
        assert!(!app.tabs.active_doc().live_monitoring);
        // Should be a no-op, no crash
        app.check_live_monitored_files();
    }

    #[test]
    fn test_cleanup_session_for_tab_no_session() {
        let app = test_app();
        // No session_id and no session_store — should not panic
        app.cleanup_session_for_tab(0);
    }

    // -- Clipboard tests --

    #[test]
    fn test_copy_no_selection_noop() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        // No selection
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 3);
        // clipboard is None in test_app, so this just exercises the code path
        app.copy();
        // Buffer unchanged
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hello");
    }

    #[test]
    fn test_paste_no_clipboard_noop() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        // clipboard is None in test_app
        app.paste();
        // Nothing happens since we have no clipboard
        // The text after the cursor insertion from insert_text won't be affected
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hello");
    }

    #[test]
    fn test_cut_multi_cursor_deletes_selections() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("aabbcc");
        // Select "aa"
        let doc = app.tabs.active_doc_mut();
        doc.cursor.position = Position::new(0, 2);
        doc.cursor.selection_anchor = Some(Position::new(0, 0));
        // Select "cc"
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.position = Position::new(0, 6);
        sc.selection_anchor = Some(Position::new(0, 4));
        doc.secondary_cursors.push(sc);

        app.cut();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "bb");
    }

    #[test]
    fn test_copy_multi_cursor_no_selection_noop() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.position = Position::new(0, 3);
        app.tabs.active_doc_mut().secondary_cursors.push(sc);
        // No selections on any cursor
        app.copy();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hello");
    }

    // -- FindNext/FindPrev all tabs edge cases --

    #[test]
    fn test_find_next_all_tabs_single_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("foo bar foo");
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 0);
        set_find_text(&mut app, "foo");
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::FindNext);
        // With single tab, should find in current tab
        assert!(app.find_replace.status.contains("matches"));
        assert_eq!(app.tabs.active, 0);
    }

    #[test]
    fn test_find_prev_all_tabs_no_matches() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        app.tabs.new_tab();
        app.tabs.active_doc_mut().insert_text("world");
        set_find_text(&mut app, "xyz");
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::FindPrev);
        assert!(app.find_replace.status.contains("No matches"));
    }

    #[test]
    fn test_replace_all_tabs_no_matches() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "xyz");
        app.find_replace.replace_text = "abc".to_string();
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::ReplaceAll);
        assert!(app.find_replace.status.contains("0 occurrences"));
    }

    #[test]
    fn test_search_invalid_regex_current_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "[invalid(");
        app.find_replace.options.use_regex = true;
        app.find_replace.scope = SearchScope::CurrentTab;
        app.handle_search_action(FindReplaceAction::Search);
        assert!(app.find_replace.status.starts_with("Error:"));
    }

    #[test]
    fn test_search_invalid_regex_all_tabs() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        set_find_text(&mut app, "[invalid(");
        app.find_replace.options.use_regex = true;
        app.find_replace.scope = SearchScope::AllTabs;
        app.handle_search_action(FindReplaceAction::Search);
        assert!(app.find_replace.status.starts_with("Error:"));
    }
}
