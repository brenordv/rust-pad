//! Print / Export-as-PDF support.
//!
//! The feature delivers "print" via a two-step flow:
//!
//! 1. Generate a PDF from the active document's text using the embedded
//!    DejaVu Sans Mono font.
//! 2. Either (a) write it into the OS temp directory and hand it off to
//!    the system's default PDF viewer (`Print...`), or (b) write it to a
//!    user-picked path via the existing file-save dialog
//!    (`Export as PDF...`).
//!
//! The module is split into four files:
//!
//! - [`layout`] — pure pagination + wrapping. No dependencies on egui or
//!   `printpdf`, fully unit-tested.
//! - [`pdf`] — pure `printpdf` wiring. Also unit-tested.
//! - [`font`] — embedded TTF bytes + glyph advance ratio.
//! - [`job`] — background worker thread that runs the CPU-bound
//!   generation off the UI thread.
//!
//! The app layer lives in [`app`](self::app) and exposes three methods on
//! `App`: `request_print`, `request_export_pdf`, and
//! `handle_print_responses`.

mod app;
pub mod font;
pub mod job;
pub mod layout;
pub mod pdf;

pub use job::PrintWorker;
