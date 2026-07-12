/// Application configuration: load, save, merge, and sanitize.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::{all_builtin_themes, ThemeDefinition};

/// When to remove dead (non-existent) files from the recent files list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RecentFilesCleanup {
    #[default]
    OnStartup,
    OnMenuOpen,
    Both,
}

/// Persisted window geometry, captured on exit and restored at startup.
///
/// The monitor size at save time is recorded so the restore path can tell
/// whether the saved position still lands on a visible screen.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometry {
    /// Outer window position; negative values are legitimate on multi-monitor
    /// setups and must not be clamped away.
    pub x: f32,
    pub y: f32,
    pub inner_w: f32,
    pub inner_h: f32,
    pub maximized: bool,
    pub monitor_w: f32,
    pub monitor_h: f32,
}

/// Height of the band along a window's top edge that must be visible for a
/// saved position to be restored (the strip the user can grab to move the
/// window back).
pub const TITLE_STRIP_HEIGHT: f32 = 30.0;

/// Smallest window size ever restored, regardless of what the config says.
pub const MIN_RESTORED_INNER: (f32, f32) = (800.0, 600.0);

impl WindowGeometry {
    /// Returns `true` when every field is finite and sizes are positive,
    /// the precondition for using this geometry at restore time.
    pub fn is_plausible(&self) -> bool {
        let finite = [
            self.x,
            self.y,
            self.inner_w,
            self.inner_h,
            self.monitor_w,
            self.monitor_h,
        ]
        .iter()
        .all(|v| v.is_finite());
        finite
            && self.inner_w > 0.0
            && self.inner_h > 0.0
            && self.monitor_w > 0.0
            && self.monitor_h > 0.0
    }

    /// The inner size to restore: clamped at both ends to
    /// [`MIN_RESTORED_INNER`] ..= the saved monitor size (hand-edited JSON
    /// can carry absurd values in either direction). The `bool` reports
    /// whether clamping changed anything, so the caller can `warn!`.
    pub fn restore_inner_size(&self) -> (f32, f32, bool) {
        let (min_w, min_h) = MIN_RESTORED_INNER;
        // A monitor smaller than the minimum still gets the minimum: a
        // too-large window beats an unusably small one.
        let w = self.inner_w.clamp(min_w, self.monitor_w.max(min_w));
        let h = self.inner_h.clamp(min_h, self.monitor_h.max(min_h));
        let clamped =
            (w - self.inner_w).abs() > f32::EPSILON || (h - self.inner_h).abs() > f32::EPSILON;
        (w, h, clamped)
    }

    /// The outer position to restore, or `None` when the saved title-bar
    /// strip does not intersect the monitor recorded at save time.
    ///
    /// Negative coordinates are legitimate multi-monitor positions and are
    /// never zero-clamped; the visibility test against the recorded monitor
    /// is the only gate. The strip is the top edge band of the window rect,
    /// so a window whose bottom pixel peeks onto the screen does not pass.
    pub fn restore_position(&self) -> Option<(f32, f32)> {
        let strip_left = self.x;
        let strip_right = self.x + self.inner_w;
        let strip_top = self.y;
        let strip_bottom = self.y + TITLE_STRIP_HEIGHT;
        let visible = strip_left < self.monitor_w
            && strip_right > 0.0
            && strip_top < self.monitor_h
            && strip_bottom > 0.0;
        visible.then_some((self.x, self.y))
    }
}

