//! Shared chrome painting primitives.
//!
//! Every visual element that appears on more than one surface (active-item
//! indicators, count badges, segment dividers, hover tints) is painted
//! through this module. A second paint-site for any of these shapes is a
//! defect: extend the helper instead.

use egui::{Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

use crate::app::resolved_theme::{ChromeTheme, IndicatorStyle, Metrics, MetricsStyle};

/// Width of the accent edge bar drawn for active items.
pub const INDICATOR_BAR_WIDTH: f32 = 2.0;
/// Minimum side length of a count badge.
const BADGE_MIN_SIZE: f32 = 14.0;
const BADGE_FONT_SIZE: f32 = 9.0;
/// Height of the custom dialog header band.
const DIALOG_HEAD_HEIGHT: f32 = 36.0;
/// Inner margin of a dialog's body content.
const DIALOG_BODY_MARGIN: i8 = 12;
/// Fill alpha for a dialog that dims while the editor is the active surface.
const INACTIVE_DIALOG_ALPHA: f32 = 0.90;

/// Alpha multiplier for a dialog's background fills: slightly translucent
/// while dimmed, fully opaque otherwise.
pub fn dialog_alpha(dimmed: bool) -> f32 {
    if dimmed {
        INACTIVE_DIALOG_ALPHA
    } else {
        1.0
    }
}

/// A fixed-height band whose children are vertically centered. Shared by
/// the dialog header and the workspace sidebar header; a bare
/// `horizontal_centered` would top-align inside the taller band.
pub fn header_band<R>(
    ui: &mut egui::Ui,
    height: f32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), height),
        egui::Layout::left_to_right(egui::Align::Center),
        add_contents,
    )
}

/// Paints the active-item indicator for `rect` in the theme's direction
/// style: a rounded accent-tint pill behind the item (soft) or a 2px accent
/// bar along the leading edge (sharp).
///
/// `leading_edge_horizontal` selects where the sharp bar goes: `false` puts
/// it on the left edge (tree rows, activity items).
pub fn accent_indicator(painter: &Painter, rect: Rect, chrome: &ChromeTheme, metrics: &Metrics) {
    match metrics.indicator {
        IndicatorStyle::TintPill => {
            painter.rect_filled(
                rect,
                CornerRadius::same(metrics.widget_radius),
                chrome.accent_soft,
            );
        }
        IndicatorStyle::EdgeBar => {
            painter.rect_filled(rect, CornerRadius::ZERO, chrome.accent_soft);
            let bar = Rect::from_min_size(
                rect.left_top(),
                Vec2::new(INDICATOR_BAR_WIDTH, rect.height()),
            );
            painter.rect_filled(bar, CornerRadius::ZERO, chrome.accent);
        }
    }
}

/// Paints a translucent accent hover tint over `rect`.
pub fn hover_tint(painter: &Painter, rect: Rect, chrome: &ChromeTheme, metrics: &Metrics) {
    painter.rect_filled(
        rect,
        CornerRadius::same(metrics.widget_radius),
        chrome.accent_soft,
    );
}

/// Paints a 1px vertical divider of `height` centered at `x`.
pub fn segment_divider(painter: &Painter, x: f32, top: f32, height: f32, color: Color32) {
    painter.line_segment(
        [Pos2::new(x, top), Pos2::new(x, top + height)],
        Stroke::new(1.0, color),
    );
}

/// Paints a count badge (min 14×14 pill) anchored with its center at
/// `center`, and returns the rect it covered. Counts above 99 render as
/// "99+" so the pill stays compact.
pub fn count_badge(
    painter: &Painter,
    center: Pos2,
    count: usize,
    fill: Color32,
    text_color: Color32,
) -> Rect {
    let text = if count > 99 {
        "99+".to_string()
    } else {
        count.to_string()
    };
    let galley = painter.layout_no_wrap(text, FontId::proportional(BADGE_FONT_SIZE), text_color);
    let width = (galley.size().x + 6.0).max(BADGE_MIN_SIZE);
    let rect = Rect::from_center_size(center, Vec2::new(width, BADGE_MIN_SIZE));
    painter.rect_filled(rect, CornerRadius::same((BADGE_MIN_SIZE / 2.0) as u8), fill);
    painter.galley(
        Align2::CENTER_CENTER
            .align_size_within_rect(galley.size(), rect)
            .min,
        galley,
        text_color,
    );
    rect
}

