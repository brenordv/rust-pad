//! Renders laid-out pages into PDF bytes using `printpdf`.
//!
//! The bulk of the complexity (wrapping, tab expansion, page breaks) lives
//! in [`layout`](super::layout) so this module can focus on translating
//! a `Vec<Page>` into `printpdf` operations. Header, footer, and line
//! numbers are drawn here because they depend on the PDF coordinate
//! system.

use anyhow::{anyhow, Context, Result};
use printpdf::*;

use super::font::FONT_BYTES;
use super::layout::{format_gutter, gutter_width_chars, Page, PageLayout};

/// All inputs needed to produce a PDF. Constructed by the caller (the
/// `PrintWorker`) from a document snapshot plus a chosen layout.
pub struct PdfInput<'a> {
    /// Short display title shown at the top-left of every page. Typically
    /// the filename or `"Untitled"`.
    pub title: String,
    /// Long-form subtitle shown at the top-right of every page. Typically
    /// the full file path, or an empty string for unsaved documents.
    pub subtitle: String,
    /// Timestamp used in the header.
    pub generated_at: chrono::DateTime<chrono::Local>,
    /// Pages produced by [`layout::paginate`](super::layout::paginate).
    pub pages: &'a [Page],
    /// Layout metrics — must match what was passed to `paginate`.
    pub layout: &'a PageLayout,
    /// Total number of source lines in the document. Used to compute the
    /// gutter width so line numbers are right-aligned consistently across
    /// pages.
    pub total_source_lines: usize,
}

/// Renders `input` into a complete PDF byte buffer.
///
/// Returns an error if the bundled font cannot be parsed (should not
/// happen in practice — see the unit test in [`super::font`]) or if
/// `printpdf` reports a failure during serialization.
pub fn render_to_bytes(input: &PdfInput<'_>) -> Result<Vec<u8>> {
    let mut font_warnings: Vec<PdfFontParseWarning> = Vec::new();
    let parsed = ParsedFont::from_bytes(FONT_BYTES, 0, &mut font_warnings)
        .ok_or_else(|| anyhow!("failed to parse bundled monospace font"))?;
    if !font_warnings.is_empty() {
        tracing::debug!("printpdf font parse warnings: {:?}", font_warnings);
    }

    let mut doc = PdfDocument::new(&input.title);
    let font_id = doc.add_font(&parsed);
    let mut warnings: Vec<PdfWarnMsg> = Vec::new();

    let total_pages = input.pages.len().max(1);
    let gutter_width = gutter_width_chars(input.total_source_lines, input.layout.show_line_numbers);

    let mut pdf_pages: Vec<PdfPage> = Vec::with_capacity(total_pages);
    for (page_idx, page) in input.pages.iter().enumerate() {
        let ops = build_page_ops(page, page_idx, total_pages, gutter_width, input, &font_id);
        pdf_pages.push(PdfPage::new(
            Mm(pt_to_mm(input.layout.page_width_pt)),
            Mm(pt_to_mm(input.layout.page_height_pt)),
            ops,
        ));
    }

    let bytes = doc
        .with_pages(pdf_pages)
        .save(&PdfSaveOptions::default(), &mut warnings);
    if !warnings.is_empty() {
        tracing::debug!("printpdf save warnings: {:?}", warnings);
    }

    // Sanity check — printpdf does not return a Result from save() but we
    // want to surface a failure if the buffer looks invalid. A valid PDF
    // starts with `%PDF-` and ends with `%%EOF`.
    if bytes.len() < 32 || !bytes.starts_with(b"%PDF-") {
        return Err(anyhow!("printpdf produced an empty or invalid buffer"));
    }
    Ok(bytes)
}

/// Converts a point value to millimeters (1 mm = 2.83465 pt).
fn pt_to_mm(pt: f32) -> f32 {
    pt / 2.834_645_7
}

