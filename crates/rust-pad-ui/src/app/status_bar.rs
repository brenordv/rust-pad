//! Status bar rendering for the editor application.
//!
//! Shows cursor position, encoding, line ending, indent style, character count,
//! file size, zoom level, match count, bookmark count, last saved time, and file path.

use eframe::egui;
use egui::{Color32, RichText};

use super::App;

/// Formats a byte count as a human-readable size (B, KB, MB, GB, TB).
fn format_file_size(bytes: usize) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    const TB: f64 = GB * 1024.0;

    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < MB {
        format!("{:.1} KB", b / KB)
    } else if b < GB {
        format!("{:.1} MB", b / MB)
    } else if b < TB {
        format!("{:.2} GB", b / GB)
    } else {
        format!("{:.2} TB", b / TB)
    }
}

/// Formats a character count with thousands separators.
fn format_char_count(count: usize) -> String {
    if count < 1_000 {
        format!("{count}")
    } else if count < 1_000_000 {
        format!("~{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("~{:.1}M", count as f64 / 1_000_000.0)
    }
}

/// Returns `true` if the system uses 24-hour time format.
fn system_uses_24h() -> bool {
    #[cfg(target_os = "windows")]
    {
        // Read the Windows locale time format from the registry or GetLocaleInfoW.
        // A simpler heuristic: format a known time with chrono's locale and check,
        // but chrono doesn't read Windows locale. Use winapi GetLocaleInfoEx.
        use std::sync::LazyLock;
        static IS_24H: LazyLock<bool> = LazyLock::new(|| {
            // Try reading the registry key for time format
            let output = std::process::Command::new("reg")
                .args(["query", r"HKCU\Control Panel\International", "/v", "iTime"])
                .output();
            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    // iTime = 1 means 24h, iTime = 0 means 12h
                    stdout.contains("0x1") || stdout.contains("    1")
                }
                Err(_) => false, // Default to 12h on failure
            }
        });
        *IS_24H
    }

    #[cfg(target_os = "macos")]
    {
        use std::sync::LazyLock;
        static IS_24H: LazyLock<bool> = LazyLock::new(|| {
            let output = std::process::Command::new("defaults")
                .args(["read", "NSGlobalDomain", "AppleICUForce24HourTime"])
                .output();
            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    stdout.trim() == "1"
                }
                Err(_) => true, // Default to 24h on macOS
            }
        });
        *IS_24H
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // Linux: check LC_TIME locale
        use std::sync::LazyLock;
        static IS_24H: LazyLock<bool> = LazyLock::new(|| {
            // Most Linux locales default to 24h; US locale uses 12h
            let lang = std::env::var("LC_TIME")
                .or_else(|_| std::env::var("LC_ALL"))
                .or_else(|_| std::env::var("LANG"))
                .unwrap_or_default();
            // en_US uses 12h, most others use 24h
            !lang.starts_with("en_US")
        });
        *IS_24H
    }
}

/// Formats a timestamp for the status bar display.
fn format_saved_time(dt: &chrono::DateTime<chrono::Local>) -> String {
    if system_uses_24h() {
        dt.format("%Y-%m-%d @ %H:%M:%S").to_string()
    } else {
        dt.format("%Y-%m-%d @ %l:%M:%S %p")
            .to_string()
            .trim()
            .to_string()
    }
}

/// Snapshot of document state captured before rendering the status bar.
///
/// This avoids borrowing `self` while rendering individual sections.
struct StatusBarData {
    cursor_line: usize,
    cursor_col: usize,
    encoding: rust_pad_core::encoding::TextEncoding,
    line_ending: rust_pad_core::encoding::LineEnding,
    indent_style: rust_pad_core::indent::IndentStyle,
    line_count: usize,
    char_count: usize,
    byte_size: usize,
    last_saved: Option<chrono::DateTime<chrono::Local>>,
    live_monitoring: bool,
    auto_save: bool,
    file_path_display: Option<String>,
    match_info: Option<(usize, usize)>,
    bookmark_count: usize,
    zoom_level: f32,
}

impl App {
    /// Renders the status bar at the bottom of the application window.
    pub(crate) fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        let data = self.collect_status_bar_data();