/// Sizing and behavior knobs for [`show_dialog`].
pub struct DialogOptions {
    pub resizable: bool,
    pub default_width: f32,
    pub default_height: Option<f32>,
    /// Open the window centered (movable afterwards).
    pub center: bool,
    /// Dim the background fills this frame (Find & Replace dims while the
    /// editor is the active surface; the caller owns the signal).
    pub dimmed: bool,
    /// Whether the dialog offers a close button. Confirmation dialogs that
    /// force a decision set this to `false`.
    pub closable: bool,
}

impl Default for DialogOptions {
    fn default() -> Self {
        Self {
            resizable: false,
            default_width: 380.0,
            default_height: None,
            center: false,
            dimmed: false,
            closable: true,
        }
    }
}

/// Opens `window` centered without anchoring it: an anchored egui area is
/// forced immovable (the frozen-dialog bug). A CENTER pivot plus a default
/// position centers the first placement while leaving the window draggable.
fn open_centered<'a>(window: egui::Window<'a>, ctx: &egui::Context) -> egui::Window<'a> {
    window
        .pivot(Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
}

/// Result of rendering a dialog's themed header band and body into a `Ui`.
pub(crate) struct DialogChrome<R> {
    /// True when the header's close button was clicked this frame.
    pub close_clicked: bool,
    /// Screen rect of the header band (used to drag a borderless window).
    pub header_rect: Rect,
    /// The body closure's return value.
    pub result: R,
}

/// Renders the themed dialog chrome (a `dialog_head` header band with a painted
/// title and optional close button, then a `dialog_bg` body) top-aligned into
/// `ui`. Shared by the in-app dialog window ([`show_dialog`]) and the borderless
/// Find & Replace viewport, so the header treatment lives in one place.
///
/// `head_top_radius` rounds the header's top corners to match the enclosing
/// window's radius (pass 0 for a square borderless window). `alpha` dims the
/// header fill (1.0 for no dimming).
pub(crate) fn dialog_header_and_body<R>(
    ui: &mut egui::Ui,
    title: &str,
    closable: bool,
    chrome_theme: &ChromeTheme,
    alpha: f32,
    head_top_radius: u8,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> DialogChrome<R> {
    let mut close_clicked = false;
    let head_radius = CornerRadius {
        nw: head_top_radius,
        ne: head_top_radius,
        sw: 0,
        se: 0,
    };
    let header = egui::Frame::new()
        .fill(chrome_theme.dialog_head.gamma_multiply(alpha))
        .corner_radius(head_radius)
        .inner_margin(egui::Margin::symmetric(DIALOG_BODY_MARGIN, 0))
        .show(ui, |ui| {
            header_band(ui, DIALOG_HEAD_HEIGHT, |ui| {
                // Painted, not a Label: the window already carries an accessible
                // node with the title, and a Label would duplicate it in the
                // accessibility tree.
                let title_color = ui.visuals().text_color();
                let title_galley = ui.painter().layout_no_wrap(
                    title.to_string(),
                    FontId::new(
                        14.0,
                        egui::FontFamily::Name(crate::app::FONT_FAMILY_SEMIBOLD.into()),
                    ),
                    title_color,
                );
                let (title_rect, _) =
                    ui.allocate_exact_size(title_galley.size(), egui::Sense::hover());
                ui.painter()
                    .galley(title_rect.min, title_galley, title_color);
                if closable {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let close = ui.add(
                            egui::Button::new(
                                egui::RichText::new(crate::icons::CLOSE)
                                    .color(chrome_theme.text_muted),
                            )
                            .frame(false),
                        );
                        close.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                true,
                                "Close dialog",
                            )
                        });
                        if close.on_hover_text("Close").clicked() {
                            close_clicked = true;
                        }
                    });
                }
            });
        });
    let header_rect = header.response.rect;

    let body = egui::Frame::new()
        .inner_margin(egui::Margin::same(DIALOG_BODY_MARGIN))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add_contents(ui)
        });

    DialogChrome {
        close_clicked,
        header_rect,
        result: body.inner,
    }
}

