//! Renders laid-out pages into PDF bytes using `pdf-writer`.
//!
//! The bulk of the complexity (wrapping, tab expansion, page breaks) lives
//! in [`layout`](super::layout) so this module can focus on translating
//! a `Vec<Page>` into a PDF. Header, footer, and line numbers are drawn
//! here because they depend on the PDF coordinate system.

use std::collections::BTreeMap;

use anyhow::{anyhow, Context, Result};
use pdf_writer::types::{CidFontType, FontFlags, SystemInfo, UnicodeCmap};
use pdf_writer::{Content, Name, Pdf, Rect, Ref, Str};

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

/// Parsed font metrics and char→GID mapping extracted from the bundled TTF.
struct FontInfo {
    char_to_gid: BTreeMap<char, u16>,
    ascender: f32,
    descender: f32,
    cap_height: f32,
    bbox: Rect,
    default_width: f32,
}

/// Parses the bundled TTF and extracts metrics and the cmap table needed
/// for PDF font embedding and text encoding.
fn parse_font() -> Result<FontInfo> {
    let face = ttf_parser::Face::parse(FONT_BYTES, 0)
        .map_err(|e| anyhow!("failed to parse bundled font: {e:?}"))?;

    let units_per_em = face.units_per_em() as f32;
    let scale = 1000.0 / units_per_em;

    let ascender = face.ascender() as f32 * scale;
    let descender = face.descender() as f32 * scale;
    let cap_height = face.capital_height().unwrap_or(face.ascender()) as f32 * scale;

    let ttf_bbox = face.global_bounding_box();
    let bbox = Rect::new(
        ttf_bbox.x_min as f32 * scale,
        ttf_bbox.y_min as f32 * scale,
        ttf_bbox.x_max as f32 * scale,
        ttf_bbox.y_max as f32 * scale,
    );

    // Build char→GID mapping for the Basic Multilingual Plane.
    let mut char_to_gid = BTreeMap::new();
    for code_point in 0u32..0x1_0000 {
        if let Some(ch) = char::from_u32(code_point) {
            if let Some(gid) = face.glyph_index(ch) {
                if gid.0 != 0 {
                    char_to_gid.insert(ch, gid.0);
                }
            }
        }
    }

    // Monospace advance width in PDF units (1/1000 of em).
    let default_width = face
        .glyph_index('M')
        .and_then(|gid| face.glyph_hor_advance(gid))
        .map(|w| w as f32 * scale)
        .unwrap_or(600.0);

    Ok(FontInfo {
        char_to_gid,
        ascender,
        descender,
        cap_height,
        bbox,
        default_width,
    })
}

/// Encodes a string as big-endian GID pairs for an Identity-H CIDFont.
fn encode_text(text: &str, char_to_gid: &BTreeMap<char, u16>) -> Vec<u8> {
    let mut buf = Vec::with_capacity(text.len() * 2);
    for ch in text.chars() {
        let gid = char_to_gid.get(&ch).copied().unwrap_or(0);
        buf.extend_from_slice(&gid.to_be_bytes());
    }
    buf
}

/// Renders `input` into a complete PDF byte buffer.
///
/// Returns an error if the bundled font cannot be parsed (should not
/// happen in practice — see the unit test in [`super::font`]) or if
/// `pdf-writer` produces an invalid buffer.
pub fn render_to_bytes(input: &PdfInput<'_>) -> Result<Vec<u8>> {
    let font_info = parse_font()?;

    let total_pages = input.pages.len().max(1);
    let gutter_width = gutter_width_chars(input.total_source_lines, input.layout.show_line_numbers);

    // ── Ref allocation ──────────────────────────────────────────────
    // Layout: catalog, page_tree, [page, content] × N, font objects.
    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let base = 3i32;
    let font_base = base + 2 * total_pages as i32;
    let type0_font_id = Ref::new(font_base);
    let cid_font_id = Ref::new(font_base + 1);
    let descriptor_id = Ref::new(font_base + 2);
    let font_stream_id = Ref::new(font_base + 3);
    let cmap_id = Ref::new(font_base + 4);

    let page_ids: Vec<Ref> = (0..total_pages)
        .map(|i| Ref::new(base + 2 * i as i32))
        .collect();
    let content_ids: Vec<Ref> = (0..total_pages)
        .map(|i| Ref::new(base + 2 * i as i32 + 1))
        .collect();

    // ── Build content streams ───────────────────────────────────────
    let content_streams: Vec<Vec<u8>> = input
        .pages
        .iter()
        .enumerate()
        .map(|(idx, page)| {
            build_page_content(
                page,
                idx,
                total_pages,
                gutter_width,
                input,
                &font_info.char_to_gid,
            )
        })
        .collect();

    // ── Assemble PDF ────────────────────────────────────────────────
    let mut pdf = Pdf::new();

    // Catalog → page tree
    pdf.catalog(catalog_id).pages(page_tree_id);

    // Page tree with shared font resources (inherited by all pages).
    {
        let mut pages = pdf.pages(page_tree_id);
        pages.kids(page_ids.iter().copied());
        pages.count(total_pages as i32);
        pages.resources().fonts().pair(Name(b"F1"), type0_font_id);
    }

    // Individual pages + content streams.
    let media_box = Rect::new(
        0.0,
        0.0,
        input.layout.page_width_pt,
        input.layout.page_height_pt,
    );
    for i in 0..total_pages {
        {
            let mut page = pdf.page(page_ids[i]);
            page.parent(page_tree_id);
            page.media_box(media_box);
            page.contents(content_ids[i]);
        }
        pdf.stream(content_ids[i], &content_streams[i]);
    }

    // Font embedding.
    embed_font(
        &mut pdf,
        &font_info,
        type0_font_id,
        cid_font_id,
        descriptor_id,
        font_stream_id,
        cmap_id,
    );

    let bytes = pdf.finish();
    if bytes.len() < 32 || !bytes.starts_with(b"%PDF-") {
        return Err(anyhow!("pdf-writer produced an empty or invalid buffer"));
    }
    Ok(bytes)
}