/// What degraded while loading the config, so the UI layer can tell the user
/// instead of silently running on defaults.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigLoadReport {
    /// The config file existed but could not be parsed; carries the error text.
    pub parse_error: Option<String>,
    /// The broken config file could not be moved aside; saving must be
    /// suppressed so the original is never overwritten.
    pub save_blocked: bool,
    /// A `current_theme` value that was reset to "System" by sanitize.
    pub theme_reset: Option<String>,
}

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub current_theme: String,
    pub current_zoom_level: f32,
    pub max_zoom_level: f32,
    pub word_wrap: bool,
    pub show_special_chars: bool,
    pub show_line_numbers: bool,
    /// Whether the breadcrumb strip above the editor is shown.
    pub show_breadcrumb: bool,
    pub restore_open_files: bool,
    pub show_full_path_in_title: bool,
    pub font_size: f32,
    /// Default file extension for new untitled tabs (e.g. "txt", "md"). Empty = none.
    pub default_extension: String,
    /// Default line ending for new documents. One of "system" (OS default),
    /// "lf" (Unix), or "crlf" (Windows). Loaded files keep their detected
    /// line ending regardless of this setting.
    pub default_line_ending: String,
    /// Whether to remember the last folder used in open/save dialogs.
    pub remember_last_folder: bool,
    /// Default working folder for file dialogs. Empty = user's home directory.
    pub default_work_folder: String,
    /// Last folder used in an open/save dialog (persisted across sessions).
    pub last_used_folder: String,
    /// Whether to auto-save file-backed documents periodically.
    pub auto_save_enabled: bool,
    /// Interval in seconds between auto-saves (minimum 5).
    pub auto_save_interval_secs: u64,
    /// Whether the recent files feature is enabled.
    pub recent_files_enabled: bool,
    /// Maximum number of recent files to remember.
    pub recent_files_max_count: usize,
    /// When to prune dead files from the recent list.
    pub recent_files_cleanup: RecentFilesCleanup,
    /// Most-recently-opened file paths (most recent first).
    pub recent_files: Vec<String>,
    /// Maximum file size in MB that can be opened. Files exceeding this limit
    /// are rejected to prevent out-of-memory crashes. 0 = no limit.
    pub max_file_size_mb: u64,
    /// Threshold in MB above which a "Copy file contents to clipboard" action
    /// prompts the user for confirmation before reading. Files above
    /// `copy_contents_max_mb` are refused outright (no prompt). 0 = always prompt.
    pub copy_contents_warning_mb: u64,
    /// Hard cap in MB for the "Copy file contents to clipboard" action. Files
    /// larger than this are refused outright. This is independent of
    /// `max_file_size_mb` (which limits opening a file in the editor): copying
    /// to the clipboard is a different operation with different memory limits.
    /// 0 = no limit.
    pub copy_contents_max_mb: u64,
    /// Maximum size (in KB) of unsaved tab content to persist in the session store.
    /// 0 = unlimited. Tabs exceeding this limit are saved as metadata only.
    pub session_content_max_kb: usize,
    /// Whether the "Print..." / "Export as PDF..." pipeline renders a
    /// line-number gutter in the generated PDF.
    pub print_show_line_numbers: bool,
    /// Whether synchronized scrolling between split panes is enabled.
    /// Only takes effect when split view is active. Persisted across runs
    /// but treated as off until the user actually splits.
    pub sync_scroll_enabled: bool,
    /// Whether synchronized scrolling mirrors horizontal deltas in addition
    /// to vertical. Has no effect when `sync_scroll_enabled` is false.
    pub sync_scroll_horizontal: bool,
    /// Whether the workspace sidebar was visible in the last session.
    pub workspace_sidebar_visible: bool,
    /// Width of the workspace sidebar in the last session.
    pub workspace_sidebar_width: f32,
    /// Whether hidden files/folders (names starting with `.`) are shown in the workspace tree.
    pub show_hidden_files: bool,
    /// Window position/size from the last session; `None` until first saved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_geometry: Option<WindowGeometry>,
    pub themes: Vec<ThemeDefinition>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            current_theme: "System".to_string(),
            current_zoom_level: 1.0,
            max_zoom_level: 15.0,
            word_wrap: false,
            show_special_chars: false,
            show_line_numbers: true,
            show_breadcrumb: true,
            restore_open_files: true,
            show_full_path_in_title: true,
            font_size: 16.0,
            default_extension: String::new(),
            default_line_ending: "system".to_string(),
            remember_last_folder: true,
            default_work_folder: String::new(),
            last_used_folder: String::new(),
            auto_save_enabled: false,
            auto_save_interval_secs: 30,
            recent_files_enabled: true,
            recent_files_max_count: 10,
            recent_files_cleanup: RecentFilesCleanup::default(),
            recent_files: Vec::new(),
            max_file_size_mb: 512,
            copy_contents_warning_mb: 5,
            copy_contents_max_mb: 64,
            session_content_max_kb: 10_240,
            print_show_line_numbers: true,
            sync_scroll_enabled: false,
            sync_scroll_horizontal: true,
            workspace_sidebar_visible: false,
            workspace_sidebar_width: 250.0,
            show_hidden_files: false,
            window_geometry: None,
            themes: all_builtin_themes(),
        }
    }
}

impl AppConfig {
    /// Returns the config file path in the platform-standard config directory.
    ///
    /// Falls back to the executable directory if the platform config
    /// directory cannot be determined.
    pub fn config_path() -> PathBuf {
        crate::paths::config_file_path()
    }

    /// Loads config from `path`, creating a default file if it doesn't exist.
    /// Returns defaults on any error (missing file, parse error, etc.).
    pub fn load_or_create(path: &std::path::Path) -> Self {
        Self::load_or_create_with_report(path).0
    }

    /// Reads only the persisted window geometry from `path`: a pure read
    /// with none of [`load_or_create_with_report`](Self::load_or_create_with_report)'s
    /// side effects (no file creation, no `.bak` move-aside), for use before
    /// the app owns config loading. Returns `None` on any error or when the
    /// stored geometry is not plausible.
    pub fn peek_window_geometry(path: &std::path::Path) -> Option<WindowGeometry> {
        let text = std::fs::read_to_string(path).ok()?;
        let config: Self = serde_json::from_str(&text).ok()?;
        config.window_geometry.filter(WindowGeometry::is_plausible)
    }

    /// Loads config from `path` like [`load_or_create`](Self::load_or_create),
    /// additionally reporting anything that degraded so the caller can surface
    /// it to the user.
    ///
    /// On a parse failure the broken file is moved aside to `<path>.bak`
    /// (replacing any older backup) so a later save can't destroy it. If that
    /// move fails, the report carries `save_blocked` and the caller must not
    /// save the config for the rest of the session.
    pub fn load_or_create_with_report(path: &std::path::Path) -> (Self, ConfigLoadReport) {
        let mut report = ConfigLoadReport::default();
        if !path.exists() {
            let config = Self::default();
            if let Err(e) = config.save(path) {
                tracing::warn!("Failed to create default config at {}: {e}", path.display());
            }
            return (config, report);
        }

        let parse_result = std::fs::read_to_string(path)
            .map_err(|e| format!("read failed: {e}"))
            .and_then(|contents| {
                serde_json::from_str::<AppConfig>(&contents)
                    .map_err(|e| format!("parse failed: {e}"))
            });

        match parse_result {
            Ok(mut config) => {
                // Merge before sanitizing: a `current_theme` naming a builtin
                // that isn't in the serialized vec yet (e.g. after an upgrade
                // that added themes) must survive the unknown-name reset.
                config.with_builtins_merged();
                report.theme_reset = config.sanitize();
                (config, report)
            }
            Err(e) => {
                tracing::warn!("Failed to load config at {}: {e}", path.display());
                report.parse_error = Some(e);
                report.save_blocked = !Self::backup_broken_file(path);
                let mut config = Self::default();
                config.sanitize();
                (config, report)
            }
        }
    }

