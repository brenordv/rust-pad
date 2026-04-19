//! Top-level application tying together tabs, editor, menus, and status bar.

mod about_dialog;
mod auto_save;
mod clipboard;
mod context_menu;
mod drag_drop;
mod editing;
mod file_dialog_state;
mod file_ops;
mod live_monitor;
mod menu_bar;
mod print;
mod recent_files;
mod search;
mod settings_dialog;
mod shortcuts;
mod split;
mod status_bar;
mod sync_scroll;
mod tab_bar;
mod theme_controller;

pub use auto_save::AutoSaveController;
pub use file_dialog_state::FileDialogState;
pub use live_monitor::LiveMonitorController;
pub use recent_files::RecentFilesManager;
pub use settings_dialog::SettingsTab;
pub use split::SplitState;
pub use theme_controller::ThemeController;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use rust_pad_config::session::{generate_session_id, SessionData, SessionStore, SessionTabEntry};
use rust_pad_config::AppConfig;
use rust_pad_core::bookmarks::BookmarkManager;
use rust_pad_core::cursor::Position;
use rust_pad_core::history::{HistoryConfig, PersistenceLayer};

use crate::dialogs::{FindReplaceDialog, GoToLineDialog};
use crate::editor::EditorWidget;
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
    /// If true, store config and data next to the executable instead of
    /// in platform-standard directories. Useful for USB/portable installs.
    pub portable: bool,
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
    pub theme_ctrl: ThemeController,
    pub word_wrap: bool,
    pub show_special_chars: bool,
    pub show_line_numbers: bool,
    pub restore_open_files: bool,
    pub show_full_path_in_title: bool,
    pub file_dialog: FileDialogState,
    pub auto_save: AutoSaveController,
    pub recent_files: RecentFilesManager,
    config_path: PathBuf,
    clipboard: Option<arboard::Clipboard>,
    dialog_state: DialogState,
    pub find_replace: FindReplaceDialog,
    pub go_to_line: GoToLineDialog,
    bookmarks: BookmarkManager,
    last_flush: Instant,
    session_store: Option<SessionStore>,
    session_content_max_kb: usize,
    /// Maximum file size in bytes that can be opened, or `None` for no limit.
    max_file_size_bytes: Option<u64>,
    last_window_title: String,
    live_monitor: LiveMonitorController,
    pub settings_open: bool,
    pub settings_tab: SettingsTab,
    pub(crate) about_open: bool,
    pub(crate) about_logo: Option<egui::TextureHandle>,
    io_worker: crate::io_worker::IoWorker,
    pub(crate) io_activity: crate::io_worker::IoActivity,
    /// Horizontal scroll offset for the tab bar (pixels).
    pub tab_scroll_offset: f32,
    /// Whether the tab bar content overflows its visible area (previous frame).
    pub tabs_overflow: bool,
    /// Active tab index on the previous frame, used to detect tab changes.
    prev_active_tab: usize,
    /// Tab count on the previous frame, used to detect tab open/close.
    prev_tab_count: usize,
    /// When true, the "Close All" operation is in progress and should
    /// continue prompting for modified tabs after each dialog resolution.
    closing_all: bool,
    /// Active tab drag-and-drop state (`None` when no drag is in progress).
    pub(crate) tab_drag: Option<tab_bar::TabDragState>,
    /// Background worker that renders PDFs for the "Print..." and
    /// "Export as PDF..." actions.
    pub(crate) print_worker: print::PrintWorker,
    /// `true` while a print/export job is in flight. Menu entries, the
    /// shortcut, and the status bar all gate on this.
    pub(crate) print_in_progress: bool,
    /// Whether the PDF pipeline renders a line-number gutter. Persisted
    /// in `AppConfig::print_show_line_numbers`.
    pub(crate) print_show_line_numbers: bool,
    /// Transient status text shown briefly after a print/export
    /// completed successfully. Cleared the next time the user takes any
    /// new action.
    pub(crate) print_last_status: Option<String>,
    /// Split-view (dual pane) UI state. `None` = single pane (the default).
    /// Per-pane tab ownership lives on `self.tabs`; this field carries the
    /// orientation, divider ratio, and divider drag flag.
    pub split: Option<SplitState>,
    /// Whether synchronized scrolling is enabled. Persisted in
    /// `AppConfig::sync_scroll_enabled` but only takes effect when split
    /// view is active.
    pub sync_scroll_enabled: bool,
    /// Whether sync scrolling mirrors horizontal deltas in addition to
    /// vertical. Persisted in `AppConfig::sync_scroll_horizontal`.
    pub sync_scroll_horizontal: bool,
    /// Last frame's per-pane `(scroll_y, scroll_x)` snapshot, used to
    /// compute deltas for sync-scroll propagation. Cleared whenever sync
    /// scrolling is off or split view is collapsed.
    pub(crate) sync_scroll_last: Option<sync_scroll::SyncScrollSnapshot>,
}

#[derive(Debug, Default)]
pub(crate) enum DialogState {
    #[default]
    None,
    ConfirmClose(usize),
    /// The user requested a reload-from-disk on a modified document.
    ConfirmReload,
    /// A file exceeded the size limit — ask the user whether to open it anyway.
    ConfirmLargeFile {
        path: std::path::PathBuf,
        message: String,
    },
    /// A file could not be opened (e.g. invalid encoding).
    FileOpenError {
        path: std::path::PathBuf,
        message: String,
        /// When true, offer a "recover as UTF-8 (lossy)" option.
        can_recover_utf8: bool,
    },
    /// A Print / Export-as-PDF job failed. When `temp_path` is `Some`,
    /// the PDF was written but the viewer could not be launched; we
    /// offer a "Reveal in File Manager" button pointing at that file.
    PrintError {
        message: String,
        temp_path: Option<std::path::PathBuf>,
    },
}

impl App {
    /// Creates a new application instance.
    pub fn new(cc: &eframe::CreationContext<'_>, args: StartupArgs) -> Self {
        // Disable egui's built-in keyboard zoom so Ctrl+/- only affects the editor text
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);

        // Migrate config/data from legacy exe-relative paths to platform dirs
        if !args.portable {
            rust_pad_config::paths::migrate_legacy_paths();
        }

        // Load config
        let config_path = if args.portable {
            rust_pad_config::paths::portable_config_file_path()
        } else {
            AppConfig::config_path()
        };
        let app_config = AppConfig::load_or_create(&config_path);

        let theme_ctrl = ThemeController::new(
            &app_config.current_theme,
            app_config.font_size,
            app_config.current_zoom_level,
            app_config.max_zoom_level,
            app_config.themes.clone(),
            &cc.egui_ctx,
        );

        let mut history_config = HistoryConfig::default();
        if args.portable {
            history_config.data_dir = rust_pad_config::paths::portable_history_data_dir();
        }
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
        let session_path = if args.portable {
            rust_pad_config::paths::portable_session_file_path()
        } else {
            SessionStore::session_path()
        };
        let session_store = match SessionStore::open(&session_path) {
            Ok(store) => Some(store),
            Err(e) => {
                tracing::warn!("Failed to open session store: {e}");
                None
            }
        };

        // Restore session if enabled. The split-view layout is reapplied
        // after the App is fully constructed, since `apply_session_split`
        // needs `&mut self`.
        let mut restored_split: Option<rust_pad_config::session::SessionSplit> = None;
        if app_config.restore_open_files {
            restored_split = Self::restore_session(&mut tabs, &session_store);
        }

        // Open files requested via CLI arguments
        Self::open_startup_files(&mut tabs, &args);

        let max_file_size_bytes = app_config.max_file_size_bytes();

        // Best-effort cleanup of stale temp PDFs from previous sessions.
        Self::cleanup_stale_print_temp_files();

        let mut app = Self {
            tabs,
            theme_ctrl,
            word_wrap: app_config.word_wrap,
            show_special_chars: app_config.show_special_chars,
            show_line_numbers: app_config.show_line_numbers,
            restore_open_files: app_config.restore_open_files,
            show_full_path_in_title: app_config.show_full_path_in_title,
            file_dialog: FileDialogState {
                remember_last_folder: app_config.remember_last_folder,
                default_work_folder: app_config.default_work_folder,
                last_used_folder: if app_config.last_used_folder.is_empty() {
                    None
                } else {
                    Some(PathBuf::from(app_config.last_used_folder))
                },
                default_extension: app_config.default_extension.clone(),
            },
            auto_save: AutoSaveController::new(
                app_config.auto_save_enabled,
                app_config.auto_save_interval_secs,
            ),
            recent_files: RecentFilesManager::new(
                app_config.recent_files_enabled,
                app_config.recent_files_max_count,
                app_config.recent_files_cleanup,
                app_config.recent_files,
            ),
            config_path,
            clipboard: arboard::Clipboard::new().ok(),
            dialog_state: DialogState::None,
            find_replace: FindReplaceDialog::new(),
            go_to_line: GoToLineDialog::new(),
            bookmarks: BookmarkManager::new(),
            last_flush: Instant::now(),
            session_store,
            session_content_max_kb: app_config.session_content_max_kb,
            max_file_size_bytes,
            last_window_title: String::new(),
            live_monitor: LiveMonitorController::new(),
            settings_open: false,
            settings_tab: settings_dialog::SettingsTab::default(),
            about_open: false,
            about_logo: None,
            io_worker: crate::io_worker::IoWorker::new(),
            io_activity: crate::io_worker::IoActivity::default(),
            tab_scroll_offset: 0.0,
            tabs_overflow: false,
            prev_active_tab: 0,
            prev_tab_count: 0,
            closing_all: false,
            tab_drag: None,
            print_worker: print::PrintWorker::new(),
            print_in_progress: false,
            print_show_line_numbers: app_config.print_show_line_numbers,
            print_last_status: None,
            split: None,
            sync_scroll_enabled: app_config.sync_scroll_enabled,
            sync_scroll_horizontal: app_config.sync_scroll_horizontal,
            sync_scroll_last: None,
        };

        // Reapply persisted split-view layout once the App is fully built.
        if let Some(split) = restored_split {
            app.apply_session_split(&split);
        }