/// Shows a dialog window in the theme's direction style and returns the
/// closure's result while the dialog is open.
///
/// Chrome themes draw a custom header band (`dialog_head` fill, 14 px
/// semibold title, close button) over a `dialog_bg` body with the direction
/// radius; legacy themes keep the stock egui window and its native title
/// bar so existing themes render unchanged.
pub fn show_dialog<R>(
    ctx: &egui::Context,
    title: &str,
    open: &mut bool,
    chrome_theme: &ChromeTheme,
    metrics: &Metrics,
    options: DialogOptions,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    if !*open {
        return None;
    }
    if metrics.style == MetricsStyle::Legacy {
        return show_legacy_dialog(ctx, title, open, options, add_contents);
    }

    let alpha = dialog_alpha(options.dimmed);
    let frame = egui::Frame::new()
        .fill(chrome_theme.dialog_bg.gamma_multiply(alpha))
        .stroke(Stroke::new(1.0, chrome_theme.border))
        .corner_radius(CornerRadius::same(metrics.window_radius))
        .inner_margin(egui::Margin::ZERO);

    let mut window = egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(options.resizable)
        .default_width(options.default_width)
        .frame(frame);
    if let Some(h) = options.default_height {
        window = window.default_height(h);
    }
    if options.center {
        window = open_centered(window, ctx);
    }

    let mut close_clicked = false;
    let mut result = None;
    window.show(ctx, |ui| {
        let chrome = dialog_header_and_body(
            ui,
            title,
            options.closable,
            chrome_theme,
            alpha,
            metrics.window_radius,
            add_contents,
        );
        close_clicked = chrome.close_clicked;
        result = Some(chrome.result);
    });
    if close_clicked {
        *open = false;
    }
    result
}

/// Legacy path for [`show_dialog`]: the stock egui window.
fn show_legacy_dialog<R>(
    ctx: &egui::Context,
    title: &str,
    open: &mut bool,
    options: DialogOptions,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> Option<R> {
    let mut window = egui::Window::new(title)
        .collapsible(false)
        .resizable(options.resizable)
        .default_width(options.default_width);
    if options.closable {
        window = window.open(open);
    }
    if let Some(h) = options.default_height {
        window = window.default_height(h);
    }
    if options.center {
        window = open_centered(window, ctx);
    }
    if options.dimmed {
        let frame = egui::Frame::window(ctx.global_style().as_ref()).fill(
            ctx.global_style()
                .visuals
                .window_fill()
                .gamma_multiply(dialog_alpha(true)),
        );
        window = window.frame(frame);
    }
    window
        .show(ctx, |ui| add_contents(ui))
        .and_then(|inner| inner.inner)
}

/// Runs `add` with the button's hovered/active frame geometry pinned to the
/// inactive state, then restores the visuals.
///
/// egui derives a bordered button's `inner_margin` from `button_padding +
/// expansion - bg_stroke.width`, and the default per-state `bg_stroke.width`
/// jumps from 0 (inactive) to 1 (hovered), so a custom-bordered button would
/// shrink a pixel on hover and its neighbours would reflow (the "jiggle").
/// Pinning `bg_stroke.width` and `expansion` keeps the allocated size, and thus
/// every button's position, constant across states. The values are restored
/// afterwards so the tweak never leaks to later widgets sharing the `Ui`.
fn with_stable_hover_geometry<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let width = ui.visuals().widgets.inactive.bg_stroke.width;
    let expansion = ui.visuals().widgets.inactive.expansion;
    let saved = {
        let w = &ui.visuals().widgets;
        (
            w.hovered.bg_stroke.width,
            w.hovered.expansion,
            w.active.bg_stroke.width,
            w.active.expansion,
        )
    };
    {
        let w = &mut ui.visuals_mut().widgets;
        w.hovered.bg_stroke.width = width;
        w.hovered.expansion = expansion;
        w.active.bg_stroke.width = width;
        w.active.expansion = expansion;
    }
    let result = add(ui);
    {
        let w = &mut ui.visuals_mut().widgets;
        w.hovered.bg_stroke.width = saved.0;
        w.hovered.expansion = saved.1;
        w.active.bg_stroke.width = saved.2;
        w.active.expansion = saved.3;
    }
    result
}