        ui.horizontal(|ui| {
            Self::status_bar_cursor_info(ui, &data);
            ui.separator();
            self.status_bar_encoding_selector(ui, &data);
            ui.separator();
            self.status_bar_line_ending_selector(ui, &data);
            ui.separator();
            self.status_bar_indent_selector(ui, &data);
            ui.separator();
            Self::status_bar_document_stats(ui, &data);
            Self::status_bar_indicators(ui, &data);
            Self::status_bar_right_section(ui, &data);
        });
    }

    /// Collects all the data needed by the status bar into a snapshot struct.
    fn collect_status_bar_data(&self) -> StatusBarData {
        let doc = self.tabs.active_doc();
        let match_info = if !self.find_replace.engine.matches.is_empty() {
            let current = self
                .find_replace
                .engine
                .current_match
                .map(|i| i + 1)
                .unwrap_or(0);
            let total = self.find_replace.engine.match_count();
            Some((current, total))
        } else {
            None
        };

        StatusBarData {
            cursor_line: doc.cursor.position.line,
            cursor_col: doc.cursor.position.col,
            encoding: doc.encoding,
            line_ending: doc.line_ending,
            indent_style: doc.indent_style,
            line_count: doc.buffer.len_lines(),
            char_count: doc.buffer.len_chars(),
            byte_size: doc.buffer.len_bytes(),
            last_saved: doc.last_saved_at,
            live_monitoring: doc.live_monitoring,
            auto_save: self.auto_save_enabled,
            file_path_display: doc.file_path.as_ref().map(|p| p.display().to_string()),
            match_info,
            bookmark_count: self.bookmarks.count(),
            zoom_level: self.zoom_level,
        }
    }

    /// Renders the cursor position (line and column).
    fn status_bar_cursor_info(ui: &mut egui::Ui, data: &StatusBarData) {
        ui.add(
            egui::Label::new(format!(
                "Ln {}, Col {}",
                data.cursor_line + 1,
                data.cursor_col + 1
            ))
            .selectable(false),
        );
    }

    /// Renders the clickable encoding selector popup.
    fn status_bar_encoding_selector(&mut self, ui: &mut egui::Ui, data: &StatusBarData) {
        let enc_response = ui.add(
            egui::Label::new(format!("{}", data.encoding))
                .selectable(false)
                .sense(egui::Sense::click()),
        );
        egui::Popup::from_toggle_button_response(&enc_response).show(|ui| {
            use rust_pad_core::encoding::TextEncoding;
            for enc in [
                TextEncoding::Utf8,
                TextEncoding::Utf8Bom,
                TextEncoding::Utf16Le,
                TextEncoding::Utf16Be,
                TextEncoding::Ascii,
            ] {
                if ui.radio(data.encoding == enc, format!("{enc}")).clicked() {
                    self.tabs.active_doc_mut().encoding = enc;
                    self.tabs.active_doc_mut().modified = true;
                    ui.close();
                }
            }
        });
    }

    /// Renders the clickable line ending selector popup.
    fn status_bar_line_ending_selector(&mut self, ui: &mut egui::Ui, data: &StatusBarData) {
        let eol_response = ui.add(
            egui::Label::new(format!("{}", data.line_ending))
                .selectable(false)
                .sense(egui::Sense::click()),
        );
        egui::Popup::from_toggle_button_response(&eol_response).show(|ui| {
            use rust_pad_core::encoding::LineEnding;
            for eol in [LineEnding::Lf, LineEnding::CrLf, LineEnding::Cr] {
                if ui
                    .radio(data.line_ending == eol, format!("{eol}"))
                    .clicked()
                {
                    self.tabs.active_doc_mut().line_ending = eol;
                    self.tabs.active_doc_mut().modified = true;
                    ui.close();
                }
            }
        });
    }

    /// Renders the clickable indent style selector popup.
    fn status_bar_indent_selector(&mut self, ui: &mut egui::Ui, data: &StatusBarData) {
        let indent_response = ui.add(
            egui::Label::new(format!("{}", data.indent_style))
                .selectable(false)
                .sense(egui::Sense::click()),
        );
        egui::Popup::from_toggle_button_response(&indent_response).show(|ui| {
            use rust_pad_core::indent::IndentStyle;
            for style in [
                IndentStyle::Spaces(2),
                IndentStyle::Spaces(4),
                IndentStyle::Spaces(8),
                IndentStyle::Tabs,
            ] {
                if ui
                    .radio(data.indent_style == style, format!("{style}"))
                    .clicked()
                {
                    self.tabs.active_doc_mut().indent_style = style;
                    ui.close();
                }
            }
        });
    }

    /// Renders document statistics: line count, char count, file size, and zoom level.
    fn status_bar_document_stats(ui: &mut egui::Ui, data: &StatusBarData) {
        ui.add(egui::Label::new(format!("{} lines", data.line_count)).selectable(false));
        ui.separator();
        ui.add(
            egui::Label::new(format!("{} chars", format_char_count(data.char_count)))
                .selectable(false),
        );
        ui.separator();
        ui.add(egui::Label::new(format_file_size(data.byte_size)).selectable(false));
        ui.separator();
        ui.add(
            egui::Label::new(format!("Zoom: {:.0}%", data.zoom_level * 100.0)).selectable(false),
        );
    }

    /// Renders conditional indicators: match count, bookmarks, live monitoring, auto-save.
    fn status_bar_indicators(ui: &mut egui::Ui, data: &StatusBarData) {
        if let Some((current, total)) = data.match_info {
            ui.separator();
            ui.add(egui::Label::new(format!("Match {current}/{total}")).selectable(false));
        }

        if data.bookmark_count > 0 {
            ui.separator();
            ui.add(
                egui::Label::new(format!("Bookmarks: {}", data.bookmark_count)).selectable(false),
            );
        }

        if data.live_monitoring {
            ui.separator();
            ui.add(egui::Label::new("LIVE").selectable(false));
        }

        if data.auto_save {
            ui.separator();
            ui.add(
                egui::Label::new(RichText::new("Auto-Save").color(Color32::GRAY)).selectable(false),
            );
        }
    }

    /// Renders the right-aligned section: last saved time and file path.
    fn status_bar_right_section(ui: &mut egui::Ui, data: &StatusBarData) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(ref path_str) = data.file_path_display {
                ui.add(
                    egui::Label::new(RichText::new(path_str).small().color(Color32::GRAY))
                        .selectable(false),
                );
            }

            if let Some(saved_at) = data.last_saved {
                if data.file_path_display.is_some() {
                    ui.separator();
                }
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Saved: {}", format_saved_time(&saved_at)))
                            .small()
                            .color(Color32::GRAY),
                    )
                    .selectable(false),
                );
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- format_file_size ----

    #[test]
    fn test_format_file_size_zero() {
        assert_eq!(format_file_size(0), "0 B");
    }

    #[test]
    fn test_format_file_size_bytes() {
        assert_eq!(format_file_size(512), "512 B");
        assert_eq!(format_file_size(1023), "1023 B");
    }

    #[test]
    fn test_format_file_size_kilobytes() {
        assert_eq!(format_file_size(1024), "1.0 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
        assert_eq!(format_file_size(10 * 1024), "10.0 KB");
    }

    #[test]
    fn test_format_file_size_megabytes() {
        assert_eq!(format_file_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_file_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn test_format_file_size_gigabytes() {
        assert_eq!(format_file_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_file_size(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[test]
    fn test_format_file_size_terabytes() {
        assert_eq!(format_file_size(1024 * 1024 * 1024 * 1024), "1.00 TB");
    }

    #[test]
    fn test_format_file_size_boundary_kb() {
        // Exactly at the KB boundary
        assert_eq!(format_file_size(1024), "1.0 KB");
        // One byte below KB
        assert_eq!(format_file_size(1023), "1023 B");
    }

    // -- format_char_count ---

    #[test]
    fn test_format_char_count_small() {
        assert_eq!(format_char_count(0), "0");
        assert_eq!(format_char_count(42), "42");
        assert_eq!(format_char_count(999), "999");
    }

    #[test]
    fn test_format_char_count_thousands() {
        assert_eq!(format_char_count(1_000), "~1.0K");
        assert_eq!(format_char_count(1_500), "~1.5K");
        assert_eq!(format_char_count(999_999), "~1000.0K");
    }

    #[test]
    fn test_format_char_count_millions() {
        assert_eq!(format_char_count(1_000_000), "~1.0M");
        assert_eq!(format_char_count(2_500_000), "~2.5M");
    }

    // -- format_saved_time ---

    #[test]
    fn test_format_saved_time_returns_non_empty() {
        let dt = chrono::Local::now();
        let result = format_saved_time(&dt);
        assert!(!result.is_empty());
        // Should contain the date portion with @ separator
        assert!(result.contains('@'));
    }

    #[test]
    fn test_format_saved_time_contains_year_month_day() {
        let dt = chrono::Local::now();
        let result = format_saved_time(&dt);
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        assert!(
            result.starts_with(&today),
            "Expected time string '{result}' to start with date '{today}'"
        );
    }

    // -- system_uses_24h -----

    #[test]
    fn test_system_uses_24h_returns_bool() {
        // Verify it doesn't panic and returns a valid boolean
        let _result: bool = system_uses_24h();
    }
}
