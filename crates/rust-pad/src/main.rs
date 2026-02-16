#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

/// A cross-platform text editor built with Rust and egui.
#[derive(Parser, Debug)]
#[command(name = "rust-pad", version, about)]
struct Cli {
    /// Files to open on startup.
    files: Vec<PathBuf>,

    /// Create a new tab pre-filled with the given text.
    #[arg(long = "new-file")]
    new_file: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tracing::info!("Starting rust-pad");

    let startup_args = rust_pad_ui::StartupArgs {
        files: cli.files,
        new_file_text: cli.new_file,
    };

    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/logo2.png"))
        .map_err(|e| anyhow::anyhow!("failed to load app icon: {e}"))?;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_min_inner_size([400.0, 300.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        "rust-pad",
        native_options,
        Box::new(move |cc| Ok(Box::new(rust_pad_ui::App::new(cc, startup_args)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {e}"))?;

    Ok(())
}