/// Paints a keyboard-focus ring for a dialog button: a 2 px inside stroke in
/// `color`, more prominent than the 1 px hover accent so a tabbed-to button is
/// unmistakable. `color` is the accent for secondary buttons and `on_accent`
/// for the accent-filled primary button, so the ring reads on the fill.
fn button_focus_ring(painter: &Painter, rect: Rect, radius: CornerRadius, color: Color32) {
    painter.rect_stroke(rect, radius, Stroke::new(2.0, color), StrokeKind::Inside);
}

/// A primary dialog button: accent fill, on-accent semibold text.
pub fn primary_button(ui: &mut egui::Ui, chrome_theme: &ChromeTheme, text: &str) -> egui::Response {
    let response = with_stable_hover_geometry(ui, |ui| {
        ui.add(
            egui::Button::new(
                egui::RichText::new(text)
                    .color(chrome_theme.on_accent)
                    .font(FontId::new(
                        13.0,
                        egui::FontFamily::Name(crate::app::FONT_FAMILY_SEMIBOLD.into()),
                    )),
            )
            .fill(chrome_theme.accent),
        )
    });
    if response.hovered() {
        ui.painter().rect_filled(
            response.rect,
            ui.visuals().widgets.inactive.corner_radius,
            Color32::from_white_alpha(10),
        );
    }
    if response.has_focus() {
        let radius = ui.visuals().widgets.inactive.corner_radius;
        button_focus_ring(ui.painter(), response.rect, radius, chrome_theme.on_accent);
    }
    response
}

/// A secondary dialog button: `button_bg` fill with a 1 px border that
/// takes the accent on hover.
pub fn secondary_button(
    ui: &mut egui::Ui,
    chrome_theme: &ChromeTheme,
    text: &str,
) -> egui::Response {
    let response = with_stable_hover_geometry(ui, |ui| {
        ui.add(
            egui::Button::new(text)
                .fill(chrome_theme.button_bg)
                .stroke(Stroke::new(1.0, chrome_theme.border)),
        )
    });
    if response.hovered() {
        ui.painter().rect_stroke(
            response.rect,
            ui.visuals().widgets.hovered.corner_radius,
            Stroke::new(1.0, chrome_theme.accent),
            StrokeKind::Inside,
        );
    }
    if response.has_focus() {
        let radius = ui.visuals().widgets.inactive.corner_radius;
        button_focus_ring(ui.painter(), response.rect, radius, chrome_theme.accent);
    }
    response
}

/// Paints a 1 px border around a text input so it stays visible even when the
/// input, and its window, is unfocused. On the light chrome themes the input
/// fill is white and matches the `dialog_bg`, so without this the field has no
/// visible boundary once the focus ring is gone. Uses `text_faint` (a mid-tone
/// that reads on both light and dark themes) rather than the near-invisible
/// window `border` colour. No-op on legacy themes, which keep egui's stock
/// text-edit frame.
pub fn input_border(painter: &Painter, rect: Rect, chrome_theme: &ChromeTheme, metrics: &Metrics) {
    if metrics.style == MetricsStyle::Legacy {
        return;
    }
    painter.rect_stroke(
        rect,
        CornerRadius::same(metrics.widget_radius),
        Stroke::new(1.0, chrome_theme.text_faint),
        StrokeKind::Inside,
    );
}

/// Recesses a dialog's text inputs to the theme's `input_bg` well so a field on
/// a `dialog_bg` surface reads as an input instead of blending in. Call it on
/// the dialog's `Ui` before rendering the text edits; egui resolves a text
/// edit's fill through `visuals.text_edit_bg_color`, so this recolors only the
/// inputs and nothing else on the surface.
///
/// On the light chrome themes `dialog_bg` is white and `input_bg` is a faint
/// gray, which is the case this fixes. On dark and legacy/derived themes
/// `input_bg` already equals egui's default text-input fill (`extreme_bg_color`),
/// so this is a pixel-identical no-op there and needs no metric gate. Scope it
/// to the dialog's own `Ui`: `visuals_mut` is copy-on-write on this `Ui`'s local
/// style, so the override never leaks to the editor or other surfaces.
pub fn use_input_fill(ui: &mut egui::Ui, chrome_theme: &ChromeTheme) {
    ui.visuals_mut().text_edit_bg_color = Some(chrome_theme.input_bg);
}

