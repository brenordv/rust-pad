/// Application configuration: load, save, merge, and sanitize.
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::{builtin_dark, builtin_light, sample_wacky, ThemeDefinition};

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
            remember_last_folder: true,
            default_work_folder: String::new(),
            last_used_folder: String::new(),
            auto_save_enabled: false,
            auto_save_interval_secs: 30,
            themes: vec![builtin_dark(), builtin_light(), sample_wacky()],
        }
    }
}

impl AppConfig {
    /// Returns the config file path: exe directory + `rust-pad.json`.
    pub fn config_path() -> PathBuf {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("rust-pad.json")))
            .unwrap_or_else(|| PathBuf::from("rust-pad.json"))
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
    pub fn save(&self, path: &std::path::Path) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
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
        let mut config = AppConfig::default();
        config.current_zoom_level = 10.0;
        config.sanitize();
        assert!((config.current_zoom_level - 10.0).abs() < f32::EPSILON);

        config.current_zoom_level = 0.1;
        config.sanitize();
        assert!((config.current_zoom_level - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sanitize_clamps_font_size() {
        let mut config = AppConfig::default();
        config.font_size = 2.0;
        config.sanitize();
        assert!((config.font_size - 6.0).abs() < f32::EPSILON);

        config.font_size = 100.0;
        config.sanitize();
        assert!((config.font_size - 72.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_sanitize_resets_unknown_theme_mode() {
        let mut config = AppConfig::default();
        config.current_theme = "NonExistent".to_string();
        config.sanitize();
        assert_eq!(config.current_theme, "System");
    }

    #[test]
    fn test_sanitize_allows_custom_theme_name() {
        let mut config = AppConfig::default();
        config.current_theme = "Wacky".to_string();
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
        let mut config = AppConfig::default();
        config.themes = vec![sample_wacky()];
        config.with_builtins_merged();
        assert!(config.find_theme("Dark").is_some());
        assert!(config.find_theme("Light").is_some());
        assert!(config.find_theme("Wacky").is_some());
    }

    #[test]
    fn test_with_builtins_merged_preserves_custom() {
        let mut custom_dark = builtin_dark();
        custom_dark.editor.bg_color = crate::HexColor::rgb(255, 0, 0);

        let mut config = AppConfig::default();
        config.themes = vec![custom_dark.clone()];
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
        let mut config = AppConfig::default();
        config.auto_save_interval_secs = 1;
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 5);
    }

    #[test]
    fn test_sanitize_preserves_valid_auto_save_interval() {
        let mut config = AppConfig::default();
        config.auto_save_interval_secs = 60;
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 60);
    }

    #[test]
    fn test_sanitize_clamps_auto_save_interval_zero() {
        let mut config = AppConfig::default();
        config.auto_save_interval_secs = 0;
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 5);
    }

    #[test]
    fn test_sanitize_auto_save_interval_boundary() {
        let mut config = AppConfig::default();
        config.auto_save_interval_secs = 5;
        config.sanitize();
        assert_eq!(config.auto_save_interval_secs, 5);
    }

    #[test]
    fn test_auto_save_serde_round_trip() {
        let mut config = AppConfig::default();
        config.auto_save_enabled = true;
        config.auto_save_interval_secs = 45;
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
}
