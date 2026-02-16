use egui_kittest::Harness;
use rust_pad_ui::{App, StartupArgs};

/// Creates a standard test harness with the app at 1024x768.
pub fn create_harness() -> Harness<'static, App> {
    Harness::builder()
        .with_size(egui::Vec2::new(1024.0, 768.0))
        .build_eframe(|cc| App::new(cc, StartupArgs::default()))
}