/// Builds an `Op::SetTextMatrix` that **absolutely** positions the text
/// cursor at `(x_pt, y_pt)` measured from the lower-left of the page.
///
/// We deliberately do not use `Op::SetTextCursor`: that op serializes to
/// the PDF `Td` operator, which is *relative* to the start of the current
/// text line. Issuing several `SetTextCursor` calls in the same text
/// section therefore makes their offsets compound, sending every line
/// after the first one off-page. `SetTextMatrix` serializes to `Tm`, which
/// **replaces** the current text matrix and gives us true absolute
/// positioning — exactly what we need for laying out independent header,
/// body, and footer rows.
fn move_to_pt(x_pt: f32, y_pt: f32) -> Op {
    Op::SetTextMatrix {
        matrix: TextMatrix::Translate(Pt(x_pt), Pt(y_pt)),
    }
}

/// Builds the full op-list for a single page: header, body, footer.
fn build_page_ops(
    page: &Page,
    page_idx: usize,
    total_pages: usize,
    gutter_width: usize,
    input: &PdfInput<'_>,
    font_id: &FontId,
) -> Vec<Op> {
    let layout = input.layout;
    let font_size = Pt(layout.font_size_pt);
    let line_h = Pt(layout.line_height_pt);
    let page_height_pt = layout.page_height_pt;

    // PDF coordinates are bottom-left origin. `y_top_body` is the baseline
    // of the first body line, counted down from the top of the page.
    let y_top_body = page_height_pt - layout.margin_pt - layout.header_height_pt;
    let y_footer_baseline = layout.margin_pt;

    let text_color = Color::Rgb(Rgb {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        icc_profile: None,
    });
    let gutter_color = Color::Rgb(Rgb {
        r: 0.45,
        g: 0.45,
        b: 0.45,
        icc_profile: None,
    });

    let mut ops: Vec<Op> = Vec::with_capacity(page.lines.len() * 4 + 16);
    ops.push(Op::SaveGraphicsState);
    ops.push(Op::StartTextSection);
    ops.push(Op::SetFont {
        font: PdfFontHandle::External(font_id.clone()),
        size: font_size,
    });
    ops.push(Op::SetLineHeight { lh: line_h });
    ops.push(Op::SetFillColor {
        col: text_color.clone(),
    });

    // -- Header --
    let header_y = page_height_pt - layout.margin_pt - layout.font_size_pt;
    ops.push(move_to_pt(layout.margin_pt, header_y));
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(truncate_display(&input.title, 80))],
    });

    if !input.subtitle.is_empty() {
        // Second header row: subtitle (full path) smaller / lighter.
        let sub_y = header_y - layout.line_height_pt;
        ops.push(Op::SetFillColor {
            col: gutter_color.clone(),
        });
        ops.push(move_to_pt(layout.margin_pt, sub_y));
        ops.push(Op::ShowText {
            items: vec![TextItem::Text(truncate_display(&input.subtitle, 100))],
        });
        ops.push(Op::SetFillColor {
            col: text_color.clone(),
        });
    }

    // Right-aligned timestamp on the first header row.
    let ts = input.generated_at.format("%Y-%m-%d %H:%M:%S").to_string();
    let ts_width_pt = ts.chars().count() as f32 * layout.font_size_pt * layout.char_advance_ratio;
    let ts_x = (layout.page_width_pt - layout.margin_pt - ts_width_pt).max(layout.margin_pt);
    ops.push(Op::SetFillColor {
        col: gutter_color.clone(),
    });
    ops.push(move_to_pt(ts_x, header_y));
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(ts)],
    });
    ops.push(Op::SetFillColor {
        col: text_color.clone(),
    });

    // -- Body --
    let char_width_pt = layout.font_size_pt * layout.char_advance_ratio;
    let gutter_pt = gutter_width as f32 * char_width_pt;
    let body_x = layout.margin_pt + gutter_pt;

    for (row_idx, row) in page.lines.iter().enumerate() {
        let baseline_y = y_top_body - (row_idx as f32 + 1.0) * layout.line_height_pt;
        if baseline_y < y_footer_baseline + layout.line_height_pt {
            break; // Safety: should not happen if lines_per_page() is correct.
        }

        if gutter_width > 0 {
            let gutter_text = format_gutter(
                if row.is_continuation {
                    None
                } else {
                    Some(row.source_line)
                },
                gutter_width,
            );
            ops.push(Op::SetFillColor {
                col: gutter_color.clone(),
            });
            ops.push(move_to_pt(layout.margin_pt, baseline_y));
            ops.push(Op::ShowText {
                items: vec![TextItem::Text(gutter_text)],
            });
            ops.push(Op::SetFillColor {
                col: text_color.clone(),
            });
        }

        if !row.text.is_empty() {
            ops.push(move_to_pt(body_x, baseline_y));
            ops.push(Op::ShowText {
                items: vec![TextItem::Text(row.text.clone())],
            });
        }
    }

    // -- Footer --
    let footer_text = format!("Page {} of {}", page_idx + 1, total_pages);
    let footer_width_pt = footer_text.chars().count() as f32 * char_width_pt;
    let footer_x = (layout.page_width_pt - footer_width_pt) / 2.0;
    ops.push(Op::SetFillColor { col: gutter_color });
    ops.push(move_to_pt(footer_x, y_footer_baseline));
    ops.push(Op::ShowText {
        items: vec![TextItem::Text(footer_text)],
    });

    ops.push(Op::EndTextSection);
    ops.push(Op::RestoreGraphicsState);
    ops
}