    /// Moves an unparsable config file to `<path>.bak`, deleting any stale
    /// backup first (Windows refuses to rename onto an existing file).
    /// Returns `false` when the file could not be moved aside; the caller
    /// must then refuse to save over it.
    fn backup_broken_file(path: &std::path::Path) -> bool {
        let mut backup = path.as_os_str().to_owned();
        backup.push(".bak");
        let backup = PathBuf::from(backup);
        if backup.exists() {
            if let Err(e) = std::fs::remove_file(&backup) {
                tracing::warn!("Failed to remove stale backup {}: {e}", backup.display());
                return false;
            }
        }
        match std::fs::rename(path, &backup) {
            Ok(()) => {
                tracing::warn!(
                    "Moved unparsable config {} to {}",
                    path.display(),
                    backup.display()
                );
                true
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to move unparsable config {} aside: {e}; config saving disabled",
                    path.display()
                );
                false
            }
        }
    }

    /// Saves config to `path` as pretty-printed JSON.
    ///
    /// Creates the parent directory if it does not exist and sets
    /// restrictive permissions on it.
    pub fn save(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)?;
                crate::permissions::set_owner_only_dir_permissions(parent);
            }
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, &json)?;
        crate::permissions::set_owner_only_file_permissions(path);
        Ok(())
    }

    /// Adds any built-in theme that is missing, so configs serialized by
    /// older versions gain themes added since.
    /// User-defined themes with matching names take priority over built-ins.
    /// The deletable "Wacky" sample is intentionally not force-merged.
    pub fn with_builtins_merged(&mut self) {
        let mut insert_at = 0;
        for builtin in all_builtin_themes() {
            if builtin.name == "Wacky" {
                continue;
            }
            if !self.themes.iter().any(|t| t.name == builtin.name) {
                let at = insert_at.min(self.themes.len());
                self.themes.insert(at, builtin);
            }
            insert_at += 1;
        }
    }

    /// Finds a theme by name.
    pub fn find_theme(&self, name: &str) -> Option<&ThemeDefinition> {
        self.themes.iter().find(|t| t.name == name)
    }

    /// Returns all theme names.
    pub fn theme_names(&self) -> Vec<&str> {
        self.themes.iter().map(|t| t.name.as_str()).collect()
    }

    /// Returns the effective starting directory for file dialogs.
    ///
    /// Resolution order:
    /// 1. `last_used_folder` (if `remember_last_folder` is true and the path exists)
    /// 2. `default_work_folder` (if non-empty and the path exists)
    /// 3. User's home directory
    pub fn resolve_work_folder(&self) -> Option<PathBuf> {
        if self.remember_last_folder && !self.last_used_folder.is_empty() {
            let p = PathBuf::from(&self.last_used_folder);
            if p.is_dir() {
                return Some(p);
            }
        }
        if !self.default_work_folder.is_empty() {
            let p = PathBuf::from(&self.default_work_folder);
            if p.is_dir() {
                return Some(p);
            }
        }
        dirs::home_dir()
    }

    /// Returns the max file size in bytes, or `None` if no limit is set.
    pub fn max_file_size_bytes(&self) -> Option<u64> {
        if self.max_file_size_mb == 0 {
            None
        } else {
            Some(self.max_file_size_mb * 1024 * 1024)
        }
    }

    /// Returns the Copy Contents warning threshold in bytes.
    ///
    /// `0` in `copy_contents_warning_mb` means "always prompt": the
    /// caller treats a `0` return value as "every file triggers the
    /// confirmation dialog".
    pub fn copy_contents_warning_bytes(&self) -> u64 {
        self.copy_contents_warning_mb.saturating_mul(1024 * 1024)
    }

    /// Returns the Copy Contents hard cap in bytes, or `None` if no limit is
    /// set (`copy_contents_max_mb == 0`). Independent of the editor open
    /// limit returned by [`max_file_size_bytes`](Self::max_file_size_bytes).
    pub fn copy_contents_max_bytes(&self) -> Option<u64> {
        if self.copy_contents_max_mb == 0 {
            None
        } else {
            Some(self.copy_contents_max_mb * 1024 * 1024)
        }
    }

    /// Clamps values to valid ranges and resets invalid fields.
    ///
    /// Returns the previous `current_theme` when it named an unknown theme
    /// and had to be reset to "System", so callers can tell the user.
    pub fn sanitize(&mut self) -> Option<String> {
        self.max_zoom_level = self.max_zoom_level.max(1.0);
        self.current_zoom_level = self.current_zoom_level.clamp(0.5, self.max_zoom_level);
        self.font_size = self.font_size.clamp(6.0, 72.0);

        let mut theme_reset = None;
        let valid_modes = ["System", "Dark", "Light"];
        // Also allow any custom theme name as a valid mode
        let theme_names: Vec<String> = self.themes.iter().map(|t| t.name.clone()).collect();
        if !valid_modes.contains(&self.current_theme.as_str())
            && !theme_names.contains(&self.current_theme)
        {
            theme_reset = Some(std::mem::replace(
                &mut self.current_theme,
                "System".to_string(),
            ));
        }
        self.auto_save_interval_secs = self.auto_save_interval_secs.max(5);
        self.recent_files_max_count = self.recent_files_max_count.clamp(1, 50);
        self.recent_files.truncate(self.recent_files_max_count);
        // 0 = no limit; otherwise clamp to 1..=10_240 MB (10 GB)
        if self.max_file_size_mb > 0 {
            self.max_file_size_mb = self.max_file_size_mb.clamp(1, 10_240);
        }
        // 0 = unlimited; otherwise clamp to 1..=10_240 MB (10 GB).
        if self.copy_contents_max_mb > 0 {
            self.copy_contents_max_mb = self.copy_contents_max_mb.clamp(1, 10_240);
        }
        // The Copy Contents warning threshold cannot exceed the Copy Contents
        // hard cap; otherwise the user would never see the prompt before
        // hitting the outright refusal. `0` on either side means "no limit /
        // always prompt" and is preserved as-is.
        if self.copy_contents_warning_mb > 0 && self.copy_contents_max_mb > 0 {
            self.copy_contents_warning_mb =
                self.copy_contents_warning_mb.min(self.copy_contents_max_mb);
        }
        // 0 = unlimited; otherwise clamp to 1..=102_400 KB (100 MB)
        if self.session_content_max_kb > 0 {
            self.session_content_max_kb = self.session_content_max_kb.clamp(1, 102_400);
        }
        self.workspace_sidebar_width = self.workspace_sidebar_width.clamp(150.0, 500.0);
        // Geometry with non-finite or non-positive values (hand-edited JSON
        // can encode infinity) is unusable; drop it and let the OS place the
        // window. Position validation happens at restore time.
        if self.window_geometry.is_some_and(|g| !g.is_plausible()) {
            self.window_geometry = None;
        }

        theme_reset
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::builtin_dark;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.current_theme, "System");
        assert!((config.current_zoom_level - 1.0).abs() < f32::EPSILON);
        assert!(!config.word_wrap);
        assert!(!config.show_special_chars);
        assert!(config.restore_open_files);
        assert!((config.font_size - 16.0).abs() < f32::EPSILON);
        assert_eq!(config.themes.len(), 8);
        assert!(config.window_geometry.is_none());
    }

    #[test]
    fn test_sanitize_clamps_zoom() {
        let mut config = AppConfig {
            current_zoom_level: 10.0,
            ..Default::default()
        };
        config.sanitize();
        assert!((config.current_zoom_level - 10.0).abs() < f32::EPSILON);

        config.current_zoom_level = 0.1;
        config.sanitize();
        assert!((config.current_zoom_level - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sanitize_clamps_font_size() {
        let mut config = AppConfig {
            font_size: 2.0,
            ..Default::default()
        };
        config.sanitize();
        assert!((config.font_size - 6.0).abs() < f32::EPSILON);

        config.font_size = 100.0;
        config.sanitize();
        assert!((config.font_size - 72.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sanitize_resets_unknown_theme_mode() {
        let mut config = AppConfig {
            current_theme: "NonExistent".to_string(),
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.current_theme, "System");
    }

    #[test]
    fn test_sanitize_allows_custom_theme_name() {
        let mut config = AppConfig {
            current_theme: "Wacky".to_string(),
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.current_theme, "Wacky");
    }

    #[test]
    fn test_find_theme() {
        let config = AppConfig::default();
        assert!(config.find_theme("Dark").is_some());
        assert!(config.find_theme("Light").is_some());
        assert!(config.find_theme("Wacky").is_some());
        assert!(config.find_theme("NonExistent").is_none());
    }

    #[test]
    fn test_theme_names() {
        let config = AppConfig::default();
        let names = config.theme_names();
        assert_eq!(
            names,
            vec![
                "Aurora Dark",
                "Aurora Light",
                "Graphite Dark",
                "Graphite Light",
                "Dark",
                "Light",
                "Dusk",
                "Wacky"
            ]
        );
    }

    #[test]
    fn test_with_builtins_merged_adds_missing() {
        let mut config = AppConfig {
            themes: vec![crate::theme::sample_wacky()],
            ..Default::default()
        };
        config.with_builtins_merged();
        for name in [
            "Aurora Dark",
            "Aurora Light",
            "Graphite Dark",
            "Graphite Light",
            "Dark",
            "Light",
            "Dusk",
            "Wacky",
        ] {
            assert!(config.find_theme(name).is_some(), "missing {name}");
        }
    }

    #[test]
    fn test_with_builtins_merged_reaches_configs_from_older_versions() {
        // Simulates a user whose serialized themes vec predates the Aurora and
        // Graphite themes: after a merge they appear, in builtin order.
        let mut config = AppConfig {
            themes: vec![
                builtin_dark(),
                crate::theme::builtin_light(),
                crate::theme::builtin_dusk(),
                crate::theme::sample_wacky(),
            ],
            ..Default::default()
        };
        config.with_builtins_merged();
        assert_eq!(config.themes.len(), 8);
        assert_eq!(config.themes[0].name, "Aurora Dark");
        assert_eq!(config.themes[3].name, "Graphite Light");
    }

    #[test]
    fn test_with_builtins_merged_does_not_resurrect_deleted_wacky() {
        let mut config = AppConfig {
            themes: vec![builtin_dark()],
            ..Default::default()
        };
        config.with_builtins_merged();
        assert!(config.find_theme("Wacky").is_none());
    }

    #[test]
    fn test_load_merges_before_sanitize_so_new_builtin_selection_survives() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let old_config = AppConfig {
            current_theme: "Aurora Dark".to_string(),
            themes: vec![builtin_dark()],
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&old_config).unwrap()).unwrap();

        let (loaded, report) = AppConfig::load_or_create_with_report(&path);

        assert_eq!(loaded.current_theme, "Aurora Dark");
        assert_eq!(report, ConfigLoadReport::default());
    }

    #[test]
    fn test_load_reports_theme_reset_for_unknown_name() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let old_config = AppConfig {
            current_theme: "Gone".to_string(),
            ..Default::default()
        };
        std::fs::write(&path, serde_json::to_string(&old_config).unwrap()).unwrap();

        let (loaded, report) = AppConfig::load_or_create_with_report(&path);

        assert_eq!(loaded.current_theme, "System");
        assert_eq!(report.theme_reset.as_deref(), Some("Gone"));
        assert!(report.parse_error.is_none());
    }

    #[test]
    fn test_load_parse_failure_moves_file_to_bak_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{ not json").unwrap();

        let (loaded, report) = AppConfig::load_or_create_with_report(&path);

        assert_eq!(loaded.current_theme, "System");
        assert!(report.parse_error.is_some());
        assert!(!report.save_blocked);
        assert!(!path.exists(), "broken original must be moved aside");
        assert!(dir.path().join("config.json.bak").exists());
    }

    #[test]
    fn test_load_parse_failure_replaces_stale_bak() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let bak = dir.path().join("config.json.bak");
        std::fs::write(&path, "{ fresh breakage").unwrap();
        std::fs::write(&bak, "old backup").unwrap();

        let (_, report) = AppConfig::load_or_create_with_report(&path);

        assert!(!report.save_blocked);
        assert_eq!(
            std::fs::read_to_string(&bak).unwrap(),
            "{ fresh breakage",
            "the newer broken file replaces the stale backup"
        );
    }

    #[test]
    fn test_with_builtins_merged_preserves_custom() {
        let mut custom_dark = builtin_dark();
        custom_dark.editor.bg_color = crate::HexColor::rgb(255, 0, 0);

        let mut config = AppConfig {
            themes: vec![custom_dark.clone()],
            ..Default::default()
        };
        config.with_builtins_merged();

        let dark = config.find_theme("Dark").unwrap();
        assert_eq!(dark.editor.bg_color, crate::HexColor::rgb(255, 0, 0));
    }

    #[test]
    fn test_serde_round_trip() {
        let config = AppConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.current_theme, config.current_theme);
        assert!((parsed.current_zoom_level - config.current_zoom_level).abs() < f32::EPSILON);
        assert_eq!(parsed.themes.len(), config.themes.len());
    }

    // ── Auto-save configuration tests ───────────────────────────────

    #[test]
    fn test_auto_save_defaults() {
        let config = AppConfig::default();
        assert!(!config.auto_save_enabled);
        assert_eq!(config.auto_save_interval_secs, 30);
    }

    #[test]
    fn test_sanitize_clamps_auto_save_interval_minimum() {
        let mut config = AppConfig {
            auto_save_interval_secs: 1,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 5);
    }

    #[test]
    fn test_sanitize_preserves_valid_auto_save_interval() {
        let mut config = AppConfig {
            auto_save_interval_secs: 60,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 60);
    }

    #[test]
    fn test_sanitize_clamps_auto_save_interval_zero() {
        let mut config = AppConfig {
            auto_save_interval_secs: 0,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 5);
    }

    #[test]
    fn test_sanitize_auto_save_interval_boundary() {
        let mut config = AppConfig {
            auto_save_interval_secs: 5,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 5);
    }

    #[test]
    fn test_auto_save_serde_round_trip() {
        let config = AppConfig {
            auto_save_enabled: true,
            auto_save_interval_secs: 45,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.auto_save_enabled);
        assert_eq!(parsed.auto_save_interval_secs, 45);
    }

    #[test]
    fn test_auto_save_missing_fields_get_defaults() {
        // Simulates loading a config file that predates auto-save feature
        let json = r#"{"current_theme": "Dark"}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!parsed.auto_save_enabled);
        assert_eq!(parsed.auto_save_interval_secs, 30);
    }

    // ── Recent files configuration tests ────────────────────────────

    #[test]
    fn test_recent_files_defaults() {
        let config = AppConfig::default();
        assert!(config.recent_files_enabled);
        assert_eq!(config.recent_files_max_count, 10);
        assert_eq!(config.recent_files_cleanup, RecentFilesCleanup::OnStartup);
        assert!(config.recent_files.is_empty());
    }

    #[test]
    fn test_recent_files_serde_round_trip() {
        let config = AppConfig {
            recent_files_enabled: false,
            recent_files_max_count: 25,
            recent_files_cleanup: RecentFilesCleanup::Both,
            recent_files: vec!["/tmp/a.txt".to_string(), "/tmp/b.rs".to_string()],
            ..Default::default()
        };

        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();

        assert!(!parsed.recent_files_enabled);
        assert_eq!(parsed.recent_files_max_count, 25);
        assert_eq!(parsed.recent_files_cleanup, RecentFilesCleanup::Both);
        assert_eq!(parsed.recent_files.len(), 2);
    }

    #[test]
    fn test_sanitize_clamps_recent_files_max_count() {
        let mut config = AppConfig {
            recent_files_max_count: 0,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.recent_files_max_count, 1);

        config.recent_files_max_count = 100;
        config.sanitize();
        assert_eq!(config.recent_files_max_count, 50);
    }

    #[test]
    fn test_sanitize_truncates_recent_files() {
        let mut config = AppConfig {
            recent_files_max_count: 3,
            recent_files: vec![
                "a.txt".to_string(),
                "b.txt".to_string(),
                "c.txt".to_string(),
                "d.txt".to_string(),
                "e.txt".to_string(),
            ],
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.recent_files.len(), 3);
    }

    #[test]
    fn test_recent_files_missing_fields_get_defaults() {
        let json = r#"{"current_theme": "Dark"}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert!(parsed.recent_files_enabled);
        assert_eq!(parsed.recent_files_max_count, 10);
        assert_eq!(parsed.recent_files_cleanup, RecentFilesCleanup::OnStartup);
        assert!(parsed.recent_files.is_empty());
    }

    // ── Session content max KB tests ───────────────────────────────

    #[test]
    fn test_session_content_max_kb_default() {
        let config = AppConfig::default();
        assert_eq!(config.session_content_max_kb, 10_240);
    }

    #[test]
    fn test_session_content_max_kb_serde_round_trip() {
        let config = AppConfig {
            session_content_max_kb: 5_000,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.session_content_max_kb, 5_000);
    }

    #[test]
    fn test_session_content_max_kb_missing_field_gets_default() {
        let json = r#"{"current_theme": "Dark"}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.session_content_max_kb, 10_240);
    }

    #[test]
    fn test_sanitize_session_content_max_kb_zero_is_unlimited() {
        let mut config = AppConfig {
            session_content_max_kb: 0,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.session_content_max_kb, 0);
    }

    #[test]
    fn test_sanitize_clamps_session_content_max_kb_upper() {
        let mut config = AppConfig {
            session_content_max_kb: 200_000,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.session_content_max_kb, 102_400);
    }

    #[test]
    fn test_sanitize_preserves_valid_session_content_max_kb() {
        let mut config = AppConfig {
            session_content_max_kb: 2_048,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.session_content_max_kb, 2_048);
    }

    // ── File size limit tests ─────────────────────────────────────

    #[test]
    fn test_max_file_size_mb_default() {
        let config = AppConfig::default();
        assert_eq!(config.max_file_size_mb, 512);
    }

    #[test]
    fn test_max_file_size_bytes_conversion() {
        let config = AppConfig::default();
        assert_eq!(config.max_file_size_bytes(), Some(512 * 1024 * 1024));
    }

    #[test]
    fn test_max_file_size_bytes_zero_means_no_limit() {
        let config = AppConfig {
            max_file_size_mb: 0,
            ..Default::default()
        };
        assert_eq!(config.max_file_size_bytes(), None);
    }

    #[test]
    fn test_sanitize_max_file_size_mb_zero_is_no_limit() {
        let mut config = AppConfig {
            max_file_size_mb: 0,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.max_file_size_mb, 0);
    }

    #[test]
    fn test_sanitize_clamps_max_file_size_mb_upper() {
        let mut config = AppConfig {
            max_file_size_mb: 20_000,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.max_file_size_mb, 10_240);
    }

    #[test]
    fn test_sanitize_preserves_valid_max_file_size_mb() {
        let mut config = AppConfig {
            max_file_size_mb: 100,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.max_file_size_mb, 100);
    }

    #[test]
    fn test_max_file_size_missing_field_gets_default() {
        let json = r#"{"current_theme": "Dark"}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.max_file_size_mb, 512);
    }

    #[test]
    fn test_max_file_size_serde_round_trip() {
        let config = AppConfig {
            max_file_size_mb: 256,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.max_file_size_mb, 256);
    }

    // ── Copy contents warning threshold tests ───────────────────────

    #[test]
    fn test_copy_contents_warning_mb_default() {
        let config = AppConfig::default();
        assert_eq!(config.copy_contents_warning_mb, 5);
    }

    #[test]
    fn test_copy_contents_warning_bytes_conversion() {
        let config = AppConfig::default();
        assert_eq!(config.copy_contents_warning_bytes(), 5 * 1024 * 1024);
    }

    #[test]
    fn test_copy_contents_warning_bytes_zero_means_always_prompt() {
        let config = AppConfig {
            copy_contents_warning_mb: 0,
            ..Default::default()
        };
        assert_eq!(config.copy_contents_warning_bytes(), 0);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_clamped_to_hard_cap() {
        // The warning is clamped to the Copy Contents hard cap, NOT the
        // editor open limit (`max_file_size_mb`).
        let mut config = AppConfig {
            copy_contents_max_mb: 10,
            copy_contents_warning_mb: 50,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 10);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_independent_of_editor_limit() {
        // A small editor open limit must NOT clamp the copy-contents warning.
        let mut config = AppConfig {
            max_file_size_mb: 1,
            copy_contents_max_mb: 64,
            copy_contents_warning_mb: 5,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 5);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_preserved_when_under_cap() {
        let mut config = AppConfig {
            copy_contents_max_mb: 100,
            copy_contents_warning_mb: 5,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 5);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_zero_preserved() {
        let mut config = AppConfig {
            copy_contents_max_mb: 100,
            copy_contents_warning_mb: 0,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 0);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_no_hard_cap_no_clamp() {
        let mut config = AppConfig {
            copy_contents_max_mb: 0,
            copy_contents_warning_mb: 50_000,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 50_000);
    }

    #[test]
    fn test_copy_contents_warning_missing_field_gets_default() {
        let json = r#"{"current_theme": "Dark"}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.copy_contents_warning_mb, 5);
    }

    #[test]
    fn test_copy_contents_max_bytes_conversion() {
        let config = AppConfig {
            copy_contents_max_mb: 64,
            ..Default::default()
        };
        assert_eq!(config.copy_contents_max_bytes(), Some(64 * 1024 * 1024));
    }

    #[test]
    fn test_copy_contents_max_bytes_zero_means_no_limit() {
        let config = AppConfig {
            copy_contents_max_mb: 0,
            ..Default::default()
        };
        assert_eq!(config.copy_contents_max_bytes(), None);
    }

    #[test]
    fn test_sanitize_copy_contents_max_clamped() {
        let mut config = AppConfig {
            copy_contents_max_mb: 99_999,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_max_mb, 10_240);
    }

    #[test]
    fn test_copy_contents_max_missing_field_gets_default() {
        let json = r#"{"current_theme": "Dark"}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.copy_contents_max_mb, 64);
    }

    #[test]
    fn test_copy_contents_warning_serde_round_trip() {
        let config = AppConfig {
            copy_contents_warning_mb: 25,
            ..Default::default()
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.copy_contents_warning_mb, 25);
    }

    // ── Workspace sidebar width tests ────────────────────────────────

    #[test]
    fn test_workspace_sidebar_width_default() {
        let config = AppConfig::default();
        assert!((config.workspace_sidebar_width - 250.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sanitize_clamps_workspace_sidebar_width_below_min() {
        let mut config = AppConfig {
            workspace_sidebar_width: 50.0,
            ..Default::default()
        };
        config.sanitize();
        assert!((config.workspace_sidebar_width - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sanitize_clamps_workspace_sidebar_width_above_max() {
        let mut config = AppConfig {
            workspace_sidebar_width: 800.0,
            ..Default::default()
        };
        config.sanitize();
        assert!((config.workspace_sidebar_width - 500.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sanitize_preserves_valid_workspace_sidebar_width() {
        let mut config = AppConfig {
            workspace_sidebar_width: 300.0,
            ..Default::default()
        };
        config.sanitize();
        assert!((config.workspace_sidebar_width - 300.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_workspace_sidebar_visible_default_false() {
        let config = AppConfig::default();
        assert!(!config.workspace_sidebar_visible);
    }

    #[test]
    fn test_workspace_sidebar_width_boundary_min() {
        let mut config = AppConfig {
            workspace_sidebar_width: 150.0,
            ..Default::default()
        };
        config.sanitize();
        assert!((config.workspace_sidebar_width - 150.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_workspace_sidebar_width_boundary_max() {
        let mut config = AppConfig {
            workspace_sidebar_width: 500.0,
            ..Default::default()
        };
        config.sanitize();
        assert!((config.workspace_sidebar_width - 500.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_workspace_sidebar_fields_serde_roundtrip() {
        let config = AppConfig {
            workspace_sidebar_visible: true,
            workspace_sidebar_width: 350.0,
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();

        assert!(parsed.workspace_sidebar_visible);
        assert!((parsed.workspace_sidebar_width - 350.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_workspace_sidebar_fields_missing_in_json_uses_defaults() {
        // A JSON without workspace fields should deserialize with defaults
        let json = r#"{"tab_size": 4}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!parsed.workspace_sidebar_visible);
        assert!(
            (parsed.workspace_sidebar_width - 0.0).abs() < f32::EPSILON
                || (parsed.workspace_sidebar_width - 250.0).abs() < f32::EPSILON
        );
    }

    // ── Show hidden files tests ─────────────────────────────────────

    #[test]
    fn test_show_hidden_files_default_false() {
        let config = AppConfig::default();
        assert!(!config.show_hidden_files);
    }

    #[test]
    fn test_show_hidden_files_serde_roundtrip() {
        let config = AppConfig {
            show_hidden_files: true,
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert!(parsed.show_hidden_files);
    }

    #[test]
    fn test_show_hidden_files_missing_field_gets_default() {
        let json = r#"{"current_theme": "Dark"}"#;
        let parsed: AppConfig = serde_json::from_str(json).unwrap();
        assert!(!parsed.show_hidden_files);
    }

    // ── Window geometry tests ───────────────────────────────────────

    fn plausible_geometry() -> WindowGeometry {
        WindowGeometry {
            x: -1920.0,
            y: 10.0,
            inner_w: 1200.0,
            inner_h: 800.0,
            maximized: false,
            monitor_w: 1920.0,
            monitor_h: 1080.0,
        }
    }

    #[test]
    fn test_geometry_serde_round_trip() {
        let config = AppConfig {
            window_geometry: Some(plausible_geometry()),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        let parsed: AppConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.window_geometry, Some(plausible_geometry()));
    }

    #[test]
    fn test_geometry_missing_field_stays_none_and_absent_in_json() {
        let config = AppConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert!(!json.contains("window_geometry"));
        let parsed: AppConfig = serde_json::from_str(r#"{"current_theme": "Dark"}"#).unwrap();
        assert!(parsed.window_geometry.is_none());
    }

    #[test]
    fn test_sanitize_preserves_negative_multi_monitor_position() {
        let mut config = AppConfig {
            window_geometry: Some(plausible_geometry()),
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.window_geometry, Some(plausible_geometry()));
    }

    #[test]
    fn test_sanitize_drops_non_finite_geometry() {
        let mut geometry = plausible_geometry();
        geometry.inner_w = f32::INFINITY;
        let mut config = AppConfig {
            window_geometry: Some(geometry),
            ..Default::default()
        };
        config.sanitize();
        assert!(config.window_geometry.is_none());
    }

    #[test]
    fn test_sanitize_drops_non_positive_sizes() {
        let mut geometry = plausible_geometry();
        geometry.inner_h = 0.0;
        let mut config = AppConfig {
            window_geometry: Some(geometry),
            ..Default::default()
        };
        config.sanitize();
        assert!(config.window_geometry.is_none());
    }

    #[test]
    fn test_geometry_out_of_range_number_fails_parse_and_takes_backup_path() {
        let json = r#"{"window_geometry": {"x": 0.0, "y": 0.0, "inner_w": 1e999, "inner_h": 800.0, "maximized": false, "monitor_w": 1920.0, "monitor_h": 1080.0}}"#;
        assert!(serde_json::from_str::<AppConfig>(json).is_err());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, json).unwrap();
        let (loaded, report) = AppConfig::load_or_create_with_report(&path);
        assert!(loaded.window_geometry.is_none());
        assert!(report.parse_error.is_some());
        assert!(dir.path().join("config.json.bak").exists());
    }

    // ── Restore rules (Phase G / ADR-7) ─────────────────────────────

    /// A geometry fully on a 1920×1080 primary monitor.
    fn on_screen_geometry() -> WindowGeometry {
        WindowGeometry {
            x: 100.0,
            y: 60.0,
            inner_w: 1200.0,
            inner_h: 800.0,
            maximized: false,
            monitor_w: 1920.0,
            monitor_h: 1080.0,
        }
    }

    #[test]
    fn restore_inner_size_passes_valid_size_through() {
        let (w, h, clamped) = on_screen_geometry().restore_inner_size();
        assert!((w - 1200.0).abs() < f32::EPSILON);
        assert!((h - 800.0).abs() < f32::EPSILON);
        assert!(!clamped);
    }

    #[test]
    fn restore_inner_size_clamps_tiny_sizes_up() {
        let mut g = on_screen_geometry();
        g.inner_w = 10.0;
        g.inner_h = 5.0;
        let (w, h, clamped) = g.restore_inner_size();
        assert!((w - 800.0).abs() < f32::EPSILON);
        assert!((h - 600.0).abs() < f32::EPSILON);
        assert!(clamped);
    }

    #[test]
    fn restore_inner_size_clamps_oversized_to_saved_monitor() {
        let mut g = on_screen_geometry();
        g.inner_w = 99_999.0;
        g.inner_h = 99_999.0;
        let (w, h, clamped) = g.restore_inner_size();
        assert!((w - 1920.0).abs() < f32::EPSILON);
        assert!((h - 1080.0).abs() < f32::EPSILON);
        assert!(clamped);
    }

    #[test]
    fn restore_inner_size_minimum_beats_a_tiny_monitor() {
        let mut g = on_screen_geometry();
        g.monitor_w = 640.0;
        g.monitor_h = 480.0;
        let (w, h, _) = g.restore_inner_size();
        assert!((w - 800.0).abs() < f32::EPSILON);
        assert!((h - 600.0).abs() < f32::EPSILON);
    }

    #[test]
    fn restore_position_accepts_on_screen_rect() {
        assert_eq!(on_screen_geometry().restore_position(), Some((100.0, 60.0)));
    }

    #[test]
    fn restore_position_never_zero_clamps_negative_coords() {
        // A window straddling the left monitor edge: negative x, but the
        // title strip still reaches into the monitor.
        let mut g = on_screen_geometry();
        g.x = -400.0;
        assert_eq!(g.restore_position(), Some((-400.0, 60.0)));
    }

    #[test]
    fn restore_position_rejects_rect_past_the_right_edge() {
        let mut g = on_screen_geometry();
        g.x = 2000.0; // entirely beyond the saved 1920-wide monitor
        assert_eq!(g.restore_position(), None);
    }

    #[test]
    fn restore_position_rejects_title_strip_above_the_screen() {
        // The window body would still be visible, but the grabbable title
        // strip is entirely above the screen, so the position is discarded.
        let mut g = on_screen_geometry();
        g.y = -(TITLE_STRIP_HEIGHT + 1.0);
        assert_eq!(g.restore_position(), None);
    }

    #[test]
    fn restore_position_rejects_rect_below_the_screen() {
        let mut g = on_screen_geometry();
        g.y = 1100.0; // strip top below the 1080-tall monitor
        assert_eq!(g.restore_position(), None);
    }

    // ── peek_window_geometry ────────────────────────────────────────

    #[test]
    fn peek_window_geometry_reads_saved_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let config = AppConfig {
            window_geometry: Some(on_screen_geometry()),
            ..Default::default()
        };
        config.save(&path).unwrap();
        assert_eq!(
            AppConfig::peek_window_geometry(&path),
            Some(on_screen_geometry())
        );
    }

    #[test]
    fn peek_window_geometry_missing_file_is_none_and_creates_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert_eq!(AppConfig::peek_window_geometry(&path), None);
        assert!(!path.exists(), "peek must not create the config file");
    }

    #[test]
    fn peek_window_geometry_broken_file_is_none_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(AppConfig::peek_window_geometry(&path), None);
        assert!(path.exists());
        assert!(
            !dir.path().join("config.json.bak").exists(),
            "peek must not move broken files aside"
        );
    }

    #[test]
    fn peek_window_geometry_filters_implausible_geometry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        let mut g = on_screen_geometry();
        g.inner_w = -5.0;
        let config = AppConfig {
            window_geometry: Some(g),
            ..Default::default()
        };
        // Bypass save-time sanitize by writing the JSON directly.
        std::fs::write(&path, serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(AppConfig::peek_window_geometry(&path), None);
    }
}