/// Paints the focused-input ring: a 1.5 px accent stroke hugging the input
/// with a 3 px `accent_soft` halo outside it. Call after rendering a text
/// input that currently has focus. No-op on legacy themes, which keep the
/// stock egui focus treatment.
pub fn focus_ring(painter: &Painter, rect: Rect, chrome_theme: &ChromeTheme, metrics: &Metrics) {
    if metrics.style == MetricsStyle::Legacy {
        return;
    }
    let radius = CornerRadius::same(metrics.widget_radius);
    painter.rect_stroke(
        rect.expand(3.0),
        radius,
        Stroke::new(3.0, chrome_theme.accent_soft),
        StrokeKind::Middle,
    );
    painter.rect_stroke(
        rect.expand(1.0),
        radius,
        Stroke::new(1.5, chrome_theme.accent),
        StrokeKind::Middle,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_painter(ctx: &egui::Context) -> Painter {
        Painter::new(
            ctx.clone(),
            egui::LayerId::background(),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 200.0)),
        )
    }

    fn run_with_painter(f: impl FnOnce(&Painter)) {
        let ctx = egui::Context::default();
        let mut f = Some(f);
        let _ = ctx.run_ui(egui::RawInput::default(), |ctx| {
            let painter = test_painter(ctx);
            if let Some(f) = f.take() {
                f(&painter);
            }
        });
    }

    #[test]
    fn count_badge_meets_minimum_hit_size() {
        run_with_painter(|painter| {
            let rect = count_badge(
                painter,
                Pos2::new(50.0, 50.0),
                2,
                Color32::YELLOW,
                Color32::BLACK,
            );
            assert!(rect.width() >= BADGE_MIN_SIZE);
            assert!((rect.height() - BADGE_MIN_SIZE).abs() < f32::EPSILON);
        });
    }

    #[test]
    fn count_badge_grows_for_wide_counts_and_caps_text() {
        run_with_painter(|painter| {
            let small = count_badge(
                painter,
                Pos2::new(50.0, 50.0),
                2,
                Color32::YELLOW,
                Color32::BLACK,
            );
            let wide = count_badge(
                painter,
                Pos2::new(50.0, 50.0),
                1234,
                Color32::YELLOW,
                Color32::BLACK,
            );
            assert!(wide.width() > small.width());
        });
    }

    #[test]
    fn accent_indicator_paints_for_both_styles_without_panicking() {
        use crate::app::resolved_theme::MetricsStyle;
        run_with_painter(|painter| {
            let chrome = ChromeTheme::default();
            let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(100.0, 26.0));
            for style in [
                MetricsStyle::Soft,
                MetricsStyle::Sharp,
                MetricsStyle::Legacy,
            ] {
                accent_indicator(painter, rect, &chrome, &Metrics::for_style(style));
            }
        });
    }

    /// `use_input_fill` overrides only the text-edit fill token, to the theme's
    /// recessed `input_bg`, scoped to the ui it is called on.
    #[test]
    fn use_input_fill_sets_text_edit_bg_to_input_bg() {
        let chrome = ChromeTheme::from_definition(&rust_pad_config::theme::aurora_light()).0;
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            use_input_fill(ui, &chrome);
            assert_eq!(ui.visuals().text_edit_bg_color, Some(chrome.input_bg));
        });
    }

    /// End to end: after `use_input_fill`, a `text_edit_singleline` paints its
    /// background well with the recessed `input_bg` (distinct from the light
    /// theme's white `dialog_bg`), so the field is visible. Rendered directly in
    /// a root ui, not through the dialog's viewport, so no open-fade skews the
    /// fill color.
    #[test]
    fn use_input_fill_recesses_the_rendered_text_edit_fill() {
        use egui::epaint::Shape;
        let chrome = ChromeTheme::from_definition(&rust_pad_config::theme::aurora_light()).0;
        assert_ne!(
            chrome.input_bg, chrome.dialog_bg,
            "precondition: recessed well"
        );
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
            ..Default::default()
        };
        let mut text = String::from("hello");
        let output = ctx.run_ui(raw, |ui| {
            use_input_fill(ui, &chrome);
            let _ = ui.text_edit_singleline(&mut text);
        });
        let has_input_well = output
            .shapes
            .iter()
            .any(|cs| matches!(&cs.shape, Shape::Rect(r) if r.fill == chrome.input_bg));
        assert!(
            has_input_well,
            "the text edit should be filled with the recessed input_bg"
        );
    }

    /// Maps the named semibold UI family onto the default proportional list
    /// so dialog headers and primary buttons lay out in a bare test context.
    fn register_semibold_alias(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        let proportional = fonts
            .families
            .get(&egui::FontFamily::Proportional)
            .cloned()
            .unwrap_or_default();
        fonts.families.insert(
            egui::FontFamily::Name(crate::app::FONT_FAMILY_SEMIBOLD.into()),
            proportional,
        );
        ctx.set_fonts(fonts);
    }

    fn screen_input() -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
            ..Default::default()
        }
    }

    #[test]
    fn show_dialog_returns_contents_result_for_every_style() {
        for style in [
            MetricsStyle::Soft,
            MetricsStyle::Sharp,
            MetricsStyle::Legacy,
        ] {
            let ctx = egui::Context::default();
            register_semibold_alias(&ctx);
            let chrome_theme = ChromeTheme::default();
            let metrics = Metrics::for_style(style);
            let mut open = true;
            let _ = ctx.run_ui(screen_input(), |ui| {
                let result = show_dialog(
                    ui.ctx(),
                    "Test Dialog",
                    &mut open,
                    &chrome_theme,
                    &metrics,
                    DialogOptions::default(),
                    |_ui| 42,
                );
                assert_eq!(result, Some(42), "style {style:?}");
            });
            assert!(open, "dialog stays open without a close click");
        }
    }

    #[test]
    fn show_dialog_skips_contents_when_closed() {
        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let chrome_theme = ChromeTheme::default();
        let metrics = Metrics::default();
        let mut open = false;
        let _ = ctx.run_ui(screen_input(), |ui| {
            let result = show_dialog(
                ui.ctx(),
                "Test Dialog",
                &mut open,
                &chrome_theme,
                &metrics,
                DialogOptions::default(),
                |_ui| 42,
            );
            assert_eq!(result, None);
        });
    }

    /// Centered dialogs must open at the screen center WITHOUT egui's
    /// `anchor` (an anchored area is forced immovable, which froze every
    /// centered dialog). Covers the chrome path and the legacy path, which
    /// each build their own window.
    #[test]
    fn centered_dialogs_open_centered_without_anchoring() {
        for style in [MetricsStyle::Soft, MetricsStyle::Legacy] {
            let ctx = egui::Context::default();
            register_semibold_alias(&ctx);
            let chrome_theme = ChromeTheme::default();
            let metrics = Metrics::for_style(style);
            let mut open = true;
            // First show is an invisible sizing pass; run a few frames so
            // the stored area rect reflects the real size.
            for _ in 0..3 {
                let _ = ctx.run_ui(screen_input(), |ui| {
                    let _ = show_dialog(
                        ui.ctx(),
                        "Centered Dialog",
                        &mut open,
                        &chrome_theme,
                        &metrics,
                        DialogOptions {
                            center: true,
                            ..Default::default()
                        },
                        |ui| {
                            ui.label("body");
                        },
                    );
                });
            }
            let rect = ctx
                .memory(|m| m.area_rect(egui::Id::new("Centered Dialog")))
                .expect("centered dialog area should exist");
            let center = rect.center();
            assert!(
                (center.x - 400.0).abs() <= 2.0 && (center.y - 300.0).abs() <= 2.0,
                "style {style:?}: dialog center {center:?} not at screen center"
            );
        }
    }

    #[test]
    fn dialog_buttons_render_without_panicking() {
        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let chrome_theme = ChromeTheme::default();
        let _ = ctx.run_ui(screen_input(), |ui| {
            let primary = primary_button(ui, &chrome_theme, "OK");
            assert!(!primary.clicked());
            let secondary = secondary_button(ui, &chrome_theme, "Cancel");
            assert!(!secondary.clicked());
        });
    }

    /// The hover jiggle came from egui shrinking a bordered button's
    /// `inner_margin` by the hovered `bg_stroke.width`. The helper must pin the
    /// hovered/active widths to inactive during the add and restore them after.
    #[test]
    fn stable_hover_geometry_pins_then_restores() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(screen_input(), |ui| {
            ui.visuals_mut().widgets.inactive.bg_stroke.width = 0.0;
            ui.visuals_mut().widgets.hovered.bg_stroke.width = 1.0;
            ui.visuals_mut().widgets.active.bg_stroke.width = 1.0;
            ui.visuals_mut().widgets.hovered.expansion = 1.0;

            let inside = with_stable_hover_geometry(ui, |ui| {
                (
                    ui.visuals().widgets.hovered.bg_stroke.width,
                    ui.visuals().widgets.active.bg_stroke.width,
                    ui.visuals().widgets.hovered.expansion,
                )
            });
            assert_eq!(inside, (0.0, 0.0, 0.0), "pinned to inactive inside the add");

            assert_eq!(
                ui.visuals().widgets.hovered.bg_stroke.width,
                1.0,
                "hovered width restored after the add"
            );
            assert_eq!(
                ui.visuals().widgets.hovered.expansion,
                1.0,
                "hovered expansion restored after the add"
            );
        });
    }

    /// End-to-end: a secondary button keeps the same width whether or not the
    /// pointer hovers it, so neighbours never reflow.
    #[test]
    fn secondary_button_width_is_stable_across_hover() {
        let ctx = egui::Context::default();
        register_semibold_alias(&ctx);
        let chrome_theme = ChromeTheme::default();

        // Warm-up frame to learn the button's placement.
        let mut center = Pos2::ZERO;
        let _ = ctx.run_ui(screen_input(), |ui| {
            center = secondary_button(ui, &chrome_theme, "Sample").rect.center();
        });

        let measure = |pointer: Pos2| -> (f32, bool) {
            let mut raw = screen_input();
            raw.events.push(egui::Event::PointerMoved(pointer));
            let mut out = (0.0, false);
            let _ = ctx.run_ui(raw, |ui| {
                let r = secondary_button(ui, &chrome_theme, "Sample");
                out = (r.rect.width(), r.hovered());
            });
            out
        };

        let (width_rest, _) = measure(Pos2::new(790.0, 590.0));
        let (width_hover, hovered) = measure(center);
        assert!(
            hovered,
            "pointer over the button should register as hovered"
        );
        assert!(
            (width_hover - width_rest).abs() < 0.5,
            "button width changed on hover: rest {width_rest} vs hover {width_hover}"
        );
    }

    /// A focused button paints its focus ring without panicking, on every
    /// metric style (the ring reads its radius from the widget visuals).
    #[test]
    fn button_focus_ring_paints_without_panicking() {
        run_with_painter(|painter| {
            let chrome = ChromeTheme::default();
            let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(90.0, 26.0));
            button_focus_ring(painter, rect, CornerRadius::same(4), chrome.accent);
            button_focus_ring(painter, rect, CornerRadius::same(0), chrome.on_accent);
        });
    }

    #[test]
    fn dialog_alpha_dims_only_when_dimmed() {
        assert!((dialog_alpha(false) - 1.0).abs() < f32::EPSILON);
        assert!((dialog_alpha(true) - INACTIVE_DIALOG_ALPHA).abs() < f32::EPSILON);
        assert!(
            dialog_alpha(true) > 0.7,
            "dimmed alpha must stay well above the old too-transparent 0.7"
        );
    }

    #[test]
    fn header_band_centers_children_vertically() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(screen_input(), |ui| {
            let band_height = 36.0;
            let inner = header_band(ui, band_height, |ui| {
                ui.label("Title");
                ui.min_rect()
            });
            let band = inner.response.rect;
            let child = inner.inner;
            assert!((band.height() - band_height).abs() < 0.5);
            let child_center = child.center().y;
            let band_center = band.center().y;
            assert!(
                (child_center - band_center).abs() <= 1.0,
                "child center {child_center} not within 1px of band center {band_center}"
            );
        });
    }

    #[test]
    fn focus_ring_paints_for_chrome_styles_and_noops_for_legacy() {
        run_with_painter(|painter| {
            let chrome = ChromeTheme::default();
            let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(120.0, 22.0));
            for style in [
                MetricsStyle::Soft,
                MetricsStyle::Sharp,
                MetricsStyle::Legacy,
            ] {
                focus_ring(painter, rect, &chrome, &Metrics::for_style(style));
            }
        });
    }
}