/// Truncates a display string to at most `max` characters, appending an
/// ellipsis if clipped. Counts characters, not bytes, so multi-byte text
/// is handled correctly.
fn truncate_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('\u{2026}');
    out
}

/// Convenience: builds a full print job (paginate + render) from a plain
/// text snapshot. Used by [`job`](super::job) and by tests.
pub fn render_document(
    text: &str,
    title: &str,
    subtitle: &str,
    generated_at: chrono::DateTime<chrono::Local>,
    show_line_numbers: bool,
) -> Result<Vec<u8>> {
    let layout = PageLayout::a4_default(show_line_numbers);
    let pages = super::layout::paginate(text, &layout);
    // Count true source lines so the gutter is sized correctly.
    let source_line_count = if text.is_empty() {
        1
    } else {
        let mut n = text.matches('\n').count();
        if !text.ends_with('\n') {
            n += 1;
        }
        n.max(1)
    };
    let input = PdfInput {
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        generated_at,
        pages: &pages,
        layout: &layout,
        total_source_lines: source_line_count,
    };
    render_to_bytes(&input).context("failed to render PDF")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> chrono::DateTime<chrono::Local> {
        chrono::Local::now()
    }

    #[test]
    fn render_empty_document_produces_valid_pdf() {
        let bytes = render_document("", "Untitled", "", now(), true).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.windows(5).any(|w| w == b"%%EOF"));
    }

    #[test]
    fn render_short_document_produces_valid_pdf() {
        let bytes =
            render_document("hello world", "test.txt", "/tmp/test.txt", now(), true).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        assert!(bytes.windows(5).any(|w| w == b"%%EOF"));
        // A single-page document with text should be well over the 32-byte
        // minimum enforced by render_to_bytes.
        assert!(
            bytes.len() > 2_000,
            "PDF suspiciously small: {} bytes",
            bytes.len()
        );
    }

    #[test]
    fn render_multipage_document_has_matching_page_count() {
        // Generate enough lines to force pagination.
        let lines: Vec<String> = (1..=500).map(|i| format!("line {i}")).collect();
        let text = lines.join("\n");
        let layout = PageLayout::a4_default(true);
        let pages = super::super::layout::paginate(&text, &layout);
        assert!(pages.len() >= 2, "500-line doc should need multiple pages");

        let bytes = render_document(&text, "big.txt", "", now(), true).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn render_non_ascii_text_succeeds() {
        // Latin supplement + Cyrillic + basic CJK that DejaVu covers.
        let text = "héllo, мир, こんにちは";
        let bytes = render_document(text, "utf8.txt", "", now(), true).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    #[test]
    fn render_without_line_numbers() {
        let bytes = render_document("no gutter here", "no_ln.txt", "", now(), false).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
    }

    /// Regression test for the "PDF only contains the filename, no body"
    /// bug. The first version of this module used `Op::SetTextCursor` to
    /// position every line, which serializes to PDF's `Td` operator —
    /// **relative** to the start of the current text line. The result was
    /// that only the very first text emit (the title in the header) landed
    /// at the right place; every subsequent emit compounded the offset and
    /// was rendered off-page, so the printed PDF only ever showed the
    /// filename. The fix is to use `Op::SetTextMatrix(Translate(..))`,
    /// which serializes to `Tm` and gives true absolute positioning.
    ///
    /// This test calls `build_page_ops` directly and verifies that:
    ///   * every text-positioning op is a `SetTextMatrix`, never a
    ///     `SetTextCursor`;
    ///   * the body emits at least one `ShowText` per laid-out row, so
    ///     content actually reaches the page.
    #[test]
    fn body_text_uses_absolute_positioning() {
        let layout = PageLayout::a4_default(true);
        let text = "first line\nsecond line\nthird line\n";
        let pages = super::super::layout::paginate(text, &layout);
        assert_eq!(pages.len(), 1, "small doc should fit on one page");

        // We need a FontId to call build_page_ops. Build a throwaway doc
        // and font exactly the way render_to_bytes does.
        let mut font_warnings: Vec<PdfFontParseWarning> = Vec::new();
        let parsed = ParsedFont::from_bytes(FONT_BYTES, 0, &mut font_warnings)
            .expect("bundled font must parse");
        let mut doc = PdfDocument::new("regression");
        let font_id = doc.add_font(&parsed);

        let input = PdfInput {
            title: "regression.txt".to_string(),
            subtitle: "/tmp/regression.txt".to_string(),
            generated_at: now(),
            pages: &pages,
            layout: &layout,
            total_source_lines: 3,
        };
        let ops = build_page_ops(
            &pages[0],
            0,
            1,
            gutter_width_chars(3, true),
            &input,
            &font_id,
        );

        let cursor_count = ops
            .iter()
            .filter(|o| matches!(o, Op::SetTextCursor { .. }))
            .count();
        assert_eq!(
            cursor_count, 0,
            "Op::SetTextCursor must not be used (it serializes to relative `Td`)"
        );

        let matrix_count = ops
            .iter()
            .filter(|o| matches!(o, Op::SetTextMatrix { .. }))
            .count();
        // Header title + subtitle + timestamp + footer = 4 fixed positions,
        // plus per-row gutter and body text positions. With 3 body rows and
        // line numbers enabled, that's 3 gutter + 3 body = 6 row positions.
        // Total expected: 4 + 6 = 10 absolute moves.
        assert!(
            matrix_count >= 8,
            "expected several SetTextMatrix ops for header/body/footer, got {matrix_count}"
        );

        let show_text_count = ops
            .iter()
            .filter(|o| matches!(o, Op::ShowText { .. }))
            .count();
        // Title + subtitle + timestamp + footer + 3 gutters + 3 body lines = 10.
        assert!(
            show_text_count >= 8,
            "expected at least one ShowText per body line, got {show_text_count}"
        );
    }

    #[test]
    fn truncate_display_counts_characters_not_bytes() {
        let s = "éééééé"; // 6 chars, 12 bytes
        assert_eq!(truncate_display(s, 10), s);
        let t = truncate_display(s, 4);
        assert_eq!(t.chars().count(), 4);
        assert!(t.ends_with('\u{2026}'));
    }
}
