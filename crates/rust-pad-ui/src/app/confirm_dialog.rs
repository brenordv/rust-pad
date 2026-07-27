//! Shared keyboard-navigable choice row for the confirm dialogs.
//!
//! Both the "unsaved changes" close dialog and the reload dialog present a row
//! of mutually exclusive choices. This module renders those choices as a
//! non-focusable row driven entirely by an external focus index: arrow keys
//! move the highlight and Space/Enter activate it, without egui's built-in
//! widget focus and Space/Enter activation running in parallel with the custom
//! model.

use eframe::egui;
use egui::{FontId, Key, Sense, Stroke, StrokeKind};

use super::resolved_theme::ChromeTheme;

/// Direction the highlight moves within a choice row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NavDir {
    Prev,
    Next,
}

/// Horizontal padding inside each choice button.
const PAD_X: f32 = 12.0;
/// Vertical padding inside each choice button.
const PAD_Y: f32 = 6.0;
/// Gap between choice buttons.
const GAP: f32 = 8.0;

/// Moves a focus index one step in `dir`, clamped to `0..len` (no wrap).
///
/// Returns `current` unchanged when `len == 0`.
fn step_choice(current: usize, len: usize, dir: NavDir) -> usize {
    if len == 0 {
        return current;
    }
    let last = len - 1;
    let cur = current.min(last);
    match dir {
        NavDir::Prev => cur.saturating_sub(1),
        NavDir::Next => (cur + 1).min(last),
    }
}

/// Renders a keyboard-navigable row of mutually exclusive choices.
///
/// `focus` is the externally-owned highlight index (the caller persists it
/// across frames). Left/Up move the highlight to the previous choice, Right/Down
/// to the next (clamped, no wrap), Home/End jump to the ends; Space/Enter
/// activate the highlighted choice; a click activates the clicked choice.
/// Returns the activated index, if any.
///
/// The choices are non-focusable (`Sense::CLICK`) and all keys are consumed via
/// `input_mut`, so egui's built-in focus navigation and Space/Enter activation
/// cannot fire in parallel. Escape is intentionally NOT consumed here; the
/// application's global shortcut handler owns dialog dismissal.
pub(crate) fn render_choice_row(
    ui: &mut egui::Ui,
    labels: &[&str],
    focus: &mut usize,
    chrome_theme: &ChromeTheme,
) -> Option<usize> {
    if labels.is_empty() {
        return None;
    }
    *focus = (*focus).min(labels.len() - 1);

    ui.input_mut(|i| {
        if i.consume_key(egui::Modifiers::NONE, Key::ArrowLeft)
            || i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)
        {
            *focus = step_choice(*focus, labels.len(), NavDir::Prev);
        }
        if i.consume_key(egui::Modifiers::NONE, Key::ArrowRight)
            || i.consume_key(egui::Modifiers::NONE, Key::ArrowDown)
        {
            *focus = step_choice(*focus, labels.len(), NavDir::Next);
        }
        if i.consume_key(egui::Modifiers::NONE, Key::Home) {
            *focus = 0;
        }
        if i.consume_key(egui::Modifiers::NONE, Key::End) {
            *focus = labels.len() - 1;
        }
    });

    let activate_focused = ui.input_mut(|i| {
        i.consume_key(egui::Modifiers::NONE, Key::Enter)
            || i.consume_key(egui::Modifiers::NONE, Key::Space)
    });
    let mut activated = if activate_focused { Some(*focus) } else { None };

    let font = FontId::proportional(13.0);
    let text_color = ui.visuals().text_color();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (idx, label) in labels.iter().enumerate() {
            let focused = idx == *focus;
            let label_color = if focused {
                chrome_theme.on_accent
            } else {
                text_color
            };
            let galley =
                ui.painter()
                    .layout_no_wrap((*label).to_owned(), font.clone(), label_color);
            let size = egui::vec2(galley.size().x + 2.0 * PAD_X, galley.size().y + 2.0 * PAD_Y);
            let (rect, response) = ui.allocate_exact_size(size, Sense::CLICK);
            let radius = ui.visuals().widgets.inactive.corner_radius;
            let painter = ui.painter();
            if focused {
                painter.rect_filled(rect, radius, chrome_theme.accent);
            } else {
                painter.rect_filled(rect, radius, chrome_theme.button_bg);
                painter.rect_stroke(
                    rect,
                    radius,
                    Stroke::new(1.0, chrome_theme.border),
                    StrokeKind::Inside,
                );
                if response.hovered() {
                    painter.rect_stroke(
                        rect,
                        radius,
                        Stroke::new(1.0, chrome_theme.accent),
                        StrokeKind::Inside,
                    );
                }
            }
            let text_pos = rect.center() - galley.size() * 0.5;
            painter.galley(text_pos, galley, label_color);
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, (*label).to_owned())
            });
            if response.clicked() {
                activated = Some(idx);
            }
        }
    });

    activated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_choice_clamps_without_wrap() {
        assert_eq!(step_choice(0, 3, NavDir::Prev), 0, "prev at start clamps");
        assert_eq!(step_choice(0, 3, NavDir::Next), 1);
        assert_eq!(step_choice(1, 3, NavDir::Next), 2);
        assert_eq!(step_choice(2, 3, NavDir::Next), 2, "next at end clamps");
        assert_eq!(step_choice(2, 3, NavDir::Prev), 1);
        // An out-of-range current is clamped to the last index first.
        assert_eq!(step_choice(9, 3, NavDir::Prev), 1);
        assert_eq!(step_choice(0, 0, NavDir::Next), 0, "empty is a no-op");
    }

    fn key_event(k: egui::Key) -> egui::Event {
        egui::Event::Key {
            key: k,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn frame(
        ctx: &egui::Context,
        labels: &[&str],
        focus: &mut usize,
        events: Vec<egui::Event>,
    ) -> Option<usize> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(400.0, 200.0),
            )),
            events,
            ..Default::default()
        };
        let theme = ChromeTheme::default();
        let mut out = None;
        let _ = ctx.run_ui(raw, |ui| {
            out = render_choice_row(ui, labels, focus, &theme);
        });
        out
    }

    #[test]
    fn arrow_moves_focus_and_enter_activates() {
        let ctx = egui::Context::default();
        let labels = ["Save & Close", "Discard", "Cancel"];
        let mut focus = 0usize;

        // Layout frame so widgets exist.
        assert_eq!(frame(&ctx, &labels, &mut focus, Vec::new()), None);
        // ArrowRight moves focus 0 -> 1 without activating.
        assert_eq!(
            frame(
                &ctx,
                &labels,
                &mut focus,
                vec![key_event(egui::Key::ArrowRight)]
            ),
            None
        );
        assert_eq!(focus, 1);
        // Enter activates the highlighted choice.
        assert_eq!(
            frame(&ctx, &labels, &mut focus, vec![key_event(egui::Key::Enter)]),
            Some(1)
        );
    }

    #[test]
    fn end_home_jump_to_bounds_and_space_activates() {
        let ctx = egui::Context::default();
        let labels = ["Reload", "Cancel"];
        let mut focus = 0usize;

        assert_eq!(frame(&ctx, &labels, &mut focus, Vec::new()), None);
        assert_eq!(
            frame(&ctx, &labels, &mut focus, vec![key_event(egui::Key::End)]),
            None
        );
        assert_eq!(focus, 1);
        // Space activates like Enter.
        assert_eq!(
            frame(&ctx, &labels, &mut focus, vec![key_event(egui::Key::Space)]),
            Some(1)
        );
    }
}
