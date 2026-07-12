//! Per-pass repaint diagnostics for stutter triage.
//!
//! Off by default; enable with `RUST_LOG=rust_pad_ui::app=trace` on a
//! release build. Each UI pass emits one TRACE line carrying the
//! frame-to-frame gap, the pass duration, and egui's repaint causes: the
//! `file:line` of every `request_repaint*` call recorded during the
//! PREVIOUS pass (egui swaps the cause list at pass begin), or `os-event`
//! when that pass recorded none (the current pass then ran for an OS
//! event or timer). A visible stutter reads directly from the log: a
//! settling pass that arrives hundreds of ms after input shows which
//! site requested it, a large `gap_ms` with no late pass points at
//! event-delivery/present, and a large `pass_ms` points at compute.

use std::time::Instant;

use eframe::egui;

use super::App;

/// Formats egui's repaint causes for the pass-trace log line.
pub(crate) fn format_repaint_causes(causes: &[egui::RepaintCause]) -> String {
    if causes.is_empty() {
        return "os-event".to_owned();
    }
    let mut parts: Vec<String> = causes.iter().map(ToString::to_string).collect();
    parts.sort();
    parts.dedup();
    parts.join(" | ")
}

impl App {
    /// Emits the once-per-pass TRACE diagnostic line and rolls the
    /// pass-timing state forward.
    ///
    /// Cheap when TRACE is filtered out: only the `Instant` bookkeeping
    /// runs; the causes Vec clone and formatting sit behind the
    /// `tracing::enabled!` guard.
    pub(crate) fn emit_pass_trace(&mut self, ctx: &egui::Context, pass_start: Instant) {
        let gap_ms = self
            .last_pass_start
            .map_or(0.0, |prev| (pass_start - prev).as_secs_f64() * 1000.0);
        self.last_pass_start = Some(pass_start);
        if !tracing::enabled!(tracing::Level::TRACE) {
            return;
        }
        let pass_ms = pass_start.elapsed().as_secs_f64() * 1000.0;
        let causes = format_repaint_causes(&ctx.repaint_causes());
        tracing::trace!(gap_ms, pass_ms, causes, "pass");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use super::*;

    #[test]
    fn empty_causes_mean_os_event() {
        assert_eq!(format_repaint_causes(&[]), "os-event");
    }

    /// Minimal subscriber: counts events, gated at `max_level`. Implemented
    /// by hand so the crate needs no tracing-subscriber dev-dependency.
    struct LevelGatedCounter {
        max_level: tracing::Level,
        events: Arc<AtomicUsize>,
    }

    impl tracing::Subscriber for LevelGatedCounter {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() <= self.max_level
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, _: &tracing::Event<'_>) {
            self.events.fetch_add(1, Ordering::SeqCst);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    fn emit_with_max_level(level: tracing::Level) -> usize {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let mut app = super::super::tests::test_app();
        let events = Arc::new(AtomicUsize::new(0));
        let subscriber = LevelGatedCounter {
            max_level: level,
            events: Arc::clone(&events),
        };
        tracing::subscriber::with_default(subscriber, || {
            app.emit_pass_trace(&ctx, Instant::now());
        });
        assert!(
            app.last_pass_start.is_some(),
            "pass bookkeeping must advance regardless of the filter"
        );
        events.load(Ordering::SeqCst)
    }

    #[test]
    fn emit_pass_trace_emits_exactly_one_line_at_trace() {
        assert_eq!(emit_with_max_level(tracing::Level::TRACE), 1);
    }

    #[test]
    fn emit_pass_trace_is_silent_at_info() {
        assert_eq!(
            emit_with_max_level(tracing::Level::INFO),
            0,
            "the enabled! guard must suppress the line (and its formatting work) at INFO"
        );
    }

    #[test]
    fn causes_are_formatted_sorted_and_deduped() {
        // Real causes recorded by egui itself: request twice from this file
        // (same site → dedupes to one entry carrying this file's name).
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            for _ in 0..2 {
                ui.ctx().request_repaint();
            }
        });
        // `repaint_causes` reports the PREVIOUS pass's requests; run one
        // silent pass so the requesting pass becomes the previous one.
        let _ = ctx.run_ui(egui::RawInput::default(), |_ui| {});
        let causes = ctx.repaint_causes();
        assert!(!causes.is_empty(), "request_repaint must record a cause");
        let formatted = format_repaint_causes(&causes);
        assert!(
            formatted.contains("pass_trace.rs"),
            "cause should carry this call site, got: {formatted}"
        );
        // Same site requested twice must not repeat in the line.
        assert_eq!(formatted.matches("pass_trace.rs").count(), 1);
    }
}
