use rust_pad_config::{AppConfig, HexColor};

#[test]
fn test_load_creates_default_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    assert!(!path.exists());

    let config = AppConfig::load_or_create(&path);
    assert!(path.exists());
    assert_eq!(config.current_theme, "System");

    // File should contain valid JSON
    let contents = std::fs::read_to_string(&path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert!(parsed.is_object());
}

#[test]
fn test_load_existing_config() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    let json = r#"{
        "current_theme": "Dark",
        "current_zoom_level": 1.5,
        "word_wrap": true,
        "font_size": 16.0,
        "themes": []
    }"#;
    std::fs::write(&path, json).unwrap();

    let config = AppConfig::load_or_create(&path);
    assert_eq!(config.current_theme, "Dark");
    assert!((config.current_zoom_level - 1.5).abs() < f32::EPSILON);
    assert!(config.word_wrap);
    assert!((config.font_size - 16.0).abs() < f32::EPSILON);
}

#[test]
fn test_broken_json_returns_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    std::fs::write(&path, "{ this is not valid json }}}").unwrap();

    let config = AppConfig::load_or_create(&path);
    assert_eq!(config.current_theme, "System");
    assert!((config.current_zoom_level - 1.0).abs() < f32::EPSILON);
}

#[test]
fn test_partial_config_fills_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    std::fs::write(&path, r#"{"current_zoom_level": 2.0}"#).unwrap();

    let config = AppConfig::load_or_create(&path);
    assert!((config.current_zoom_level - 2.0).abs() < f32::EPSILON);
    assert_eq!(config.current_theme, "System");
    assert!(!config.word_wrap);
    assert!(!config.show_special_chars);
    assert!((config.font_size - 16.0).abs() < f32::EPSILON);
}

#[test]
fn test_custom_theme_overrides_builtin() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    let json = r##"{
        "themes": [
            {
                "name": "Dark",
                "dark_mode": true,
                "syntax_theme": "base16-eighties.dark",
                "editor": {
                    "bg_color": "#FF0000"
                },
                "ui": {}
            }
        ]
    }"##;
    std::fs::write(&path, json).unwrap();

    let config = AppConfig::load_or_create(&path);
    let dark = config.find_theme("Dark").unwrap();
    assert_eq!(dark.editor.bg_color, HexColor::rgb(255, 0, 0));
}

#[test]
fn test_builtin_themes_always_present() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    let json = r#"{
        "themes": [
            {
                "name": "Wacky",
                "dark_mode": true,
                "syntax_theme": "base16-eighties.dark",
                "editor": {},
                "ui": {}
            }
        ]
    }"#;
    std::fs::write(&path, json).unwrap();

    let config = AppConfig::load_or_create(&path);
    assert!(config.find_theme("Dark").is_some());
    assert!(config.find_theme("Light").is_some());
    assert!(config.find_theme("Wacky").is_some());
}

#[test]
fn test_save_then_load_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");

    let config = AppConfig {
        current_zoom_level: 1.8,
        word_wrap: true,
        font_size: 18.0,
        current_theme: "Dark".to_string(),
        ..Default::default()
    };
    config.save(&path).unwrap();

    let loaded = AppConfig::load_or_create(&path);
    assert_eq!(loaded.current_theme, "Dark");
    assert!((loaded.current_zoom_level - 1.8).abs() < f32::EPSILON);
    assert!(loaded.word_wrap);
    assert!((loaded.font_size - 18.0).abs() < f32::EPSILON);
}

#[test]
fn test_sanitize_clamps_values() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    let json = r#"{
        "current_zoom_level": 99.0,
        "font_size": 0.5,
        "current_theme": "NonExistent"
    }"#;
    std::fs::write(&path, json).unwrap();

    let config = AppConfig::load_or_create(&path);
    assert!((config.current_zoom_level - 15.0).abs() < f32::EPSILON);
    assert!((config.font_size - 6.0).abs() < f32::EPSILON);
    assert_eq!(config.current_theme, "System");
}

// ── New fields: restore_open_files, show_line_numbers, max_zoom_level ────

#[test]
fn test_restore_open_files_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");

    let config = AppConfig {
        restore_open_files: false,
        ..Default::default()
    };
    config.save(&path).unwrap();

    let loaded = AppConfig::load_or_create(&path);
    assert!(!loaded.restore_open_files);
}

#[test]
fn test_restore_open_files_defaults_true() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    // Config file without restore_open_files should default to true
    std::fs::write(&path, r#"{"current_zoom_level": 1.0}"#).unwrap();

    let config = AppConfig::load_or_create(&path);
    assert!(config.restore_open_files);
}

#[test]
fn test_show_line_numbers_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");

    let config = AppConfig {
        show_line_numbers: false,
        ..Default::default()
    };
    config.save(&path).unwrap();

    let loaded = AppConfig::load_or_create(&path);
    assert!(!loaded.show_line_numbers);
}

#[test]
fn test_show_line_numbers_defaults_true() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    std::fs::write(&path, r#"{"current_zoom_level": 1.0}"#).unwrap();

    let config = AppConfig::load_or_create(&path);
    assert!(config.show_line_numbers);
}

#[test]
fn test_max_zoom_level_persists() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");

    let config = AppConfig {
        max_zoom_level: 5.0,
        ..Default::default()
    };
    config.save(&path).unwrap();

    let loaded = AppConfig::load_or_create(&path);
    assert!((loaded.max_zoom_level - 5.0).abs() < f32::EPSILON);
}

#[test]
fn test_max_zoom_level_sanitize_clamps_minimum() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    std::fs::write(&path, r#"{"max_zoom_level": 0.1}"#).unwrap();

    let config = AppConfig::load_or_create(&path);
    assert!((config.max_zoom_level - 1.0).abs() < f32::EPSILON);
}

#[test]
fn test_zoom_level_clamped_by_max_zoom() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");
    // max_zoom_level = 3.0 but current_zoom_level = 10.0
    std::fs::write(
        &path,
        r#"{"max_zoom_level": 3.0, "current_zoom_level": 10.0}"#,
    )
    .unwrap();

    let config = AppConfig::load_or_create(&path);
    assert!((config.current_zoom_level - 3.0).abs() < f32::EPSILON);
    assert!((config.max_zoom_level - 3.0).abs() < f32::EPSILON);
}

#[test]
fn test_all_new_fields_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rust-pad.json");

    let config = AppConfig {
        restore_open_files: false,
        show_line_numbers: false,
        show_special_chars: true,
        max_zoom_level: 8.0,
        word_wrap: true,
        ..Default::default()
    };
    config.save(&path).unwrap();

    let loaded = AppConfig::load_or_create(&path);
    assert!(!loaded.restore_open_files);
    assert!(!loaded.show_line_numbers);
    assert!(loaded.show_special_chars);
    assert!((loaded.max_zoom_level - 8.0).abs() < f32::EPSILON);
    assert!(loaded.word_wrap);
}
