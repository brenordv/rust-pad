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

    /// Store config and data next to the executable instead of in
    /// platform-standard directories. Useful for USB/portable installs.
    #[arg(long)]
    portable: bool,
}

/// Raises the open-file-descriptor soft limit to the hard limit on Unix.
///
/// macOS `.app` bundles (and some shells) inherit a low `RLIMIT_NOFILE` soft
/// limit (historically 256). With a filesystem watcher, font/GPU handles, and
/// concurrent file I/O, that ceiling is easy to hit, surfacing as
/// `EMFILE` ("Too many open files") on the next open/read/write. Raising the
/// soft limit to the already-permitted hard limit costs nothing and removes a
/// whole class of spurious I/O failures. No-op on non-Unix.
#[cfg(unix)]
fn raise_fd_limit() {
    // SAFETY: `getrlimit`/`setrlimit` are async-signal-safe libc calls operating
    // on a stack-local `rlimit`; we only ever raise the soft limit up to the
    // existing hard limit, which any process is permitted to do.
    unsafe {
        let mut lim = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) != 0 {
            tracing::warn!("Could not query RLIMIT_NOFILE; leaving fd limit unchanged");
            return;
        }
        // macOS reports an "unlimited" (`RLIM_INFINITY`) hard limit for NOFILE
        // but actually refuses to set the soft limit above `OPEN_MAX` (10240) —
        // a naive `rlim_cur = rlim_max` would fail there, the very platform this
        // guards. Clamp to a finite target when the hard limit is unbounded;
        // finite hard limits (Linux) pass through unchanged.
        const FD_LIMIT_TARGET: libc::rlim_t = 10_240;
        let target = if lim.rlim_max == libc::RLIM_INFINITY {
            FD_LIMIT_TARGET
        } else {
            lim.rlim_max
        };
        if lim.rlim_cur >= target {
            return;
        }
        let previous = lim.rlim_cur;
        lim.rlim_cur = target;
        if libc::setrlimit(libc::RLIMIT_NOFILE, &lim) == 0 {
            tracing::info!(
                from = previous,
                to = target,
                "Raised open-file-descriptor soft limit"
            );
        } else {
            tracing::warn!("Could not raise RLIMIT_NOFILE; leaving fd limit unchanged");
        }
    }
}

#[cfg(not(unix))]
fn raise_fd_limit() {}

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

    // Raise the fd soft limit before opening any files or filesystem watchers.
    raise_fd_limit();

    // Initialize the global problem-log store before anything else so that
    // startup errors can be captured.
    rust_pad_ui::problem_log::init(cli.portable);

    // Install a panic hook that logs to the problem store so users can see
    // crash information in Help > Problems after a restart.
    rust_pad_ui::problem_log::install_panic_hook();

    let startup_args = rust_pad_ui::StartupArgs {
        files: cli.files,
        new_file_text: cli.new_file,
        portable: cli.portable,
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