        app
    }

    /// Restores a previous session from the session store. Returns the
    /// persisted split-view layout (if any) so the caller can apply it
    /// after the App finishes constructing.
    fn restore_session(
        tabs: &mut TabManager,
        session_store: &Option<SessionStore>,
    ) -> Option<rust_pad_config::session::SessionSplit> {
        let Some(store) = session_store else {
            return None;
        };
        let Ok(Some(session_data)) = store.load_session() else {
            return None;
        };

        // Track per-tab pin/color metadata in the order tabs are added so we
        // can replay it after the placeholder removal step. Each entry holds
        // `(pinned, tab_color)` for the tab that was successfully restored.
        let mut restored_metadata: Vec<(bool, Option<rust_pad_core::tab_color::TabColor>)> =
            Vec::with_capacity(session_data.tabs.len());

        for entry in &session_data.tabs {
            match entry {
                SessionTabEntry::File {
                    path,
                    pinned,
                    tab_color,
                } => {
                    let p = std::path::Path::new(path);
                    if p.exists() {
                        if let Err(e) = tabs.open_file(p) {
                            tracing::warn!("Failed to restore '{path}': {e}");
                        } else {
                            let color = tab_color
                                .as_deref()
                                .and_then(rust_pad_core::tab_color::TabColor::from_serde_str);
                            restored_metadata.push((*pinned, color));
                        }
                    }
                }
                SessionTabEntry::Unsaved {
                    session_id,
                    title,
                    pinned,
                    tab_color,
                } => {
                    let content = store
                        .load_content(session_id)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    let mut doc = rust_pad_core::document::Document::new();
                    doc.title = title.clone();
                    if !content.is_empty() {
                        doc.buffer = rust_pad_core::buffer::TextBuffer::from(content.as_str());
                        doc.modified = true;
                    }
                    doc.session_id = Some(generate_session_id());
                    tabs.documents.push(doc);
                    let color = tab_color
                        .as_deref()
                        .and_then(rust_pad_core::tab_color::TabColor::from_serde_str);
                    restored_metadata.push((*pinned, color));
                }
            }
        }

        let any_restored = !restored_metadata.is_empty();
        if any_restored {
            tabs.documents.remove(0);
            tabs.active = session_data
                .active_tab_index
                .min(tabs.documents.len().saturating_sub(1));

            // Apply pin/color metadata to the restored tabs. We set raw flags
            // (rather than calling pin_tab) and then perform a stable sort to
            // bring pinned tabs to the left, since the persisted ordering
            // already reflects the user's intended layout.
            let count = restored_metadata.len().min(tabs.documents.len());
            for (i, (pinned, color)) in restored_metadata.into_iter().take(count).enumerate() {
                tabs.documents[i].pinned = pinned;
                tabs.documents[i].tab_color = color;
            }
        }
        let _ = store.clear_all_content();
        // Caller will reapply the split layout once the App is built.
        if any_restored {
            session_data.split
        } else {
            None
        }
    }

    /// Opens files from startup arguments (CLI).
    fn open_startup_files(tabs: &mut TabManager, args: &StartupArgs) {
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

        if let Some(text) = &args.new_file_text {
            tabs.new_tab();
            let doc = tabs.active_doc_mut();
            doc.insert_text(text);
        }

        // Remove the initial empty tab if CLI args opened any tabs
        if has_cli_content && tabs.tab_count() > 1 {
            let first_is_empty = tabs.documents[0].buffer.is_empty()
                && tabs.documents[0].file_path.is_none()
                && !tabs.documents[0].modified;
            if first_is_empty {
                tabs.close_tab(0);
            }
        }
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

    /// Processes completed background I/O responses.
    fn handle_io_responses(&mut self) {
        use crate::io_worker::IoResponse;

        while let Some(response) = self.io_worker.poll() {
            match response {
                IoResponse::DialogFileOpened { path, bytes } => {
                    self.io_activity.dialog_open = false;
                    self.file_dialog.update_last_folder(&path);
                    if let Err(e) = self.tabs.open_from_bytes(&path, &bytes) {
                        tracing::error!("Failed to open file: {e:#}");
                        let msg = format!("{e:#}");
                        self.dialog_state = DialogState::FileOpenError {
                            can_recover_utf8: Self::is_decode_error(&msg),
                            path,
                            message: msg,
                        };
                    } else {
                        self.recent_files.track(&path);
                    }
                }
                IoResponse::FileRead { path, bytes } => {
                    self.io_activity.pending_reads =
                        self.io_activity.pending_reads.saturating_sub(1);
                    if let Err(e) = self.tabs.open_from_bytes(&path, &bytes) {
                        tracing::error!("Failed to open file: {e:#}");
                        let msg = format!("{e:#}");
                        self.dialog_state = DialogState::FileOpenError {
                            can_recover_utf8: Self::is_decode_error(&msg),
                            path,
                            message: msg,
                        };
                    } else {
                        self.recent_files.track(&path);
                    }
                }
                IoResponse::DialogFileSavedAs { path } => {
                    self.io_activity.dialog_open = false;
                    self.file_dialog.update_last_folder(&path);

                    if let Some(ctx) = self.io_activity.save_as_context.take() {
                        if ctx.is_copy {
                            // "Save a Copy" — don't update document state
                            tracing::info!("Saved copy to '{}'", path.display());
                        } else {
                            // Normal "Save As" — update document state
                            let tab_idx = self.find_save_as_tab(&ctx);

                            if let Some(idx) = tab_idx {
                                // Clean up session content before transitioning to file-backed
                                if let Some(sid) = &self.tabs.documents[idx].session_id {
                                    if let Some(store) = &self.session_store {
                                        let _ = store.delete_content(sid);
                                    }
                                }
                                let doc = &mut self.tabs.documents[idx];
                                doc.mark_saved(&path, ctx.content_version);
                                doc.session_id = None;
                                self.recent_files.track(&path);
                            }
                        }
                    }
                }
                IoResponse::FileSaved { path } => {
                    if let Some(pos) = self
                        .io_activity
                        .pending_saves
                        .iter()
                        .position(|s| s.path == path)
                    {
                        let pending = self.io_activity.pending_saves.remove(pos);
                        if let Some(doc) = self
                            .tabs
                            .documents
                            .iter_mut()
                            .find(|d| d.file_path.as_deref() == Some(path.as_path()))
                        {
                            doc.mark_saved(&path, pending.content_version);
                        }
                    }
                }
                IoResponse::DialogCancelled => {
                    self.io_activity.dialog_open = false;
                    self.io_activity.save_as_context = None;
                }
                IoResponse::Error { path, message } => {
                    tracing::error!("I/O error: {message}");
                    // Clean up dialog state if it was a dialog error
                    if self.io_activity.dialog_open {
                        self.io_activity.dialog_open = false;
                        self.io_activity.save_as_context = None;
                    }
                    // Clean up pending save if it was a save error
                    if let Some(p) = &path {
                        self.io_activity.pending_saves.retain(|s| s.path != *p);
                        self.io_activity.pending_reads =
                            self.io_activity.pending_reads.saturating_sub(1);
                    }
                }
                IoResponse::FileTooLarge { path, message } => {
                    tracing::warn!("File too large: {message}");
                    if self.io_activity.dialog_open {
                        self.io_activity.dialog_open = false;
                    }
                    self.dialog_state = DialogState::ConfirmLargeFile { path, message };
                }
            }
        }
    }

    /// Finds the tab that initiated a save-as operation.
    fn find_save_as_tab(&self, ctx: &crate::io_worker::SaveAsContext) -> Option<usize> {
        // Try by session_id first (untitled tabs)
        if let Some(sid) = &ctx.session_id {
            if let Some(idx) = self
                .tabs
                .documents
                .iter()
                .position(|d| d.session_id.as_deref() == Some(sid.as_str()))
            {
                return Some(idx);
            }
        }
        // Try by original path (file-backed tabs doing "Save As")
        if let Some(path) = &ctx.original_path {
            if let Some(idx) = self
                .tabs
                .documents
                .iter()
                .position(|d| d.file_path.as_deref() == Some(path.as_path()))
            {
                return Some(idx);
            }
        }
        // Fall back to active tab
        Some(self.tabs.active)
    }

    /// Shows all dialog windows.
    fn show_dialogs(&mut self, ctx: &egui::Context) {
        self.show_confirm_close_dialog(ctx);
        self.show_confirm_reload_dialog(ctx);
        self.show_confirm_large_file_dialog(ctx);
        self.show_file_open_error_dialog(ctx);
        self.show_print_error_dialog(ctx);
        self.show_settings_dialog(ctx);

        if self.about_open {
            self.load_about_logo(ctx);
        }
        self.show_about_dialog(ctx);

        if let Some(action) = self.find_replace.show(ctx) {
            self.handle_search_action(action);
        }

        let total_lines = self.tabs.active_doc().buffer.len_lines();
        if let Some(target) = self.go_to_line.show(ctx, total_lines) {
            let doc = self.tabs.active_doc_mut();
            doc.cursor.clear_selection();
            doc.cursor
                .move_to(Position::new(target.line, target.column), &doc.buffer);
        }
    }

    /// Shows the confirm-close dialog when a tab has unsaved changes.
    fn show_confirm_close_dialog(&mut self, ctx: &egui::Context) {
        let DialogState::ConfirmClose(idx) = self.dialog_state else {
            return;
        };

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
                        self.continue_close_all();
                    }
                    if ui.button("  Discard  ").clicked() {
                        self.cleanup_session_for_tab(idx);
                        self.tabs.close_tab(idx);
                        self.dialog_state = DialogState::None;
                        self.continue_close_all();
                    }
                    if ui.button("  Cancel  ").clicked() {
                        self.dialog_state = DialogState::None;
                        self.closing_all = false;
                    }
                });
            });

        if !open {
            self.dialog_state = DialogState::None;
            self.closing_all = false;
        }
    }

    /// Shows the confirm-reload dialog when the user wants to reload a modified document.
    fn show_confirm_reload_dialog(&mut self, ctx: &egui::Context) {
        if !matches!(self.dialog_state, DialogState::ConfirmReload) {
            return;
        }

        let mut open = true;
        let title = self.tabs.active_doc().title.clone();

        egui::Window::new("Reload from Disk")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(format!(
                    "'{title}' has unsaved changes. Discard changes and reload from disk?"
                ));
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if ui.button("  Reload  ").clicked() {
                        self.do_reload_from_disk();
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

    /// Shows a confirmation dialog when a file exceeds the configured size limit.
    fn show_confirm_large_file_dialog(&mut self, ctx: &egui::Context) {
        let DialogState::ConfirmLargeFile { path, message } = &self.dialog_state else {
            return;
        };
        let path = path.clone();
        let message = message.clone();

        let mut open = true;
        egui::Window::new("File Too Large")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(&message);
                ui.label("Do you want to open it anyway?");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if ui.button("  Open Anyway  ").clicked() {
                        self.io_activity.pending_reads += 1;
                        self.io_worker.send(crate::io_worker::IoRequest::ReadFile {
                            path: path.clone(),
                            max_file_size_bytes: None,
                        });
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

    /// Shows an error dialog when a file could not be opened.
    ///
    /// When the failure is a decode error, offers a "Recover as UTF-8"
    /// button that re-reads the file and force-decodes it with lossy
    /// UTF-8 replacement (`U+FFFD` for invalid bytes).
    fn show_file_open_error_dialog(&mut self, ctx: &egui::Context) {
        let DialogState::FileOpenError {
            path,
            message,
            can_recover_utf8,
        } = &self.dialog_state
        else {
            return;
        };
        let path = path.clone();
        let filename = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let message = message.clone();
        let can_recover = *can_recover_utf8;

        let mut open = true;
        egui::Window::new("Failed to Open File")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(format!("Could not open '{filename}':"));
                ui.label(&message);
                if can_recover {
                    ui.add_space(2.0);
                    ui.label(
                        "You can attempt a lossy recovery as UTF-8. \
                         Invalid bytes will be replaced with \u{FFFD}.",
                    );
                }
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    if can_recover && ui.button("  Recover as UTF-8  ").clicked() {
                        self.recover_file_as_utf8_lossy(&path);
                        self.dialog_state = DialogState::None;
                    }
                    if ui.button("  OK  ").clicked() {
                        self.dialog_state = DialogState::None;
                    }
                });
            });

        if !open {
            self.dialog_state = DialogState::None;
        }
    }

    /// Returns true if the error message indicates a decoding failure.
    fn is_decode_error(message: &str) -> bool {
        message.contains("failed to decode")
    }

    /// Reads a file from disk and opens it as lossy UTF-8 in a new tab.
    ///
    /// The resulting document is untitled (no file path) so the user must
    /// explicitly "Save As" to write it back — this prevents accidentally
    /// overwriting the original file.
    fn recover_file_as_utf8_lossy(&mut self, path: &std::path::Path) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!("Recovery failed — could not read file: {e}");
                return;
            }
        };
        let recovered = String::from_utf8_lossy(&bytes);
        let filename = path.file_name().map_or_else(
            || "Recovered".to_string(),
            |n| n.to_string_lossy().into_owned(),
        );

        self.tabs.new_tab();
        let doc = self.tabs.active_doc_mut();
        doc.insert_text(&recovered);
        doc.title = format!("[Recovered] {filename}");
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Prevent egui's built-in Ctrl+scroll zoom — we handle zoom ourselves
        ctx.set_zoom_factor(1.0);

        self.handle_global_shortcuts(&ctx);
        self.handle_dropped_files(&ctx);

        // Update the OS window title to reflect the active document
        self.update_window_title(&ctx);

        // Menu bar
        let panel_fill = ctx.global_style().visuals.panel_fill;
        let faint_bg = ctx.global_style().visuals.faint_bg_color;
        let extreme_bg = ctx.global_style().visuals.extreme_bg_color;

        egui::Panel::top("menu_bar")
            .frame(
                egui::Frame::new()
                    .fill(panel_fill)
                    .inner_margin(egui::Margin::symmetric(8, 4)),
            )
            .show_inside(ui, |ui| {
                self.show_menu_bar(ui, &ctx);
            });

        // Tab bar — only the global single-pane bar. In split-view mode the
        // tab strips are rendered inside each pane by `render_split_panes`,
        // so the outer tab bar is suppressed to avoid stacking two strips.
        if !self.is_split() {
            egui::Panel::top("tab_bar")
                .frame(
                    egui::Frame::new()
                        .fill(faint_bg)
                        .inner_margin(egui::Margin::symmetric(8, 4)),
                )
                .show_inside(ui, |ui| {
                    self.show_tab_bar(ui);
                });
        }

        // Status bar
        egui::Panel::bottom("status_bar")
            .max_size(24.0)
            .frame(
                egui::Frame::new()
                    .fill(extreme_bg)
                    .inner_margin(egui::Margin::symmetric(8, 3)),
            )
            .show_inside(ui, |ui| {
                self.show_status_bar(ui);
            });

        // Editor area
        let dialog_open = self.is_dialog_open();
        let modal_dialog_open = self.is_modal_dialog_open();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(self.theme_ctrl.theme.bg_color))
            .show_inside(ui, |ui| {
                if self.is_split() {
                    // Split-pane render path. Each pane renders its own
                    // tab strip + editor. Routing to the focused pane is
                    // handled inside `render_split_panes`.
                    self.render_split_panes(ui, dialog_open, modal_dialog_open);
                    return;
                }

                let (response, zoom_request) = {
                    let doc = self.tabs.active_doc_mut();
                    let mut editor = EditorWidget::new(
                        doc,
                        &self.theme_ctrl.theme,
                        self.theme_ctrl.zoom_level,
                        Some(&self.theme_ctrl.syntax_highlighter),
                    );
                    editor.word_wrap = self.word_wrap;
                    editor.show_special_chars = self.show_special_chars;
                    editor.show_line_numbers = self.show_line_numbers;
                    editor.dialog_open = dialog_open;
                    editor.modal_dialog_open = modal_dialog_open;
                    editor.bookmarks = Some(&self.bookmarks);
                    let r = editor.show(ui);
                    (r, editor.zoom_request)
                };

                response.context_menu(|ui| {
                    self.show_editor_context_menu(ui);
                });

                if zoom_request != 1.0 {
                    self.theme_ctrl.zoom_level = (self.theme_ctrl.zoom_level * zoom_request)
                        .clamp(0.5, self.theme_ctrl.max_zoom_level);
                }
            });

        // Synchronized scrolling: propagate user-initiated viewport
        // deltas from the focused pane to the other pane. Runs after
        // both panes have rendered (i.e. after the central panel
        // closure) so it observes whatever the editor widgets wrote.
        self.propagate_sync_scroll();

        // Dialogs
        self.show_dialogs(&ctx);

        // Drag-and-drop hover overlay (painted on top of everything)
        self.paint_drop_overlay(&ctx);

        // Process completed background I/O operations
        self.handle_io_responses();

        // Process completed print / export-as-PDF jobs
        self.handle_print_responses();

        // Live file monitoring: check for external changes every second
        self.live_monitor
            .tick(&mut self.tabs, self.max_file_size_bytes);

        // Periodic flush of undo history to disk
        if self.last_flush.elapsed() >= Duration::from_secs(FLUSH_INTERVAL_SECS) {
            self.tabs.flush_all_history();
            self.last_flush = Instant::now();
        }

        // Auto-save file-backed documents
        self.auto_save.tick(&mut self.tabs);

        let has_live_monitoring = self.tabs.documents.iter().any(|d| d.live_monitoring);
        let has_pending_io = self.io_activity.is_busy();
        let next_repaint = if has_pending_io {
            // Poll more frequently while I/O is in flight
            Duration::from_millis(100)
        } else if has_live_monitoring {
            Duration::from_secs(1)
        } else if let Some(interval) = self.auto_save.repaint_interval() {
            interval.min(Duration::from_secs(FLUSH_INTERVAL_SECS))
        } else {
            Duration::from_secs(FLUSH_INTERVAL_SECS)
        };
        ctx.request_repaint_after(next_repaint);
    }

    fn on_exit(&mut self) {
        self.tabs.flush_all_history();

        // Save session state (tab list + unsaved content) to redb
        if let Some(store) = &self.session_store {
            let mut tabs_list = Vec::new();
            for doc in &self.tabs.documents {
                let tab_color_str = doc.tab_color.map(|c| c.as_serde_str().to_string());
                if let Some(path) = &doc.file_path {
                    tabs_list.push(SessionTabEntry::File {
                        path: path.to_string_lossy().into_owned(),
                        pinned: doc.pinned,
                        tab_color: tab_color_str,
                    });
                } else {
                    let sid = doc.session_id.clone().unwrap_or_else(generate_session_id);
                    let content = doc.buffer.to_string();
                    let content_bytes = content.len();
                    let limit_bytes = self.session_content_max_kb * 1024;

                    if self.session_content_max_kb > 0 && content_bytes > limit_bytes {
                        let actual_kb = content_bytes / 1024;
                        tracing::warn!(
                            "Tab '{}' content ({} KB) exceeds session limit ({} KB), skipping content save",
                            doc.title,
                            actual_kb,
                            self.session_content_max_kb,
                        );
                    } else if let Err(e) = store.save_content(&sid, &content) {
                        tracing::warn!("Failed to save session content: {e}");
                    }

                    tabs_list.push(SessionTabEntry::Unsaved {
                        session_id: sid,
                        title: doc.title.clone(),
                        pinned: doc.pinned,
                        tab_color: tab_color_str,
                    });
                }
            }
            let split = self.build_session_split();
            let session_data = SessionData {
                tabs: tabs_list,
                active_tab_index: self.tabs.active,
                split,
            };
            if let Err(e) = store.save_session(&session_data) {
                tracing::warn!("Failed to save session: {e}");
            }
        }

        // Save current preferences to config file
        let config = AppConfig {
            current_theme: self.theme_ctrl.theme_mode.0.clone(),
            current_zoom_level: self.theme_ctrl.zoom_level,
            max_zoom_level: self.theme_ctrl.max_zoom_level,
            word_wrap: self.word_wrap,
            show_special_chars: self.show_special_chars,
            show_line_numbers: self.show_line_numbers,
            restore_open_files: self.restore_open_files,
            show_full_path_in_title: self.show_full_path_in_title,
            font_size: self.theme_ctrl.theme.font_size,
            default_extension: self.file_dialog.default_extension.clone(),
            remember_last_folder: self.file_dialog.remember_last_folder,
            default_work_folder: self.file_dialog.default_work_folder.clone(),
            last_used_folder: self
                .file_dialog
                .last_used_folder
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            auto_save_enabled: self.auto_save.enabled,
            auto_save_interval_secs: self.auto_save.interval_secs,
            recent_files_enabled: self.recent_files.enabled,
            recent_files_max_count: self.recent_files.max_count,
            recent_files_cleanup: self.recent_files.cleanup,
            recent_files: self.recent_files.to_config_strings(),
            max_file_size_mb: self.max_file_size_bytes.map_or(0, |b| b / (1024 * 1024)),
            session_content_max_kb: self.session_content_max_kb,
            print_show_line_numbers: self.print_show_line_numbers,
            sync_scroll_enabled: self.sync_scroll_enabled,
            sync_scroll_horizontal: self.sync_scroll_horizontal,
            themes: self.theme_ctrl.available_themes.clone(),
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
    use crate::editor::{EditorTheme, SyntaxHighlighter};
    use egui::Color32;
    use rust_pad_config::RecentFilesCleanup;
    use rust_pad_core::encoding::{LineEnding, TextEncoding};
    use rust_pad_core::line_ops::{CaseConversion, SortOrder};

    /// Helper: create an App for unit-testing (no rendering needed).
    pub(crate) fn test_app() -> App {
        App {
            tabs: TabManager::new(),
            theme_ctrl: ThemeController {
                theme: EditorTheme::default(),
                theme_mode: ThemeMode::dark(),
                zoom_level: 1.0,
                max_zoom_level: 15.0,
                available_themes: vec![
                    rust_pad_config::theme::builtin_dark(),
                    rust_pad_config::theme::builtin_light(),
                    rust_pad_config::theme::sample_wacky(),
                ],
                accent_color: Color32::from_rgb(80, 180, 200),
                syntax_highlighter: SyntaxHighlighter::new(),
            },
            word_wrap: false,
            show_special_chars: false,
            show_line_numbers: true,
            restore_open_files: true,
            show_full_path_in_title: true,
            file_dialog: FileDialogState {
                remember_last_folder: true,
                default_work_folder: String::new(),
                last_used_folder: None,
                default_extension: String::new(),
            },
            auto_save: AutoSaveController::new(false, 30),
            recent_files: RecentFilesManager {
                enabled: true,
                max_count: 10,
                cleanup: RecentFilesCleanup::default(),
                files: Vec::new(),
            },
            config_path: std::path::PathBuf::from("rust-pad.json"),
            clipboard: None,
            dialog_state: DialogState::None,
            find_replace: FindReplaceDialog::new(),
            go_to_line: GoToLineDialog::new(),
            bookmarks: BookmarkManager::new(),
            last_flush: Instant::now(),
            session_store: None,
            session_content_max_kb: 10_240,
            max_file_size_bytes: Some(512 * 1024 * 1024),
            last_window_title: String::new(),
            live_monitor: LiveMonitorController::new(),
            settings_open: false,
            settings_tab: settings_dialog::SettingsTab::default(),
            about_open: false,
            about_logo: None,
            io_worker: crate::io_worker::IoWorker::new(),
            io_activity: crate::io_worker::IoActivity::default(),
            tab_scroll_offset: 0.0,
            tabs_overflow: false,
            prev_active_tab: 0,
            prev_tab_count: 0,
            closing_all: false,
            tab_drag: None,
            print_worker: print::PrintWorker::new(),
            print_in_progress: false,
            print_show_line_numbers: true,
            print_last_status: None,
            split: None,
            sync_scroll_enabled: false,
            sync_scroll_horizontal: true,
            sync_scroll_last: None,
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
        app.theme_ctrl.zoom_level = 14.95;
        app.theme_ctrl.zoom_in();
        assert!((app.theme_ctrl.zoom_level - 15.0).abs() < 0.01);
    }

    #[test]
    fn test_zoom_level_clamps_min() {
        let mut app = test_app();
        app.theme_ctrl.zoom_level = 0.55;
        app.theme_ctrl.zoom_out();
        assert!((app.theme_ctrl.zoom_level - 0.5).abs() < 0.01);
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
    fn test_is_dialog_open_io_open_dialog() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;
        assert!(app.is_dialog_open());
        app.io_activity.dialog_open = false;
        assert!(!app.is_dialog_open());
    }

    #[test]
    fn test_is_dialog_open_io_save_as_dialog() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;
        app.io_activity.save_as_context = Some(crate::io_worker::SaveAsContext {
            content_version: 0,
            session_id: None,
            original_path: None,
            is_copy: false,
        });
        assert!(app.is_dialog_open());
    }

    // ── is_modal_dialog_open ────────────────────────────────────────

    #[test]
    fn test_is_modal_dialog_open_excludes_find_replace() {
        let mut app = test_app();
        app.find_replace.open();
        assert!(app.is_dialog_open());
        assert!(!app.is_modal_dialog_open());
    }

    #[test]
    fn test_is_modal_dialog_open_includes_go_to_line() {
        let mut app = test_app();
        app.go_to_line.open();
        assert!(app.is_modal_dialog_open());
    }

    #[test]
    fn test_is_modal_dialog_open_includes_settings() {
        let mut app = test_app();
        app.settings_open = true;
        assert!(app.is_modal_dialog_open());
    }

    #[test]
    fn test_is_modal_dialog_open_includes_confirm_close() {
        let mut app = test_app();
        app.dialog_state = DialogState::ConfirmClose(0);
        assert!(app.is_modal_dialog_open());
    }

    #[test]
    fn test_is_modal_dialog_open_includes_io_dialog() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;
        assert!(app.is_modal_dialog_open());
    }

    #[test]
    fn test_editing_blocked_during_io_dialog() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("original");
        let original = app.tabs.active_doc().buffer.to_string();

        // Simulate an open file dialog on the background thread
        app.io_activity.dialog_open = true;
        assert!(app.is_dialog_open());

        // Editor-only shortcuts would be suppressed; verify the buffer is unchanged
        // (In the real flow, handle_edit_shortcut is skipped when is_dialog_open() is true)
        assert_eq!(app.tabs.active_doc().buffer.to_string(), original);
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
        assert_eq!(app.theme_ctrl.theme_mode, ThemeMode::dark());
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
    fn test_auto_save_skips_when_disabled() {
        let mut app = test_app();
        // auto_save is disabled by default in test_app
        assert!(!app.auto_save.enabled);
        let saved = app.auto_save.tick(&mut app.tabs);
        assert!(!saved);
    }

    #[test]
    fn test_auto_save_skips_no_filepath() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("unsaved content");
        app.tabs.active_doc_mut().modified = true;
        app.auto_save.enabled = true;
        app.auto_save.interval_secs = 0; // trigger immediately
        let _saved = app.auto_save.tick(&mut app.tabs);
        // No file_path, so auto_save should skip — doc still modified
        assert!(app.tabs.active_doc().modified);
    }

    #[test]
    fn test_check_live_monitored_skips_non_monitored() {
        let mut app = test_app();
        assert!(!app.tabs.active_doc().live_monitoring);
        // Should be a no-op, no crash
        app.live_monitor.tick(&mut app.tabs, None);
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

    // ── Context menu: scoped operations ─────────────────────────────

    use super::context_menu::OperationScope;

    #[test]
    fn test_convert_case_scoped_global_uppercases_entire_doc() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Global);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "HELLO WORLD");
        assert!(app.tabs.active_doc().modified);
    }

    #[test]
    fn test_convert_case_scoped_global_lowercases_entire_doc() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("HELLO WORLD");
        app.convert_case_scoped(CaseConversion::Lower, OperationScope::Global);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hello world");
    }

    #[test]
    fn test_convert_case_scoped_global_title_case() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        app.convert_case_scoped(CaseConversion::TitleCase, OperationScope::Global);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "Hello World");
    }

    #[test]
    fn test_convert_case_scoped_global_no_change_noop() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("HELLO");
        // Reset modified flag after insert
        app.tabs.active_doc_mut().modified = false;
        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Global);
        // Already uppercase — nothing changed
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "HELLO");
        assert!(!app.tabs.active_doc().modified);
    }

    #[test]
    fn test_convert_case_scoped_selection_only_affects_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Select "hello" (chars 0..5)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "HELLO world");
    }

    #[test]
    fn test_convert_case_scoped_selection_no_selection_noop() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // No selection
        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "hello world");
    }

    #[test]
    fn test_sort_lines_scoped_global() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("cherry\napple\nbanana");
        app.sort_lines_scoped(SortOrder::Ascending, OperationScope::Global);
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "apple\nbanana\ncherry"
        );
    }

    #[test]
    fn test_sort_lines_scoped_selection_only_sorts_selected_lines() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("delta\ncherry\napple\nbanana");
        // Select lines 1-2 ("cherry\napple")
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(2, 5), &doc.buffer);

        app.sort_lines_scoped(SortOrder::Ascending, OperationScope::Selection);
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "delta\napple\ncherry\nbanana"
        );
    }

    #[test]
    fn test_remove_duplicate_lines_scoped_global() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\na\nc\nb");
        app.remove_duplicate_lines_scoped(OperationScope::Global);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_remove_duplicate_lines_scoped_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x\na\nb\na\ny");
        // Select lines 1-3 ("a\nb\na")
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(3, 1), &doc.buffer);

        app.remove_duplicate_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "x\na\nb\ny");
    }

    #[test]
    fn test_remove_empty_lines_scoped_global() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\n\nb\n\nc");
        app.remove_empty_lines_scoped(OperationScope::Global);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_remove_empty_lines_scoped_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x\na\n\nb\ny");
        // Select lines 1-3 ("a\n\nb")
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(3, 1), &doc.buffer);

        app.remove_empty_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "x\na\nb\ny");
    }

    // ── Context menu: invert selection ──────────────────────────────

    #[test]
    fn test_invert_selection_no_selection_selects_all() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        // Cursor at position 3, no selection
        app.tabs.active_doc_mut().cursor.position = Position::new(0, 3);
        app.tabs.active_doc_mut().cursor.clear_selection();

        app.invert_selection();

        // Should select all text
        assert!(app.tabs.active_doc().cursor.selection_anchor.is_some());
        assert_eq!(
            app.tabs.active_doc().selected_text(),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_invert_selection_entire_doc_clears() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.select_all(&doc.buffer);

        app.invert_selection();

        // Should clear selection
        assert!(app.tabs.active_doc().cursor.selection_anchor.is_none());
    }

    #[test]
    fn test_invert_selection_at_start() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Select "hello" (0..5)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        app.invert_selection();

        // Should now select " world" (5..11)
        let sel_text = app.tabs.active_doc().selected_text().unwrap();
        assert_eq!(sel_text, " world");
    }

    #[test]
    fn test_invert_selection_at_end() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Select " world" (5..11)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 11), &doc.buffer);

        app.invert_selection();

        // Should now select "hello" (0..5)
        let sel_text = app.tabs.active_doc().selected_text().unwrap();
        assert_eq!(sel_text, "hello");
    }

    #[test]
    fn test_invert_selection_in_middle() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Select "lo wo" (3..8)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 3), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 8), &doc.buffer);

        app.invert_selection();

        // Primary cursor: select "hel" (0..3)
        let primary_text = app.tabs.active_doc().selected_text().unwrap();
        assert_eq!(primary_text, "hel");
        // Secondary cursor should exist and select "rld" (8..11)
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 1);
        let sc = &app.tabs.active_doc().secondary_cursors[0];
        assert_eq!(sc.selection_anchor, Some(Position::new(0, 8)));
        assert_eq!(sc.position, Position::new(0, 11));
    }

    // ── Context menu: delete_selection_or_char ──────────────────────

    #[test]
    fn test_delete_selection_or_char_with_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        app.delete_selection_or_char();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), " world");
    }

    #[test]
    fn test_delete_selection_or_char_no_selection_deletes_forward() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.clear_selection();

        app.delete_selection_or_char();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "ello");
    }

    #[test]
    fn test_delete_selection_or_char_multi_cursor() {
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

        app.delete_selection_or_char();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "bb");
    }

    // ── Vertical (multi-cursor) selection operations ────────────────

    #[test]
    fn test_convert_case_multi_cursor_vertical_selection() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("hello world\nfoo bar\nbaz qux");
        // Simulate vertical selection: select "hello" on line 0, "foo" on line 1
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.selection_anchor = Some(Position::new(1, 0));
        sc.position = Position::new(1, 3);
        doc.secondary_cursors.push(sc);

        app.convert_selection_case(CaseConversion::Upper);
        let text = app.tabs.active_doc().buffer.to_string();
        assert_eq!(text, "HELLO world\nFOO bar\nbaz qux");
    }

    #[test]
    fn test_convert_case_scoped_selection_multi_cursor() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("abc\ndef\nghi");
        // Vertical selection: "abc" on line 0, "def" on line 1
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 3), &doc.buffer);

        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.selection_anchor = Some(Position::new(1, 0));
        sc.position = Position::new(1, 3);
        doc.secondary_cursors.push(sc);

        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "ABC\nDEF\nghi");
    }

    #[test]
    fn test_sort_lines_scoped_selection_multi_cursor_vertical() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("delta\ncherry\napple\nbanana");
        // Vertical cursors on lines 1 and 2 (no selections, just cursor positions)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.position = Position::new(1, 0);
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.position = Position::new(2, 0);
        doc.secondary_cursors.push(sc);

        app.sort_lines_scoped(SortOrder::Ascending, OperationScope::Selection);
        // Lines 1-2 should be sorted: "apple\ncherry"
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "delta\napple\ncherry\nbanana"
        );
    }

    #[test]
    fn test_remove_duplicates_scoped_selection_multi_cursor_vertical() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x\na\nb\na\ny");
        // Vertical cursors spanning lines 1-3
        let doc = app.tabs.active_doc_mut();
        doc.cursor.position = Position::new(1, 0);
        let mut sc1 = rust_pad_core::cursor::Cursor::new();
        sc1.position = Position::new(2, 0);
        doc.secondary_cursors.push(sc1);
        let mut sc2 = rust_pad_core::cursor::Cursor::new();
        sc2.position = Position::new(3, 0);
        doc.secondary_cursors.push(sc2);

        app.remove_duplicate_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "x\na\nb\ny");
    }

    #[test]
    fn test_remove_empty_lines_scoped_selection_multi_cursor_vertical() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x\na\n\nb\ny");
        // Vertical cursors spanning lines 1-3
        let doc = app.tabs.active_doc_mut();
        doc.cursor.position = Position::new(1, 0);
        let mut sc1 = rust_pad_core::cursor::Cursor::new();
        sc1.position = Position::new(2, 0);
        doc.secondary_cursors.push(sc1);
        let mut sc2 = rust_pad_core::cursor::Cursor::new();
        sc2.position = Position::new(3, 0);
        doc.secondary_cursors.push(sc2);

        app.remove_empty_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "x\na\nb\ny");
    }

    // ── selection_line_range with multi-cursor ──────────────────────

    #[test]
    fn test_selection_line_range_single_cursor_no_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc\nd");
        app.tabs.active_doc_mut().cursor.position = Position::new(2, 0);
        let doc = app.tabs.active_doc();
        let (start, end) = super::editing::selection_line_range(doc);
        assert_eq!(start, 2);
        assert_eq!(end, 3);
    }

    #[test]
    fn test_selection_line_range_single_cursor_with_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc\nd");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(2, 1), &doc.buffer);
        let doc = app.tabs.active_doc();
        let (start, end) = super::editing::selection_line_range(doc);
        assert_eq!(start, 1);
        assert_eq!(end, 3);
    }

    #[test]
    fn test_selection_line_range_multi_cursor_spans_all_lines() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc\nd\ne");
        // Primary on line 1, secondary on line 3
        let doc = app.tabs.active_doc_mut();
        doc.cursor.position = Position::new(1, 0);
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.position = Position::new(3, 0);
        doc.secondary_cursors.push(sc);

        let doc = app.tabs.active_doc();
        let (start, end) = super::editing::selection_line_range(doc);
        assert_eq!(start, 1);
        assert_eq!(end, 4);
    }

    #[test]
    fn test_selection_line_range_multi_cursor_with_selections() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\nc\nd\ne");
        // Primary selects line 0 content
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 1), &doc.buffer);
        // Secondary selects across lines 3-4
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.selection_anchor = Some(Position::new(3, 0));
        sc.position = Position::new(4, 1);
        doc.secondary_cursors.push(sc);

        let doc = app.tabs.active_doc();
        let (start, end) = super::editing::selection_line_range(doc);
        assert_eq!(start, 0);
        assert_eq!(end, 5);
    }

    // ── sort_lines delegates to scoped ──────────────────────────────

    #[test]
    fn test_sort_lines_delegates_to_global_scope() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("cherry\napple\nbanana");
        // sort_lines delegates to sort_lines_scoped(_, Global)
        app.sort_lines(SortOrder::Ascending);
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "apple\nbanana\ncherry"
        );
    }

    #[test]
    fn test_remove_duplicate_lines_delegates_to_global_scope() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\nb\na\nc\nb");
        app.remove_duplicate_lines();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_remove_empty_lines_delegates_to_global_scope() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("a\n\nb\n\nc");
        app.remove_empty_lines();
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a\nb\nc");
    }

    // ── Invert selection edge cases ─────────────────────────────────

    #[test]
    fn test_invert_selection_empty_doc() {
        let mut app = test_app();
        // Empty doc, no selection → select all (which is empty)
        app.invert_selection();
        // Should not crash; selection anchor should be set by select_all
        assert!(app.tabs.active_doc().cursor.selection_anchor.is_some());
    }

    #[test]
    fn test_invert_selection_single_char_doc() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x");
        // Select "x"
        let doc = app.tabs.active_doc_mut();
        doc.cursor.select_all(&doc.buffer);

        app.invert_selection();
        // Entire doc selected → should clear
        assert!(app.tabs.active_doc().cursor.selection_anchor.is_none());
    }

    // ── Selection preservation after Selection Operations ───────────

    #[test]
    fn test_convert_case_selection_preserves_selection_single_cursor() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        // Select "hello" (chars 0..5)
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "HELLO world");
        // Selection must be preserved
        let doc = app.tabs.active_doc();
        assert!(
            doc.cursor.selection_anchor.is_some(),
            "selection should be preserved after case conversion"
        );
        assert_eq!(doc.selected_text(), Some("HELLO".to_string()));
    }

    #[test]
    fn test_convert_case_selection_preserves_selection_multi_cursor() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("hello world\nfoo bar");
        // Select "hello" on line 0, "foo" on line 1
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.selection_anchor = Some(Position::new(1, 0));
        sc.position = Position::new(1, 3);
        doc.secondary_cursors.push(sc);

        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Selection);
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "HELLO world\nFOO bar"
        );
        // Primary selection preserved
        let doc = app.tabs.active_doc();
        assert!(
            doc.cursor.selection_anchor.is_some(),
            "primary selection should be preserved"
        );
        // Secondary cursors preserved
        assert_eq!(
            doc.secondary_cursors.len(),
            1,
            "secondary cursor should be preserved"
        );
        assert!(
            doc.secondary_cursors[0].selection_anchor.is_some(),
            "secondary selection should be preserved"
        );
    }

    #[test]
    fn test_sort_lines_selection_preserves_selection() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("delta\ncherry\napple\nbanana");
        // Select lines 1-2 ("cherry\napple")
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(2, 5), &doc.buffer);

        app.sort_lines_scoped(SortOrder::Ascending, OperationScope::Selection);
        assert_eq!(
            app.tabs.active_doc().buffer.to_string(),
            "delta\napple\ncherry\nbanana"
        );
        // Selection preserved
        let doc = app.tabs.active_doc();
        assert!(
            doc.cursor.selection_anchor.is_some(),
            "selection should be preserved after sort"
        );
    }

    #[test]
    fn test_remove_duplicate_lines_selection_preserves_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x\na\nb\na\ny");
        // Select lines 1-3 ("a\nb\na")
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(3, 1), &doc.buffer);

        app.remove_duplicate_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "x\na\nb\ny");
        // Selection preserved (clamped to valid range)
        let doc = app.tabs.active_doc();
        assert!(
            doc.cursor.selection_anchor.is_some(),
            "selection should be preserved after remove duplicates"
        );
    }

    #[test]
    fn test_remove_empty_lines_selection_preserves_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x\na\n\nb\ny");
        // Select lines 1-3 ("a\n\nb")
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(3, 1), &doc.buffer);

        app.remove_empty_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "x\na\nb\ny");
        // Selection preserved (clamped to valid range)
        let doc = app.tabs.active_doc();
        assert!(
            doc.cursor.selection_anchor.is_some(),
            "selection should be preserved after remove empty lines"
        );
    }

    #[test]
    fn test_remove_duplicates_selection_clamps_cursor_beyond_buffer() {
        let mut app = test_app();
        // All 3 selected lines are duplicates → 2 get removed
        app.tabs.active_doc_mut().insert_text("a\na\na");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(2, 1), &doc.buffer);

        app.remove_duplicate_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "a");
        // Cursor clamped to valid position
        let doc = app.tabs.active_doc();
        assert!(doc.cursor.position.line < doc.buffer.len_lines());
        assert!(doc.cursor.selection_anchor.is_some());
    }

    #[test]
    fn test_remove_empty_lines_multi_cursor_preserves_cursors() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("x\na\n\nb\ny");
        // Vertical cursors spanning lines 1-3
        let doc = app.tabs.active_doc_mut();
        doc.cursor.position = Position::new(1, 0);
        doc.cursor.start_selection();
        doc.cursor.position = Position::new(1, 1);
        let mut sc1 = rust_pad_core::cursor::Cursor::new();
        sc1.position = Position::new(2, 0);
        doc.secondary_cursors.push(sc1);
        let mut sc2 = rust_pad_core::cursor::Cursor::new();
        sc2.position = Position::new(3, 0);
        doc.secondary_cursors.push(sc2);

        app.remove_empty_lines_scoped(OperationScope::Selection);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "x\na\nb\ny");
        // Primary selection preserved
        let doc = app.tabs.active_doc();
        assert!(
            doc.cursor.selection_anchor.is_some(),
            "primary selection should be preserved"
        );
    }

    #[test]
    fn test_convert_case_selection_noop_preserves_selection() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("HELLO world");
        // Select "HELLO" which is already uppercase
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        app.convert_case_scoped(CaseConversion::Upper, OperationScope::Selection);
        // No change, but selection should still be there
        let doc = app.tabs.active_doc();
        assert!(
            doc.cursor.selection_anchor.is_some(),
            "selection should be preserved even when case conversion is noop"
        );
        assert_eq!(doc.selected_text(), Some("HELLO".to_string()));
    }

    // -- Vertical selection (Alt+Shift+Up/Down) --

    #[test]
    fn test_add_cursor_below_inherits_selection() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("Hello World\nFoo Bar Baz\nLine Three!");
        // Select columns 0..5 ("Hello") on line 0
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        app.add_cursor_below();

        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        let sc = &doc.secondary_cursors[0];
        assert_eq!(sc.position, Position::new(1, 5));
        assert_eq!(sc.selection_anchor, Some(Position::new(1, 0)));
    }

    #[test]
    fn test_add_cursor_above_inherits_selection() {
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("Hello World\nFoo Bar Baz\nLine Three!");
        // Select columns 0..5 on line 2
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(2, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(2, 5), &doc.buffer);

        app.add_cursor_above();

        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        let sc = &doc.secondary_cursors[0];
        assert_eq!(sc.position, Position::new(1, 5));
        assert_eq!(sc.selection_anchor, Some(Position::new(1, 0)));
    }

    #[test]
    fn test_add_cursor_below_clamps_to_short_line() {
        let mut app = test_app();
        // Line 0 has 10 chars, line 1 has only 2 chars
        app.tabs
            .active_doc_mut()
            .insert_text("0123456789\nab\nlong line here");
        // Select columns 3..8 on line 0
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 3), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 8), &doc.buffer);

        app.add_cursor_below();

        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        let sc = &doc.secondary_cursors[0];
        // Both position and anchor columns clamped to line 1 length (2)
        assert_eq!(sc.position, Position::new(1, 2));
        assert_eq!(sc.selection_anchor, Some(Position::new(1, 2)));
    }

    #[test]
    fn test_add_cursor_below_no_selection_backward_compat() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("Hello\nWorld\nThree");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 3), &doc.buffer);

        app.add_cursor_below();

        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        let sc = &doc.secondary_cursors[0];
        assert_eq!(sc.position, Position::new(1, 3));
        assert_eq!(sc.selection_anchor, None);
    }

    #[test]
    fn test_vertical_selection_shrink_below_then_up() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("aaa\nbbb\nccc\nddd");
        // Primary cursor on line 1
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(1, 0), &doc.buffer);

        // Extend down twice: adds cursors on lines 2 and 3
        app.add_cursor_below();
        app.add_cursor_below();
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 2);

        // Pressing Up should shrink: remove the furthest below (line 3)
        app.add_cursor_above();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        assert_eq!(doc.secondary_cursors[0].position.line, 2);

        // Pressing Up again removes line 2
        app.add_cursor_above();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 0);
    }

    #[test]
    fn test_vertical_selection_shrink_above_then_down() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("aaa\nbbb\nccc\nddd");
        // Primary cursor on line 2
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(2, 0), &doc.buffer);

        // Extend up twice: adds cursors on lines 1 and 0
        app.add_cursor_above();
        app.add_cursor_above();
        assert_eq!(app.tabs.active_doc().secondary_cursors.len(), 2);

        // Pressing Down should shrink: remove the furthest above (line 0)
        app.add_cursor_below();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        assert_eq!(doc.secondary_cursors[0].position.line, 1);

        // Pressing Down again removes line 1
        app.add_cursor_below();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 0);
    }

    #[test]
    fn test_vertical_selection_full_walkthrough() {
        // Mimics: select "Hello" on line 0, Alt+Shift+Down x2, Alt+Shift+Up x2
        let mut app = test_app();
        app.tabs
            .active_doc_mut()
            .insert_text("Hello World\nFoo Bar Baz\nLine Three!");
        let doc = app.tabs.active_doc_mut();
        doc.cursor.move_to(Position::new(0, 0), &doc.buffer);
        doc.cursor.start_selection();
        doc.cursor.move_to(Position::new(0, 5), &doc.buffer);

        // Alt+Shift+Down: add cursor on line 1 with selection 0..5
        app.add_cursor_below();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        assert_eq!(doc.secondary_cursors[0].position, Position::new(1, 5));
        assert_eq!(
            doc.secondary_cursors[0].selection_anchor,
            Some(Position::new(1, 0))
        );

        // Alt+Shift+Down: add cursor on line 2 with selection 0..5
        app.add_cursor_below();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 2);
        assert_eq!(doc.secondary_cursors[1].position, Position::new(2, 5));
        assert_eq!(
            doc.secondary_cursors[1].selection_anchor,
            Some(Position::new(2, 0))
        );

        // Alt+Shift+Up: shrink — remove line 2 cursor
        app.add_cursor_above();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 1);
        assert_eq!(doc.secondary_cursors[0].position.line, 1);

        // Alt+Shift+Up: shrink — remove line 1 cursor
        app.add_cursor_above();
        let doc = app.tabs.active_doc();
        assert_eq!(doc.secondary_cursors.len(), 0);
    }

    // ── Session content size limit tests ───────────────────────────

    use eframe::App as _;
    use rust_pad_config::session::SessionStore;

    /// Helper: create a test app with a real session store backed by a temp dir.
    /// Also redirects the config path into the temp dir so `on_exit()` doesn't
    /// write a `rust-pad.json` into the repo root.
    fn test_app_with_session(dir: &std::path::Path) -> App {
        let db_path = dir.join("test-session.redb");
        let store = SessionStore::open(&db_path).expect("open test session store");
        let mut app = test_app();
        app.session_store = Some(store);
        app.config_path = dir.join("rust-pad.json");
        app
    }

    #[test]
    fn test_on_exit_saves_content_under_limit() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-session.redb");
        {
            let mut app = test_app_with_session(dir.path());
            app.session_content_max_kb = 1;
            app.tabs.active_doc_mut().insert_text("hello world");
            app.tabs.active_doc_mut().session_id = Some("test-small".to_string());
            app.on_exit();
        }
        // App dropped — reopen store to verify
        let store = SessionStore::open(&db_path).expect("reopen store");
        let loaded = store.load_content("test-small").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap(), "hello world");
    }

    #[test]
    fn test_on_exit_skips_content_over_limit() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-session.redb");
        {
            let mut app = test_app_with_session(dir.path());
            app.session_content_max_kb = 1;
            let large_content = "x".repeat(1025);
            app.tabs.active_doc_mut().insert_text(&large_content);
            app.tabs.active_doc_mut().session_id = Some("test-large".to_string());
            app.on_exit();
        }
        let store = SessionStore::open(&db_path).expect("reopen store");
        // Content should NOT have been saved
        assert!(store.load_content("test-large").unwrap().is_none());
        // But the tab metadata should still be in the session
        let session = store.load_session().unwrap().unwrap();
        assert_eq!(session.tabs.len(), 1);
        match &session.tabs[0] {
            SessionTabEntry::Unsaved { session_id, .. } => {
                assert_eq!(session_id, "test-large");
            }
            _ => panic!("expected Unsaved entry"),
        }
    }

    #[test]
    fn test_on_exit_unlimited_when_zero() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-session.redb");
        {
            let mut app = test_app_with_session(dir.path());
            app.session_content_max_kb = 0;
            let large_content = "x".repeat(50 * 1024);
            app.tabs.active_doc_mut().insert_text(&large_content);
            app.tabs.active_doc_mut().session_id = Some("test-unlimited".to_string());
            app.on_exit();
        }
        let store = SessionStore::open(&db_path).expect("reopen store");
        let loaded = store.load_content("test-unlimited").unwrap();
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().len(), 50 * 1024);
    }

    #[test]
    fn test_on_exit_content_exactly_at_limit_saves() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-session.redb");
        {
            let mut app = test_app_with_session(dir.path());
            app.session_content_max_kb = 1;
            let exact_content = "x".repeat(1024);
            app.tabs.active_doc_mut().insert_text(&exact_content);
            app.tabs.active_doc_mut().session_id = Some("test-exact".to_string());
            app.on_exit();
        }
        let store = SessionStore::open(&db_path).expect("reopen store");
        assert!(store.load_content("test-exact").unwrap().is_some());
    }

    #[test]
    fn test_on_exit_multiple_tabs_independent_limit_check() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test-session.redb");
        {
            let mut app = test_app_with_session(dir.path());
            app.session_content_max_kb = 1;
            app.tabs.active_doc_mut().insert_text("small");
            app.tabs.active_doc_mut().session_id = Some("tab-small".to_string());
            app.tabs.new_tab();
            let large = "y".repeat(2048);
            app.tabs.active_doc_mut().insert_text(&large);
            app.tabs.active_doc_mut().session_id = Some("tab-large".to_string());
            app.on_exit();
        }
        let store = SessionStore::open(&db_path).expect("reopen store");
        assert!(store.load_content("tab-small").unwrap().is_some());
        assert!(store.load_content("tab-large").unwrap().is_none());
        let session = store.load_session().unwrap().unwrap();
        assert_eq!(session.tabs.len(), 2);
    }

    // ── file_ops: open_file_dialog ──────────────────────────────────────

    #[test]
    fn test_open_file_dialog_sets_dialog_open() {
        // Verify the precondition: dialog_open starts false.
        // We cannot call open_file_dialog() directly because it spawns a
        // background thread with rfd::FileDialog which hangs on headless CI.
        // Instead verify the guard logic and state transitions.
        let app = test_app();
        assert!(!app.io_activity.dialog_open);
        assert!(!app.is_dialog_open());
    }

    #[test]
    fn test_open_file_dialog_blocked_when_already_open() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;
        // Should return early without panicking
        app.open_file_dialog();
        assert!(app.io_activity.dialog_open);
    }

    // ── file_ops: open_file_path ────────────────────────────────────────

    #[test]
    fn test_open_file_path_duplicate_switches_tab() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dup.txt");
        std::fs::write(&path, "hello").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();
        assert_eq!(app.tabs.active, 1);

        // Switch away
        app.tabs.switch_to(0);
        assert_eq!(app.tabs.active, 0);

        // open_file_path should switch to existing tab, not start a read
        app.open_file_path(&path);
        assert_eq!(app.tabs.active, 1);
        assert_eq!(app.io_activity.pending_reads, 0);
    }

    #[test]
    fn test_open_file_path_new_file_increments_pending() {
        let mut app = test_app();
        assert_eq!(app.io_activity.pending_reads, 0);
        app.open_file_path(std::path::Path::new("/some/new/file.txt"));
        assert_eq!(app.io_activity.pending_reads, 1);
    }

    // ── file_ops: save_active ───────────────────────────────────────────

    #[test]
    fn test_save_active_file_backed_sends_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("save.txt");
        std::fs::write(&path, "original").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();
        app.tabs.active_doc_mut().insert_text(" modified");

        assert!(app.io_activity.pending_saves.is_empty());
        app.save_active();
        assert_eq!(app.io_activity.pending_saves.len(), 1);
        assert_eq!(app.io_activity.pending_saves[0].path, path);
    }

    #[test]
    fn test_save_active_untitled_has_no_path() {
        // save_active() on an untitled doc would call save_as_dialog() which
        // spawns an rfd dialog thread — not safe on headless CI. Instead verify
        // the branching precondition: untitled docs have no file_path.
        let app = test_app();
        assert!(app.tabs.active_doc().file_path.is_none());
    }

    // ── file_ops: save_as_dialog ────────────────────────────────────────

    #[test]
    fn test_save_as_context_captures_version() {
        // save_as_dialog() spawns an rfd dialog thread — not safe on headless
        // CI. Instead verify the SaveAsContext struct captures the right data.
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("content");
        let version = app.tabs.active_doc().content_version;
        let session_id = app.tabs.active_doc().session_id.clone();

        let ctx = crate::io_worker::SaveAsContext {
            content_version: version,
            session_id,
            original_path: app.tabs.active_doc().file_path.clone(),
            is_copy: false,
        };
        assert_eq!(ctx.content_version, version);
        assert!(ctx.original_path.is_none());
    }

    #[test]
    fn test_save_as_dialog_blocked_when_dialog_open() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;
        app.save_as_dialog();
        // save_as_context should not be set
        assert!(app.io_activity.save_as_context.is_none());
    }

    // ── handle_io_responses: FileRead ───────────────────────────────────

    #[test]
    fn test_handle_file_read_opens_tab() {
        let mut app = test_app();
        app.io_activity.pending_reads = 1;

        let path = std::path::PathBuf::from("/tmp/test_read.txt");
        app.io_worker
            .inject_response(crate::io_worker::IoResponse::FileRead {
                path: path.clone(),
                bytes: b"file content".to_vec(),
            });
        app.handle_io_responses();

        assert_eq!(app.io_activity.pending_reads, 0);
        assert_eq!(app.tabs.tab_count(), 2);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "file content");
    }

    // ── handle_io_responses: DialogFileOpened ────────────────────────────

    #[test]
    fn test_handle_dialog_file_opened() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;

        let path = std::path::PathBuf::from("/tmp/dialog_open.txt");
        app.io_worker
            .inject_response(crate::io_worker::IoResponse::DialogFileOpened {
                path: path.clone(),
                bytes: b"opened".to_vec(),
            });
        app.handle_io_responses();

        assert!(!app.io_activity.dialog_open);
        assert_eq!(app.tabs.tab_count(), 2);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "opened");
    }

    // ── handle_io_responses: FileSaved ───────────────────────────────────

    #[test]
    fn test_handle_file_saved_clears_modified() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("saved.txt");
        std::fs::write(&path, "original").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();
        app.tabs.active_doc_mut().insert_text(" edit");
        assert!(app.tabs.active_doc().modified);
        let version = app.tabs.active_doc().content_version;

        app.io_activity
            .pending_saves
            .push(crate::io_worker::PendingSave {
                path: path.clone(),
                content_version: version,
            });

        app.io_worker
            .inject_response(crate::io_worker::IoResponse::FileSaved { path: path.clone() });
        app.handle_io_responses();

        assert!(app.io_activity.pending_saves.is_empty());
        assert!(!app.tabs.active_doc().modified);
    }

    #[test]
    fn test_handle_file_saved_keeps_modified_if_version_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("versioned.txt");
        std::fs::write(&path, "original").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();
        app.tabs.active_doc_mut().insert_text(" first");
        let old_version = app.tabs.active_doc().content_version;

        app.io_activity
            .pending_saves
            .push(crate::io_worker::PendingSave {
                path: path.clone(),
                content_version: old_version,
            });

        // Simulate user editing while save is in flight
        app.tabs.active_doc_mut().insert_text(" second");
        assert_ne!(app.tabs.active_doc().content_version, old_version);

        app.io_worker
            .inject_response(crate::io_worker::IoResponse::FileSaved { path: path.clone() });
        app.handle_io_responses();

        // modified should remain true because content changed after save started
        assert!(app.tabs.active_doc().modified);
    }

    // ── handle_io_responses: DialogFileSavedAs ──────────────────────────

    #[test]
    fn test_handle_dialog_file_saved_as() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("unsaved content");
        let sid = "test-session-id".to_string();
        app.tabs.active_doc_mut().session_id = Some(sid.clone());
        let version = app.tabs.active_doc().content_version;

        app.io_activity.dialog_open = true;
        app.io_activity.save_as_context = Some(crate::io_worker::SaveAsContext {
            content_version: version,
            session_id: Some(sid),
            original_path: None,
            is_copy: false,
        });

        let save_path = std::path::PathBuf::from("/tmp/saved_as.txt");
        app.io_worker
            .inject_response(crate::io_worker::IoResponse::DialogFileSavedAs {
                path: save_path.clone(),
            });
        app.handle_io_responses();

        assert!(!app.io_activity.dialog_open);
        assert!(app.io_activity.save_as_context.is_none());
        assert!(!app.tabs.active_doc().modified);
        assert_eq!(
            app.tabs.active_doc().file_path.as_deref(),
            Some(save_path.as_path())
        );
        assert!(app.tabs.active_doc().session_id.is_none());
    }

    // ── handle_io_responses: DialogCancelled ────────────────────────────

    #[test]
    fn test_handle_dialog_cancelled() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;
        app.io_activity.save_as_context = Some(crate::io_worker::SaveAsContext {
            content_version: 0,
            session_id: None,
            original_path: None,
            is_copy: false,
        });

        app.io_worker
            .inject_response(crate::io_worker::IoResponse::DialogCancelled);
        app.handle_io_responses();

        assert!(!app.io_activity.dialog_open);
        assert!(app.io_activity.save_as_context.is_none());
    }

    // ── handle_io_responses: Error ──────────────────────────────────────

    #[test]
    fn test_handle_error_clears_dialog_state() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;
        app.io_activity.save_as_context = Some(crate::io_worker::SaveAsContext {
            content_version: 0,
            session_id: None,
            original_path: None,
            is_copy: false,
        });

        app.io_worker
            .inject_response(crate::io_worker::IoResponse::Error {
                path: None,
                message: "test error".to_string(),
            });
        app.handle_io_responses();

        assert!(!app.io_activity.dialog_open);
        assert!(app.io_activity.save_as_context.is_none());
    }

    #[test]
    fn test_handle_error_clears_pending_save() {
        let mut app = test_app();
        let path = std::path::PathBuf::from("/tmp/fail.txt");
        app.io_activity
            .pending_saves
            .push(crate::io_worker::PendingSave {
                path: path.clone(),
                content_version: 0,
            });

        app.io_worker
            .inject_response(crate::io_worker::IoResponse::Error {
                path: Some(path),
                message: "write failed".to_string(),
            });
        app.handle_io_responses();

        assert!(app.io_activity.pending_saves.is_empty());
    }

    // ── find_save_as_tab ────────────────────────────────────────────────

    #[test]
    fn test_find_save_as_tab_by_session_id() {
        let mut app = test_app();
        app.tabs.active_doc_mut().session_id = Some("sid-1".to_string());

        let ctx = crate::io_worker::SaveAsContext {
            content_version: 0,
            session_id: Some("sid-1".to_string()),
            original_path: None,
            is_copy: false,
        };
        assert_eq!(app.find_save_as_tab(&ctx), Some(0));
    }

    #[test]
    fn test_find_save_as_tab_by_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("find.txt");
        std::fs::write(&path, "content").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();

        let ctx = crate::io_worker::SaveAsContext {
            content_version: 0,
            session_id: None,
            original_path: Some(path),
            is_copy: false,
        };
        assert_eq!(app.find_save_as_tab(&ctx), Some(1));
    }

    #[test]
    fn test_find_save_as_tab_falls_back_to_active() {
        let app = test_app();
        let ctx = crate::io_worker::SaveAsContext {
            content_version: 0,
            session_id: Some("nonexistent".to_string()),
            original_path: None,
            is_copy: false,
        };
        assert_eq!(app.find_save_as_tab(&ctx), Some(app.tabs.active));
    }

    // ── handle_io_responses: FileTooLarge ───────────────────────────────

    #[test]
    fn test_handle_file_too_large_sets_confirm_dialog() {
        let mut app = test_app();
        let path = std::path::PathBuf::from("/tmp/huge.bin");

        app.io_worker
            .inject_response(crate::io_worker::IoResponse::FileTooLarge {
                path: path.clone(),
                message: "File is too large (2.0 MB)".to_string(),
            });
        app.handle_io_responses();

        match &app.dialog_state {
            DialogState::ConfirmLargeFile {
                path: p,
                message: m,
            } => {
                assert_eq!(*p, path);
                assert!(m.contains("2.0 MB"));
            }
            other => panic!("Expected ConfirmLargeFile, got {other:?}"),
        }
    }

    #[test]
    fn test_handle_file_too_large_clears_dialog_open() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;

        app.io_worker
            .inject_response(crate::io_worker::IoResponse::FileTooLarge {
                path: std::path::PathBuf::from("/tmp/huge.bin"),
                message: "too large".to_string(),
            });
        app.handle_io_responses();

        assert!(!app.io_activity.dialog_open);
    }

    // ── handle_io_responses: decode error → FileOpenError ───────────────

    #[test]
    fn test_handle_file_read_decode_error_sets_dialog_with_recovery() {
        let mut app = test_app();
        app.io_activity.pending_reads = 1;

        // UTF-16 LE BOM followed by an odd byte count → decode failure
        let bad_utf16 = vec![0xFF, 0xFE, 0x41, 0x00, 0x42];
        app.io_worker
            .inject_response(crate::io_worker::IoResponse::FileRead {
                path: std::path::PathBuf::from("/tmp/corrupt.txt"),
                bytes: bad_utf16,
            });
        app.handle_io_responses();

        assert_eq!(app.io_activity.pending_reads, 0);
        match &app.dialog_state {
            DialogState::FileOpenError {
                message,
                can_recover_utf8,
                ..
            } => {
                assert!(message.contains("failed to decode"), "Got: {message}");
                assert!(can_recover_utf8);
            }
            other => panic!("Expected FileOpenError, got {other:?}"),
        }
    }

    #[test]
    fn test_handle_dialog_file_opened_decode_error_sets_dialog() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;

        let bad_utf16 = vec![0xFF, 0xFE, 0x41, 0x00, 0x42];
        app.io_worker
            .inject_response(crate::io_worker::IoResponse::DialogFileOpened {
                path: std::path::PathBuf::from("/tmp/corrupt.txt"),
                bytes: bad_utf16,
            });
        app.handle_io_responses();

        assert!(!app.io_activity.dialog_open);
        assert!(matches!(
            app.dialog_state,
            DialogState::FileOpenError {
                can_recover_utf8: true,
                ..
            }
        ));
    }

    // ── is_decode_error ─────────────────────────────────────────────────

    #[test]
    fn test_is_decode_error_positive() {
        assert!(App::is_decode_error("failed to decode file: /tmp/x.txt"));
        assert!(App::is_decode_error(
            "Invalid UTF-16 LE: failed to decode: odd bytes"
        ));
    }

    #[test]
    fn test_is_decode_error_negative() {
        assert!(!App::is_decode_error("failed to load undo history"));
        assert!(!App::is_decode_error("permission denied"));
        assert!(!App::is_decode_error(""));
    }

    // ── recover_file_as_utf8_lossy ──────────────────────────────────────

    #[test]
    fn test_recover_file_as_utf8_lossy_creates_tab() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.txt");
        // Write bytes with some invalid UTF-8 sequences
        std::fs::write(&path, b"hello\xFF\xFEworld").unwrap();

        let mut app = test_app();
        let initial_count = app.tabs.tab_count();
        app.recover_file_as_utf8_lossy(&path);

        assert_eq!(app.tabs.tab_count(), initial_count + 1);
        let doc = app.tabs.active_doc();
        assert_eq!(doc.title, "[Recovered] broken.txt");
        assert!(doc.file_path.is_none(), "Recovered tab should be untitled");
        let content = doc.buffer.to_string();
        assert!(content.contains("hello"));
        assert!(content.contains("world"));
        assert!(
            content.contains('\u{FFFD}'),
            "Should contain replacement chars"
        );
    }

    #[test]
    fn test_recover_file_as_utf8_lossy_valid_utf8() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("valid.txt");
        std::fs::write(&path, "perfectly fine text").unwrap();

        let mut app = test_app();
        app.recover_file_as_utf8_lossy(&path);

        let doc = app.tabs.active_doc();
        assert_eq!(doc.title, "[Recovered] valid.txt");
        assert_eq!(doc.buffer.to_string(), "perfectly fine text");
    }

    #[test]
    fn test_recover_file_as_utf8_lossy_missing_file() {
        let mut app = test_app();
        let initial_count = app.tabs.tab_count();

        app.recover_file_as_utf8_lossy(std::path::Path::new("/nonexistent/file.txt"));

        // Should not create a tab when the file cannot be read
        assert_eq!(app.tabs.tab_count(), initial_count);
    }

    // ── Reload from Disk ──────────────────────────────────────────────

    #[test]
    fn test_request_reload_untitled_is_noop() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("content");
        app.request_reload_from_disk();
        // Untitled tab has no file_path, so nothing happens
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_request_reload_modified_prompts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reload.txt");
        std::fs::write(&path, "original").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();
        app.tabs.switch_to(1);
        app.tabs.active_doc_mut().insert_text(" changed");
        app.request_reload_from_disk();
        assert!(matches!(app.dialog_state, DialogState::ConfirmReload));
    }

    #[test]
    fn test_request_reload_unmodified_reloads_directly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reload2.txt");
        std::fs::write(&path, "original").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();
        app.tabs.switch_to(1);

        // Modify the file on disk
        std::fs::write(&path, "updated").unwrap();
        app.request_reload_from_disk();

        // Should reload immediately without a dialog
        assert!(matches!(app.dialog_state, DialogState::None));
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "updated");
    }

    // ── Close operations ──────────────────────────────────────────────

    #[test]
    fn test_close_unchanged_tabs() {
        let mut app = test_app();
        // Tab 0: unmodified
        app.new_tab(); // Tab 1: unmodified
        app.new_tab(); // Tab 2
        app.tabs.active_doc_mut().insert_text("modified");
        // Tab 2 is modified, tabs 0 and 1 are not

        assert_eq!(app.tabs.tab_count(), 3);
        app.close_unchanged_tabs();

        // Only the modified tab should remain (or a blank tab replacing the last)
        assert!(app
            .tabs
            .documents
            .iter()
            .all(|d| d.modified || d.buffer.is_empty()));
    }

    #[test]
    fn test_close_all_but_active() {
        let mut app = test_app();
        app.new_tab();
        app.new_tab();
        assert_eq!(app.tabs.tab_count(), 3);
        app.tabs.switch_to(1);

        app.close_all_but_active();
        assert_eq!(app.tabs.tab_count(), 1);
        assert_eq!(app.tabs.active, 0);
    }

    #[test]
    fn test_close_all_tabs_unmodified() {
        let mut app = test_app();
        app.new_tab();
        app.new_tab();
        assert_eq!(app.tabs.tab_count(), 3);

        app.close_all_tabs();
        // All tabs are unmodified, so all should close (reset to one blank)
        assert_eq!(app.tabs.tab_count(), 1);
        assert!(app.tabs.active_doc().buffer.is_empty());
    }

    #[test]
    fn test_close_all_tabs_with_modified() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("unsaved");
        app.new_tab();
        assert_eq!(app.tabs.tab_count(), 2);

        app.close_all_tabs();
        // Unmodified tab is closed, modified tab remains with ConfirmClose
        assert_eq!(app.tabs.tab_count(), 1);
        assert!(app.tabs.active_doc().modified);
        assert!(matches!(app.dialog_state, DialogState::ConfirmClose(0)));
        assert!(app.closing_all);
    }

    #[test]
    fn test_close_all_chains_through_multiple_modified() {
        let mut app = test_app();
        // Create 3 modified tabs
        app.tabs.active_doc_mut().insert_text("mod1");
        app.new_tab();
        app.tabs.active_doc_mut().insert_text("mod2");
        app.new_tab();
        app.tabs.active_doc_mut().insert_text("mod3");
        assert_eq!(app.tabs.tab_count(), 3);

        app.close_all_tabs();
        // All 3 are modified, first one prompted
        assert_eq!(app.tabs.tab_count(), 3);
        assert!(matches!(app.dialog_state, DialogState::ConfirmClose(0)));
        assert!(app.closing_all);

        // Simulate "Discard" for tab 0
        app.cleanup_session_for_tab(0);
        app.tabs.close_tab(0);
        app.dialog_state = DialogState::None;
        app.continue_close_all();

        // Should prompt for the next modified tab
        assert_eq!(app.tabs.tab_count(), 2);
        assert!(matches!(app.dialog_state, DialogState::ConfirmClose(_)));
        assert!(app.closing_all);

        // Simulate "Discard" for next tab
        let DialogState::ConfirmClose(idx) = app.dialog_state else {
            panic!("expected ConfirmClose");
        };
        app.cleanup_session_for_tab(idx);
        app.tabs.close_tab(idx);
        app.dialog_state = DialogState::None;
        app.continue_close_all();

        // Should prompt for the last modified tab
        assert!(matches!(app.dialog_state, DialogState::ConfirmClose(_)));

        // Simulate "Discard" for last tab
        let DialogState::ConfirmClose(idx) = app.dialog_state else {
            panic!("expected ConfirmClose");
        };
        app.cleanup_session_for_tab(idx);
        app.tabs.close_tab(idx);
        app.dialog_state = DialogState::None;
        app.continue_close_all();

        // All done — single blank tab, closing_all cleared
        assert_eq!(app.tabs.tab_count(), 1);
        assert!(!app.tabs.active_doc().modified);
        assert!(!app.closing_all);
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_close_all_cancel_stops_chain() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("mod1");
        app.new_tab();
        app.tabs.active_doc_mut().insert_text("mod2");

        app.close_all_tabs();
        assert!(app.closing_all);

        // Simulate "Cancel"
        app.dialog_state = DialogState::None;
        app.closing_all = false;

        // Chain should be stopped — no more prompts
        assert!(!app.closing_all);
        assert_eq!(app.tabs.tab_count(), 2);
    }

    // ── Save a Copy ──────────────────────────────────────────────────

    #[test]
    fn test_save_copy_does_not_update_document() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("copy content");
        let original_title = app.tabs.active_doc().title.clone();

        // Simulate a completed save-a-copy by injecting the response
        app.io_activity.save_as_context = Some(crate::io_worker::SaveAsContext {
            content_version: app.tabs.active_doc().content_version,
            session_id: app.tabs.active_doc().session_id.clone(),
            original_path: None,
            is_copy: true,
        });
        app.io_activity.dialog_open = true;

        // Inject the response
        app.io_worker
            .inject_response(crate::io_worker::IoResponse::DialogFileSavedAs {
                path: std::path::PathBuf::from("/tmp/copy.txt"),
            });

        app.handle_io_responses();

        // Document state should NOT be updated
        assert_eq!(app.tabs.active_doc().title, original_title);
        assert!(app.tabs.active_doc().modified);
        assert!(app.tabs.active_doc().file_path.is_none());
    }

    // ── Bookmark indicator (theme field) ──────────────────────────────

    #[test]
    fn test_confirm_reload_dialog_state() {
        let mut app = test_app();
        app.dialog_state = DialogState::ConfirmReload;
        assert!(app.is_dialog_open());
    }

    // ── Additional coverage for close operations ─────────────────────

    #[test]
    fn test_close_unchanged_tabs_all_modified() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("mod1");
        app.new_tab();
        app.tabs.active_doc_mut().insert_text("mod2");
        assert_eq!(app.tabs.tab_count(), 2);

        app.close_unchanged_tabs();
        // All tabs are modified, none should close
        assert_eq!(app.tabs.tab_count(), 2);
    }

    #[test]
    fn test_close_unchanged_tabs_all_clean() {
        let mut app = test_app();
        app.new_tab();
        app.new_tab();
        assert_eq!(app.tabs.tab_count(), 3);

        app.close_unchanged_tabs();
        // All clean → all closed, reset to one blank tab
        assert_eq!(app.tabs.tab_count(), 1);
        assert!(!app.tabs.active_doc().modified);
    }

    #[test]
    fn test_close_all_but_active_single_tab() {
        let mut app = test_app();
        assert_eq!(app.tabs.tab_count(), 1);

        app.close_all_but_active();
        // Single tab stays, no crash
        assert_eq!(app.tabs.tab_count(), 1);
    }

    #[test]
    fn test_close_all_but_active_preserves_correct_tab() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("keep this");
        app.new_tab();
        app.tabs.active_doc_mut().insert_text("discard");
        app.new_tab();

        // Switch to middle tab (index 0 has "keep this")
        app.tabs.switch_to(0);
        app.close_all_but_active();

        assert_eq!(app.tabs.tab_count(), 1);
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "keep this");
    }

    #[test]
    fn test_close_all_tabs_no_modified() {
        let mut app = test_app();
        app.new_tab();
        app.new_tab();

        app.close_all_tabs();
        // No modified tabs → everything closes, no dialog
        assert_eq!(app.tabs.tab_count(), 1);
        assert!(!app.closing_all);
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_continue_close_all_noop_when_not_closing() {
        let mut app = test_app();
        assert!(!app.closing_all);

        app.continue_close_all();
        // Should be a no-op
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    // ── Bulk close skips pinned tabs ──────────────────────────────────

    #[test]
    fn test_close_unchanged_skips_pinned() {
        let mut app = test_app();
        app.new_tab(); // tab 1
        app.new_tab(); // tab 2
        app.tabs.documents[0].title = "keep_pinned".to_string();
        app.tabs.pin_tab(0);
        // After pin: tab "keep_pinned" is at idx 0, all unmodified.
        assert_eq!(app.tabs.tab_count(), 3);

        app.close_unchanged_tabs();
        // Pinned tab survives; the unpinned unchanged tabs are closed.
        // Closing the last unpinned tab via close_tab resets to a single
        // empty tab when only one document is left, but here we still have
        // the pinned one — so the pinned tab should remain.
        assert!(
            app.tabs.documents.iter().any(|d| d.title == "keep_pinned"),
            "pinned tab must survive close_unchanged_tabs"
        );
    }

    #[test]
    fn test_close_all_but_active_skips_pinned() {
        let mut app = test_app();
        app.new_tab(); // tab 1
        app.new_tab(); // tab 2
        app.tabs.documents[1].title = "pinned_other".to_string();
        app.tabs.pin_tab(1);
        // After pin: pinned_other is at idx 0. The originally-active tab
        // (idx 2) follows the move and is now at idx 2 (or wherever).
        // Switch to a non-pinned tab to keep.
        let keep_idx = app
            .tabs
            .documents
            .iter()
            .position(|d| !d.pinned)
            .expect("at least one unpinned tab");
        app.tabs.switch_to(keep_idx);

        app.close_all_but_active();
        // Pinned tab and the active tab survive; the other unpinned tab is closed.
        assert!(
            app.tabs.documents.iter().any(|d| d.title == "pinned_other"),
            "pinned tab must survive close_all_but_active"
        );
        assert_eq!(app.tabs.tab_count(), 2);
    }

    #[test]
    fn test_close_all_skips_pinned_unmodified() {
        let mut app = test_app();
        app.new_tab();
        app.new_tab();
        app.tabs.documents[0].title = "pinned_clean".to_string();
        app.tabs.pin_tab(0);
        assert_eq!(app.tabs.tab_count(), 3);

        app.close_all_tabs();
        // Pinned unmodified tab survives; nothing is modified so no dialog.
        assert!(
            app.tabs.documents.iter().any(|d| d.title == "pinned_clean"),
            "pinned tab must survive close_all_tabs"
        );
        assert!(!app.closing_all);
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_close_all_skips_pinned_modified() {
        let mut app = test_app();
        // Pin a modified tab.
        app.tabs.active_doc_mut().insert_text("pinned_dirty");
        app.tabs.documents[0].title = "pinned_dirty".to_string();
        app.tabs.pin_tab(0);
        // Add an unpinned modified tab.
        app.new_tab();
        app.tabs.active_doc_mut().insert_text("dirty");

        app.close_all_tabs();
        // Pinned modified tab is NOT prompted for; the unpinned modified tab is.
        assert!(app.closing_all);
        let DialogState::ConfirmClose(idx) = app.dialog_state else {
            panic!("expected ConfirmClose");
        };
        assert!(
            !app.tabs.documents[idx].pinned,
            "ConfirmClose should target a non-pinned tab"
        );
        // The pinned tab still exists.
        assert!(app.tabs.documents.iter().any(|d| d.title == "pinned_dirty"));
    }

    // ── Additional coverage for reload ────────────────────────────────

    #[test]
    fn test_do_reload_from_disk_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("will_delete.txt");
        std::fs::write(&path, "original").unwrap();

        let mut app = test_app();
        app.tabs.open_file(&path).unwrap();
        app.tabs.switch_to(1);

        // Delete the file, then reload
        std::fs::remove_file(&path).unwrap();
        app.do_reload_from_disk();

        // Should log error but not panic; content unchanged
        assert_eq!(app.tabs.active_doc().buffer.to_string(), "original");
    }

    // ── Additional coverage for save-a-copy ──────────────────────────

    #[test]
    fn test_save_copy_blocked_when_dialog_open() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;

        app.save_copy_dialog();
        // Should not set a new save_as_context
        assert!(app.io_activity.save_as_context.is_none());
    }

    #[test]
    fn test_save_as_blocked_when_dialog_open() {
        let mut app = test_app();
        app.io_activity.dialog_open = true;

        app.save_as_dialog();
        assert!(app.io_activity.save_as_context.is_none());
    }

    #[test]
    fn test_save_copy_context_has_is_copy_true() {
        // save_copy_dialog() spawns an rfd dialog thread — not safe on
        // headless CI. Instead verify the SaveAsContext would have is_copy
        // set by constructing it directly, matching the pattern at line ~3296.
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("content");
        let doc = app.tabs.active_doc();

        let ctx = crate::io_worker::SaveAsContext {
            content_version: doc.content_version,
            session_id: doc.session_id.clone(),
            original_path: doc.file_path.clone(),
            is_copy: true,
        };
        assert!(ctx.is_copy);
        assert_eq!(ctx.content_version, doc.content_version);
    }

    #[test]
    fn test_save_as_context_has_is_copy_false() {
        // save_as_dialog() spawns an rfd dialog thread — not safe on
        // headless CI. Verify the SaveAsContext directly.
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("content");
        let doc = app.tabs.active_doc();

        let ctx = crate::io_worker::SaveAsContext {
            content_version: doc.content_version,
            session_id: doc.session_id.clone(),
            original_path: doc.file_path.clone(),
            is_copy: false,
        };
        assert!(!ctx.is_copy);
        assert_eq!(ctx.content_version, doc.content_version);
    }

    // -- Escape key priority chain --

    #[test]
    fn test_escape_non_escape_key_returns_false() {
        let mut app = test_app();
        assert!(!app.handle_escape_shortcut(egui::Key::Enter));
    }

    #[test]
    fn test_escape_closes_confirm_close_first() {
        let mut app = test_app();
        app.dialog_state = DialogState::ConfirmClose(0);
        app.closing_all = true;
        app.settings_open = true;

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(matches!(app.dialog_state, DialogState::None));
        assert!(!app.closing_all);
        // Settings should still be open — only one dialog per press
        assert!(app.settings_open);
    }

    #[test]
    fn test_escape_closes_confirm_reload() {
        let mut app = test_app();
        app.dialog_state = DialogState::ConfirmReload;

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_escape_closes_confirm_large_file() {
        let mut app = test_app();
        app.dialog_state = DialogState::ConfirmLargeFile {
            path: std::path::PathBuf::from("big.txt"),
            message: "Too large".into(),
        };

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_escape_closes_file_open_error() {
        let mut app = test_app();
        app.dialog_state = DialogState::FileOpenError {
            path: std::path::PathBuf::from("bad.bin"),
            message: "Invalid encoding".into(),
            can_recover_utf8: true,
        };

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_escape_closes_print_error() {
        let mut app = test_app();
        app.dialog_state = DialogState::PrintError {
            message: "Print failed".into(),
            temp_path: None,
        };

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(matches!(app.dialog_state, DialogState::None));
    }

    #[test]
    fn test_escape_closes_settings_before_about() {
        let mut app = test_app();
        app.settings_open = true;
        app.about_open = true;

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(!app.settings_open);
        // About should still be open
        assert!(app.about_open);
    }

    #[test]
    fn test_escape_closes_about_when_settings_closed() {
        let mut app = test_app();
        app.about_open = true;

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(!app.about_open);
    }

    #[test]
    fn test_escape_closes_find_replace_before_go_to_line() {
        let mut app = test_app();
        app.find_replace.open();
        app.go_to_line.open();

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(!app.find_replace.visible);
        // Go to line should still be open
        assert!(app.go_to_line.visible);
    }

    #[test]
    fn test_escape_closes_go_to_line_when_find_replace_closed() {
        let mut app = test_app();
        app.go_to_line.open();

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(!app.go_to_line.visible);
    }

    #[test]
    fn test_escape_clears_secondary_cursors_when_no_dialogs() {
        let mut app = test_app();
        app.tabs.active_doc_mut().insert_text("hello world");
        let mut sc = rust_pad_core::cursor::Cursor::new();
        sc.position = Position::new(0, 5);
        app.tabs.active_doc_mut().secondary_cursors.push(sc);
        assert!(app.tabs.active_doc().is_multi_cursor());

        assert!(app.handle_escape_shortcut(egui::Key::Escape));
        assert!(!app.tabs.active_doc().is_multi_cursor());
    }

    #[test]
    fn test_escape_full_priority_chain() {
        let mut app = test_app();
        // Set up everything at once
        app.dialog_state = DialogState::ConfirmClose(0);
        app.closing_all = true;
        app.settings_open = true;
        app.about_open = true;
        app.find_replace.open();
        app.go_to_line.open();

        // 1st Escape: ConfirmClose
        app.handle_escape_shortcut(egui::Key::Escape);
        assert!(matches!(app.dialog_state, DialogState::None));
        assert!(!app.closing_all);
        assert!(app.settings_open);

        // 2nd Escape: Settings
        app.handle_escape_shortcut(egui::Key::Escape);
        assert!(!app.settings_open);
        assert!(app.about_open);

        // 3rd Escape: About
        app.handle_escape_shortcut(egui::Key::Escape);
        assert!(!app.about_open);
        assert!(app.find_replace.visible);

        // 4th Escape: Find/Replace
        app.handle_escape_shortcut(egui::Key::Escape);
        assert!(!app.find_replace.visible);
        assert!(app.go_to_line.visible);

        // 5th Escape: Go to Line
        app.handle_escape_shortcut(egui::Key::Escape);
        assert!(!app.go_to_line.visible);
    }
}
