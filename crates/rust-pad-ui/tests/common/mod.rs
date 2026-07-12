use egui_kittest::Harness;
use rust_pad_ui::{App, StartupArgs};

/// Creates a standard test harness with the app at 1024x768.
///
/// Portable mode makes config load from the test executable directory,
/// where no config file exists, so the app starts with default values.
pub fn create_harness() -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::Vec2::new(1024.0, 768.0))
        .build_eframe(|cc| {
            App::new(
                cc,
                StartupArgs {
                    portable: true,
                    ..Default::default()
                },
            )
        })
}
