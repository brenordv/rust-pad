mod app;
pub mod dialogs;
mod editor;
pub mod icons;
pub mod io_worker;
pub mod problem_log;
mod tabs;
mod text_sanitize;
pub mod workspace;

pub use app::resolved_theme;
pub use app::{
    App, SettingsTab, StartupArgs, ThemeController, ThemeMode, FONT_FAMILY_MEDIUM,
    FONT_FAMILY_SEMIBOLD,
};
