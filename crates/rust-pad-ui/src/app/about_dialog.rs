//! About dialog showing application information, version, author, and links.

use eframe::egui;

use super::App;

impl App {
    /// Renders the About dialog window.
    ///
    /// Returns `true` if the dialog is open.
    pub(crate) fn show_about_dialog(&mut self, ctx: &egui::Context) -> bool {
        if !self.about_open {
            return false;
        }

        let mut open = true;
        egui::Window::new("About rust-pad")
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    // Logo
                    if let Some(texture) = &self.about_logo {
                        let size = egui::Vec2::new(96.0, 96.0);
                        ui.image(egui::load::SizedTexture::new(texture.id(), size));
                    }

                    ui.add_space(8.0);

                    ui.heading("rust-pad");
                    ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));

                    ui.add_space(4.0);

                    ui.label("A cross-platform text editor built with Rust and egui");

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    ui.label(format!("Author: {}", Self::author_name()));
                    ui.label(format!("License: {}", env!("CARGO_PKG_LICENSE")));

                    ui.add_space(4.0);

                    ui.horizontal(|ui| {
                        ui.label("GitHub:");
                        ui.hyperlink_to(
                            "brenordv/rust-pad",
                            "https://github.com/brenordv/rust-pad",
                        );
                    });

                    ui.horizontal(|ui| {
                        ui.label("Author:");
                        ui.hyperlink_to("raccoon.ninja", "https://raccoon.ninja");
                    });

                    ui.add_space(8.0);
                    ui.separator();
                    ui.add_space(4.0);

                    ui.label(format!(
                        "OS: {} {}",
                        Self::os_name(),
                        std::env::consts::ARCH
                    ));
                    ui.label(format!("Built with Rust {}", Self::rustc_version()));
                });
            });

        if !open {
            self.about_open = false;
        }

        self.about_open
    }

    /// Loads the logo texture for the About dialog.
    pub(crate) fn load_about_logo(&mut self, ctx: &egui::Context) {
        if self.about_logo.is_some() {
            return;
        }

        let png_bytes = include_bytes!("../../../../assets/logo2.png");
        let image = match image::load_from_memory(png_bytes) {
            Ok(img) => img,
            Err(e) => {
                tracing::warn!("Failed to load about logo: {e}");
                return;
            }
        };
        let rgba = image.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let pixels = rgba.into_raw();
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
        let texture = ctx.load_texture("about-logo", color_image, egui::TextureOptions::default());
        self.about_logo = Some(texture);
    }

    fn author_name() -> &'static str {
        // Authors from Cargo.toml, format: "Name <email>"
        let authors = env!("CARGO_PKG_AUTHORS");
        if authors.is_empty() {
            "Unknown"
        } else {
            authors
        }
    }

    fn os_name() -> &'static str {
        #[cfg(target_os = "windows")]
        {
            "Windows"
        }
        #[cfg(target_os = "macos")]
        {
            "macOS"
        }
        #[cfg(target_os = "linux")]
        {
            "Linux"
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
        {
            std::env::consts::OS
        }
    }

    fn rustc_version() -> &'static str {
        let version = env!("CARGO_PKG_RUST_VERSION");
        if version.is_empty() {
            "stable"
        } else {
            version
        }
    }
}

#[cfg(test)]
mod tests {
    use super::App;

    #[test]
    fn test_author_name_not_empty() {
        let name = App::author_name();
        assert!(!name.is_empty(), "author_name() should not be empty");
    }

    #[test]
    fn test_author_name_contains_expected() {
        let name = App::author_name();
        assert!(
            name.contains("Breno"),
            "Expected author name to contain 'Breno', got: {name}"
        );
    }

    #[test]
    fn test_os_name_is_known_platform() {
        let os = App::os_name();
        let valid = ["Windows", "macOS", "Linux"];
        assert!(
            valid.contains(&os) || !os.is_empty(),
            "os_name() returned unexpected value: {os}"
        );
    }

    #[test]
    fn test_rustc_version_not_empty() {
        let version = App::rustc_version();
        assert!(!version.is_empty(), "rustc_version() should not be empty");
    }

    #[test]
    fn test_rustc_version_fallback() {
        // Since we don't set rust-version in Cargo.toml, should return "stable"
        let version = App::rustc_version();
        assert_eq!(version, "stable");
    }
}
