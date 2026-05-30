//! Global problem-log accessor.
//!
//! The problem store is initialized once at startup (before `App::new`) and
//! is available to any code in the process via the free functions in this
//! module. When the store is unavailable (init failed or was never called),
//! all operations gracefully degrade to no-ops.

use std::sync::{Arc, OnceLock};

use rust_pad_config::problem_log::ProblemStore;

/// Process-wide problem store singleton.
static STORE: OnceLock<Arc<ProblemStore>> = OnceLock::new();

/// Initializes the global problem store.
///
/// When `portable` is true the database is created next to the executable;
/// otherwise it uses the platform-standard data directory.
///
/// Must be called exactly once, before any other function in this module.
/// If the database cannot be opened the global remains empty and all
/// subsequent operations fall back to tracing-only logging.
pub fn init(portable: bool) {
    let path = if portable {
        rust_pad_config::paths::portable_problem_log_file_path()
    } else {
        ProblemStore::default_path()
    };
    match ProblemStore::open(&path) {
        Ok(store) => {
            let _ = STORE.set(Arc::new(store));
        }
        Err(e) => {
            tracing::warn!("Failed to open problem log store: {e}");
        }
    }
}

/// Returns a reference to the global problem store, if available.
pub fn store() -> Option<&'static Arc<ProblemStore>> {
    STORE.get()
}

/// Records a problem entry. Falls back to `tracing::warn!` when the
/// store is unavailable.
pub fn log_problem(message: &str) {
    if let Some(s) = STORE.get() {
        if let Err(e) = s.add_entry(message) {
            tracing::warn!("Failed to write problem log entry: {e}");
        }
    }
}

/// Emits `message` at warn level via `tracing` and also persists it to
/// the problem store. Pairs the two operations that appeared together
/// at dozens of call sites across the UI layer.
///
/// Use this when the user should see the message in Help > Problems
/// and operators should see it in the structured log. Use bare
/// `tracing::warn!` when the message is purely diagnostic and would
/// only add noise to the user-facing problem list.
pub fn warn_problem(message: &str) {
    tracing::warn!("{message}");
    log_problem(message);
}

/// Same as [`warn_problem`] at info level. Used for routine outcomes
/// the user might want to acknowledge (e.g. "duplicate folder ignored").
pub fn info_problem(message: &str) {
    tracing::info!("{message}");
    log_problem(message);
}

/// Returns the number of unread problem entries (0 when the store is
/// unavailable).
pub fn unread_count() -> usize {
    STORE.get().and_then(|s| s.unread_count().ok()).unwrap_or(0)
}

/// Extracts a human-readable message from a panic payload.
///
/// Handles the two common payload types (`&str` and `String`) and falls
/// back to `"unknown panic"` for anything else.
pub fn format_panic_payload(payload: &dyn std::any::Any) -> String {
    match payload.downcast_ref::<&str>() {
        Some(s) => (*s).to_string(),
        None => match payload.downcast_ref::<String>() {
            Some(s) => s.clone(),
            None => "unknown panic".to_string(),
        },
    }
}

/// Formats a full panic message from a payload and an optional source location.
///
/// Returns a string like `"Panic at src/main.rs:42:5: something broke"`
/// or `"Panic: something broke"` when the location is unavailable.
pub fn format_panic_message(
    payload: &dyn std::any::Any,
    location: Option<(&str, u32, u32)>,
) -> String {
    let message = format_panic_payload(payload);
    let loc = location
        .map(|(file, line, col)| format!(" at {file}:{line}:{col}"))
        .unwrap_or_default();
    format!("Panic{loc}: {message}")
}

/// Installs a panic hook that logs crash information to the problem
/// store so users can review it in Help > Problems after a restart.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info.location().map(|l| (l.file(), l.line(), l.column()));
        let msg = format_panic_message(info.payload(), location);
        log_problem(&msg);
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_panic_payload_str() {
        let payload: &str = "something went wrong";
        let msg = format_panic_payload(&payload as &dyn std::any::Any);
        assert_eq!(msg, "something went wrong");
    }

    #[test]
    fn format_panic_payload_string() {
        let payload = String::from("owned error message");
        let msg = format_panic_payload(&payload as &dyn std::any::Any);
        assert_eq!(msg, "owned error message");
    }

    #[test]
    fn format_panic_payload_unknown_type() {
        let payload: i32 = 42;
        let msg = format_panic_payload(&payload as &dyn std::any::Any);
        assert_eq!(msg, "unknown panic");
    }

    #[test]
    fn format_panic_message_with_location() {
        let payload: &str = "index out of bounds";
        let msg = format_panic_message(
            &payload as &dyn std::any::Any,
            Some(("src/editor/widget.rs", 1603, 37)),
        );
        assert_eq!(
            msg,
            "Panic at src/editor/widget.rs:1603:37: index out of bounds"
        );
    }

    #[test]
    fn format_panic_message_without_location() {
        let payload = String::from("something failed");
        let msg = format_panic_message(&payload as &dyn std::any::Any, None);
        assert_eq!(msg, "Panic: something failed");
    }

    #[test]
    fn format_panic_message_unknown_payload_with_location() {
        let payload: f64 = 3.14;
        let msg = format_panic_message(&payload as &dyn std::any::Any, Some(("main.rs", 1, 1)));
        assert_eq!(msg, "Panic at main.rs:1:1: unknown panic");
    }
}