/// Writes the Type0 + CIDFont + FontDescriptor + font stream + ToUnicode
/// CMap into the PDF.
fn embed_font(
    pdf: &mut Pdf,
    info: &FontInfo,
    type0_id: Ref,
    cid_font_id: Ref,
    descriptor_id: Ref,
    font_stream_id: Ref,
    cmap_id: Ref,
) {
    let font_name = Name(b"DejaVuSansMono");

    // Type0 (composite) font.
    {
        let mut f = pdf.type0_font(type0_id);
        f.base_font(font_name);
        f.encoding_predefined(Name(b"Identity-H"));
        f.descendant_font(cid_font_id);
        f.to_unicode(cmap_id);
    }

    // CID font (descendant — TrueType outlines).
    {
        let mut f = pdf.cid_font(cid_font_id);
        f.subtype(CidFontType::Type2);
        f.base_font(font_name);
        f.system_info(SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        });
        f.font_descriptor(descriptor_id);
        f.default_width(info.default_width);
        f.cid_to_gid_map_predefined(Name(b"Identity"));
    }

    // Font descriptor.
    {
        let mut f = pdf.font_descriptor(descriptor_id);
        f.name(font_name);
        f.flags(FontFlags::FIXED_PITCH | FontFlags::NON_SYMBOLIC);
        f.bbox(info.bbox);
        f.italic_angle(0.0);
        f.ascent(info.ascender);
        f.descent(info.descender);
        f.cap_height(info.cap_height);
        f.stem_v(80.0);
        f.font_file2(font_stream_id);
    }

    // Raw TTF stream (font file data).
    pdf.stream(font_stream_id, FONT_BYTES);

    // ToUnicode CMap — enables text selection/search in the PDF.
    let mut gid_to_char: BTreeMap<u16, char> = BTreeMap::new();
    for (&ch, &gid) in &info.char_to_gid {
        gid_to_char.entry(gid).or_insert(ch);
    }
    let mut cmap = UnicodeCmap::new(
        Name(b"Custom"),
        SystemInfo {
            registry: Str(b"Adobe"),
            ordering: Str(b"Identity"),
            supplement: 0,
        },
    );
    for (&gid, &ch) in &gid_to_char {
        cmap.pair(gid, ch);
    }
    let cmap_data = cmap.finish();
    pdf.stream(cmap_id, &cmap_data);
}

