/// Application configuration: load, save, merge, and sanitize.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::{builtin_dark, builtin_light, sample_wacky, ThemeDefinition};

/// When to remove dead (non-existent) files from the recent files list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum RecentFilesCleanup {
    #[default]
    OnStartup,
    OnMenuOpen,
    Both,
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
    /// `max_file_size_mb` are refused outright (no prompt). 0 = always prompt.
    pub copy_contents_warning_mb: u64,
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
            session_content_max_kb: 10_240,
            print_show_line_numbers: true,
            sync_scroll_enabled: false,
            sync_scroll_horizontal: true,
            workspace_sidebar_visible: false,
            workspace_sidebar_width: 250.0,
            show_hidden_files: false,
            themes: vec![builtin_dark(), builtin_light(), sample_wacky()],
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
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(contents) => match serde_json::from_str::<AppConfig>(&contents) {
                    Ok(mut config) => {
                        config.sanitize();
                        config.with_builtins_merged();
                        return config;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse config at {}: {e}", path.display());
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read config at {}: {e}", path.display());
                }
            }
            // Return defaults on error (don't overwrite broken file)
            let mut config = Self::default();
            config.sanitize();
            config
        } else {
            let config = Self::default();
            if let Err(e) = config.save(path) {
                tracing::warn!("Failed to create default config at {}: {e}", path.display());
            }
            config
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

    /// Ensures built-in Dark and Light themes are always present.
    /// User-defined themes with matching names take priority over built-ins.
    pub fn with_builtins_merged(&mut self) {
        let has_dark = self.themes.iter().any(|t| t.name == "Dark");
        let has_light = self.themes.iter().any(|t| t.name == "Light");

        if !has_dark {
            self.themes.insert(0, builtin_dark());
        }
        if !has_light {
            let insert_at = 1.min(self.themes.len());
            self.themes.insert(insert_at, builtin_light());
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
    /// `0` in `copy_contents_warning_mb` means "always prompt" — the
    /// caller treats a `0` return value as "every file triggers the
    /// confirmation dialog".
    pub fn copy_contents_warning_bytes(&self) -> u64 {
        self.copy_contents_warning_mb.saturating_mul(1024 * 1024)
    }

    /// Clamps values to valid ranges and resets invalid fields.
    pub fn sanitize(&mut self) {
        self.max_zoom_level = self.max_zoom_level.max(1.0);
        self.current_zoom_level = self.current_zoom_level.clamp(0.5, self.max_zoom_level);
        self.font_size = self.font_size.clamp(6.0, 72.0);

        let valid_modes = ["System", "Dark", "Light"];
        // Also allow any custom theme name as a valid mode
        let theme_names: Vec<String> = self.themes.iter().map(|t| t.name.clone()).collect();
        if !valid_modes.contains(&self.current_theme.as_str())
            && !theme_names.contains(&self.current_theme)
        {
            self.current_theme = "System".to_string();
        }
        self.auto_save_interval_secs = self.auto_save_interval_secs.max(5);
        self.recent_files_max_count = self.recent_files_max_count.clamp(1, 50);
        self.recent_files.truncate(self.recent_files_max_count);
        // 0 = no limit; otherwise clamp to 1..=10_240 MB (10 GB)
        if self.max_file_size_mb > 0 {
            self.max_file_size_mb = self.max_file_size_mb.clamp(1, 10_240);
        }
        // The Copy Contents warning threshold cannot exceed the hard cap —
        // otherwise the user would never see the prompt before hitting the
        // outright refusal. `0` on either side means "no limit / always
        // prompt" and is preserved as-is.
        if self.copy_contents_warning_mb > 0 && self.max_file_size_mb > 0 {
            self.copy_contents_warning_mb =
                self.copy_contents_warning_mb.min(self.max_file_size_mb);
        }
        // 0 = unlimited; otherwise clamp to 1..=102_400 KB (100 MB)
        if self.session_content_max_kb > 0 {
            self.session_content_max_kb = self.session_content_max_kb.clamp(1, 102_400);
        }
        self.workspace_sidebar_width = self.workspace_sidebar_width.clamp(150.0, 500.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.current_theme, "System");
        assert!((config.current_zoom_level - 1.0).abs() < f32::EPSILON);
        assert!(!config.word_wrap);
        assert!(!config.show_special_chars);
        assert!(config.restore_open_files);
        assert!((config.font_size - 16.0).abs() < f32::EPSILON);
        assert_eq!(config.themes.len(), 3);
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
        assert_eq!(names, vec!["Dark", "Light", "Wacky"]);
    }

    #[test]
    fn test_with_builtins_merged_adds_missing() {
        let mut config = AppConfig {
            themes: vec![sample_wacky()],
            ..Default::default()
        };
        config.with_builtins_merged();
        assert!(config.find_theme("Dark").is_some());
        assert!(config.find_theme("Light").is_some());
        assert!(config.find_theme("Wacky").is_some());
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
        let mut config = AppConfig {
            max_file_size_mb: 10,
            copy_contents_warning_mb: 50,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 10);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_preserved_when_under_cap() {
        let mut config = AppConfig {
            max_file_size_mb: 100,
            copy_contents_warning_mb: 5,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 5);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_zero_preserved() {
        let mut config = AppConfig {
            max_file_size_mb: 100,
            copy_contents_warning_mb: 0,
            ..Default::default()
        };
        config.sanitize();
        assert_eq!(config.copy_contents_warning_mb, 0);
    }

    #[test]
    fn test_sanitize_copy_contents_warning_no_hard_cap_no_clamp() {
        let mut config = AppConfig {
            max_file_size_mb: 0,
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
}