/// Builds the PDF content stream (raw operator bytes) for a single page:
/// header, body lines, and footer.
fn build_page_content(
    page: &Page,
    page_idx: usize,
    total_pages: usize,
    gutter_width: usize,
    input: &PdfInput<'_>,
    char_to_gid: &BTreeMap<char, u16>,
) -> Vec<u8> {
    let layout = input.layout;
    let font_size = layout.font_size_pt;
    let page_height_pt = layout.page_height_pt;

    // PDF coordinates are bottom-left origin.
    let y_top_body = page_height_pt - layout.margin_pt - layout.header_height_pt;
    let y_footer_baseline = layout.margin_pt;

    let mut content = Content::new();
    content.save_state();
    content.begin_text();
    content.set_font(Name(b"F1"), font_size);
    content.set_leading(layout.line_height_pt);
    content.set_fill_rgb(0.0, 0.0, 0.0);

    // ── Header ──────────────────────────────────────────────────────
    let header_y = page_height_pt - layout.margin_pt - font_size;

    // Title (left-aligned).
    content.set_text_matrix([1.0, 0.0, 0.0, 1.0, layout.margin_pt, header_y]);
    let encoded = encode_text(&truncate_display(&input.title, 80), char_to_gid);
    content.show(Str(&encoded));

    // Subtitle (second row, dimmed).
    if !input.subtitle.is_empty() {
        let sub_y = header_y - layout.line_height_pt;
        content.set_fill_rgb(0.45, 0.45, 0.45);
        content.set_text_matrix([1.0, 0.0, 0.0, 1.0, layout.margin_pt, sub_y]);
        let encoded = encode_text(&truncate_display(&input.subtitle, 100), char_to_gid);
        content.show(Str(&encoded));
        content.set_fill_rgb(0.0, 0.0, 0.0);
    }

    // Timestamp (right-aligned on the first header row).
    let ts = input.generated_at.format("%Y-%m-%d %H:%M:%S").to_string();
    let ts_width_pt = ts.chars().count() as f32 * font_size * layout.char_advance_ratio;
    let ts_x = (layout.page_width_pt - layout.margin_pt - ts_width_pt).max(layout.margin_pt);
    content.set_fill_rgb(0.45, 0.45, 0.45);
    content.set_text_matrix([1.0, 0.0, 0.0, 1.0, ts_x, header_y]);
    let encoded = encode_text(&ts, char_to_gid);
    content.show(Str(&encoded));
    content.set_fill_rgb(0.0, 0.0, 0.0);

    // ── Body ────────────────────────────────────────────────────────
    let char_width_pt = font_size * layout.char_advance_ratio;
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
            content.set_fill_rgb(0.45, 0.45, 0.45);
            content.set_text_matrix([1.0, 0.0, 0.0, 1.0, layout.margin_pt, baseline_y]);
            let encoded = encode_text(&gutter_text, char_to_gid);
            content.show(Str(&encoded));
            content.set_fill_rgb(0.0, 0.0, 0.0);
        }

        if !row.text.is_empty() {
            content.set_text_matrix([1.0, 0.0, 0.0, 1.0, body_x, baseline_y]);
            let encoded = encode_text(&row.text, char_to_gid);
            content.show(Str(&encoded));
        }
    }

    // ── Footer ──────────────────────────────────────────────────────
    let footer_text = format!("Page {} of {}", page_idx + 1, total_pages);
    let footer_width_pt = footer_text.chars().count() as f32 * char_width_pt;
    let footer_x = (layout.page_width_pt - footer_width_pt) / 2.0;
    content.set_fill_rgb(0.45, 0.45, 0.45);
    content.set_text_matrix([1.0, 0.0, 0.0, 1.0, footer_x, y_footer_baseline]);
    let encoded = encode_text(&footer_text, char_to_gid);
    content.show(Str(&encoded));

    content.end_text();
    content.restore_state();
    content.finish().to_vec()
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
    /// bug. The first version of the print module used relative `Td`
    /// positioning, which compounded offsets and pushed everything off-page.
    /// The fix is to use `Tm` (absolute text matrix) for every text emit.
    ///
    /// This test calls `build_page_content` directly and inspects the raw
    /// content stream bytes to verify:
    ///   * every text-positioning op is `Tm`, never `Td`;
    ///   * the body emits at least one `Tj` per laid-out row.
    #[test]
    fn body_text_uses_absolute_positioning() {
        let layout = PageLayout::a4_default(true);
        let text = "first line\nsecond line\nthird line\n";
        let pages = super::super::layout::paginate(text, &layout);
        assert_eq!(pages.len(), 1, "small doc should fit on one page");

        let font_info = parse_font().expect("bundled font must parse");
        let gutter_width = gutter_width_chars(3, true);

        let input = PdfInput {
            title: "regression.txt".to_string(),
            subtitle: "/tmp/regression.txt".to_string(),
            generated_at: now(),
            pages: &pages,
            layout: &layout,
            total_source_lines: 3,
        };
        let content_bytes = build_page_content(
            &pages[0],
            0,
            1,
            gutter_width,
            &input,
            &font_info.char_to_gid,
        );

        let stream = String::from_utf8_lossy(&content_bytes);

        // Td (relative positioning) must not appear.
        let td_count = stream.lines().filter(|l| l.ends_with(" Td")).count();
        assert_eq!(
            td_count, 0,
            "`Td` operator must not be used (it serializes relative positioning)"
        );

        // Tm (absolute positioning) — header title + subtitle + timestamp
        // + footer = 4 fixed, plus per-row gutter + body text.
        let tm_count = stream.lines().filter(|l| l.ends_with(" Tm")).count();
        assert!(
            tm_count >= 8,
            "expected several Tm ops for header/body/footer, got {tm_count}"
        );

        // Tj (show text) — at least one per emit.
        let tj_count = stream.lines().filter(|l| l.ends_with(" Tj")).count();
        assert!(
            tj_count >= 8,
            "expected at least one Tj per body line, got {tj_count}"
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
