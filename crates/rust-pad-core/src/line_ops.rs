/// Advanced line operations: sort, deduplicate, move, duplicate, case conversion.
use crate::buffer::TextBuffer;
use crate::indent::IndentStyle;
use anyhow::{Context, Result};

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    Ascending,
    Descending,
}

/// Sort options.
#[derive(Debug, Clone)]
pub struct SortOptions {
    pub order: SortOrder,
    pub case_sensitive: bool,
    pub numeric: bool,
}

impl Default for SortOptions {
    fn default() -> Self {
        Self {
            order: SortOrder::Ascending,
            case_sensitive: true,
            numeric: false,
        }
    }
}

/// Case conversion mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseConversion {
    Upper,
    Lower,
    TitleCase,
}

/// Reads lines from the buffer in the given range, stripping trailing newlines.
fn read_lines_trimmed(
    buffer: &TextBuffer,
    start_line: usize,
    end_line: usize,
) -> Result<Vec<String>> {
    (start_line..end_line)
        .map(|i| {
            buffer
                .line(i)
                .map(|l| l.to_string().trim_end_matches('\n').to_string())
        })
        .collect::<Result<Vec<_>>>()
}

/// Replaces lines in the buffer range with the given lines.
fn write_lines_back(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
    lines: &[&str],
) -> Result<()> {
    let start_char = buffer.line_to_char(start_line)?;
    let end_char = if end_line < buffer.len_lines() {
        buffer.line_to_char(end_line)?
    } else {
        buffer.len_chars()
    };

    let new_text = lines.join("\n")
        + if end_line < buffer.len_lines() {
            "\n"
        } else {
            ""
        };

    buffer.replace(start_char, end_char, &new_text)?;
    Ok(())
}

/// Compares two strings using case-sensitive or case-insensitive ordering.
fn compare_strings(a: &str, b: &str, case_sensitive: bool) -> std::cmp::Ordering {
    if case_sensitive {
        a.cmp(b)
    } else {
        a.to_lowercase().cmp(&b.to_lowercase())
    }
}

/// Compares two lines according to the given sort options.
fn compare_lines(a: &str, b: &str, options: &SortOptions) -> std::cmp::Ordering {
    let base = if options.numeric {
        match (a.trim().parse::<f64>().ok(), b.trim().parse::<f64>().ok()) {
            (Some(an), Some(bn)) => an.partial_cmp(&bn).unwrap_or(std::cmp::Ordering::Equal),
            _ => compare_strings(a, b, options.case_sensitive),
        }
    } else {
        compare_strings(a, b, options.case_sensitive)
    };

    match options.order {
        SortOrder::Ascending => base,
        SortOrder::Descending => base.reverse(),
    }
}

/// Sorts lines in the given range.
///
/// # Errors
///
/// Returns an error if the line range is out of bounds.
pub fn sort_lines(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
    options: &SortOptions,
) -> Result<()> {
    if end_line > buffer.len_lines() {
        anyhow::bail!("end_line {} out of bounds", end_line);
    }
    if start_line >= end_line {
        return Ok(());
    }

    let mut lines = read_lines_trimmed(buffer, start_line, end_line)
        .context("failed to read lines for sorting")?;

    lines.sort_by(|a, b| compare_lines(a, b, options));

    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    write_lines_back(buffer, start_line, end_line, &refs)?;
    Ok(())
}

/// Removes consecutive duplicate lines in the given range.
pub fn remove_consecutive_duplicates(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
) -> Result<usize> {
    if start_line >= end_line || end_line > buffer.len_lines() {
        return Ok(0);
    }

    let lines = read_lines_trimmed(buffer, start_line, end_line)?;

    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut removed = 0;

    for line in &lines {
        if result.last() != Some(line) {
            result.push(line.clone());
        } else {
            removed += 1;
        }
    }

    if removed > 0 {
        let refs: Vec<&str> = result.iter().map(String::as_str).collect();
        write_lines_back(buffer, start_line, end_line, &refs)?;
    }

    Ok(removed)
}

/// Removes all duplicate lines (keeping first occurrence).
pub fn remove_all_duplicates(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
) -> Result<usize> {
    if start_line >= end_line || end_line > buffer.len_lines() {
        return Ok(0);
    }

    let lines = read_lines_trimmed(buffer, start_line, end_line)?;

    let mut seen = std::collections::HashSet::new();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut removed = 0;

    for line in &lines {
        if seen.insert(line.clone()) {
            result.push(line.clone());
        } else {
            removed += 1;
        }
    }

    if removed > 0 {
        let refs: Vec<&str> = result.iter().map(String::as_str).collect();
        write_lines_back(buffer, start_line, end_line, &refs)?;
    }

    Ok(removed)
}

/// Removes empty lines in the given range.
pub fn remove_empty_lines(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
) -> Result<usize> {
    if start_line >= end_line || end_line > buffer.len_lines() {
        return Ok(0);
    }

    let lines = read_lines_trimmed(buffer, start_line, end_line)?;

    let non_empty: Vec<&str> = lines
        .iter()
        .map(String::as_str)
        .filter(|l| !l.trim().is_empty())
        .collect();
    let removed = lines.len() - non_empty.len();

    if removed > 0 {
        write_lines_back(buffer, start_line, end_line, &non_empty)?;
    }

    Ok(removed)
}

/// Duplicates a line.
pub fn duplicate_line(buffer: &mut TextBuffer, line_idx: usize) -> Result<()> {
    if line_idx >= buffer.len_lines() {
        anyhow::bail!("line index {} out of bounds", line_idx);
    }

    let line_text = buffer
        .line(line_idx)?
        .to_string()
        .trim_end_matches('\n')
        .to_string();

    let insert_pos = if line_idx + 1 < buffer.len_lines() {
        buffer.line_to_char(line_idx + 1)?
    } else {
        buffer.len_chars()
    };

    let text_to_insert = if line_idx + 1 < buffer.len_lines() {
        format!("{line_text}\n")
    } else {
        format!("\n{line_text}")
    };

    buffer.insert(insert_pos, &text_to_insert)?;
    Ok(())
}

/// Moves a line up by one position.
pub fn move_line_up(buffer: &mut TextBuffer, line_idx: usize) -> Result<bool> {
    if line_idx == 0 || line_idx >= buffer.len_lines() {
        return Ok(false);
    }

    let current_line = buffer
        .line(line_idx)?
        .to_string()
        .trim_end_matches('\n')
        .to_string();
    let prev_line = buffer
        .line(line_idx - 1)?
        .to_string()
        .trim_end_matches('\n')
        .to_string();

    let start = buffer.line_to_char(line_idx - 1)?;
    let end = if line_idx + 1 < buffer.len_lines() {
        buffer.line_to_char(line_idx + 1)?
    } else {
        buffer.len_chars()
    };

    let new_text = if line_idx + 1 < buffer.len_lines() {
        format!("{current_line}\n{prev_line}\n")
    } else {
        format!("{current_line}\n{prev_line}")
    };

    buffer.replace(start, end, &new_text)?;
    Ok(true)
}

/// Moves a line down by one position.
pub fn move_line_down(buffer: &mut TextBuffer, line_idx: usize) -> Result<bool> {
    if line_idx + 1 >= buffer.len_lines() {
        return Ok(false);
    }

    let current_line = buffer
        .line(line_idx)?
        .to_string()
        .trim_end_matches('\n')
        .to_string();
    let next_line = buffer
        .line(line_idx + 1)?
        .to_string()
        .trim_end_matches('\n')
        .to_string();

    let start = buffer.line_to_char(line_idx)?;
    let end = if line_idx + 2 < buffer.len_lines() {
        buffer.line_to_char(line_idx + 2)?
    } else {
        buffer.len_chars()
    };

    let new_text = if line_idx + 2 < buffer.len_lines() {
        format!("{next_line}\n{current_line}\n")
    } else {
        format!("{next_line}\n{current_line}")
    };

    buffer.replace(start, end, &new_text)?;
    Ok(true)
}

/// Converts text to title case (capitalize first letter of each word).
fn to_title_case(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut capitalize_next = true;
    for ch in text.chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' {
            capitalize_next = true;
            result.push(ch);
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.extend(ch.to_lowercase());
        }
    }
    result
}

/// Converts the case of text.
pub fn convert_case(text: &str, conversion: CaseConversion) -> String {
    match conversion {
        CaseConversion::Upper => text.to_uppercase(),
        CaseConversion::Lower => text.to_lowercase(),
        CaseConversion::TitleCase => to_title_case(text),
    }
}

/// Indents lines in the given range using the specified indent style.
pub fn indent_lines(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
    style: &IndentStyle,
) -> Result<()> {
    let indent = style.indent_text();
    // Process from last to first to maintain char positions
    for line_idx in (start_line..end_line.min(buffer.len_lines())).rev() {
        let line_start = buffer.line_to_char(line_idx)?;
        buffer.insert(line_start, &indent)?;
    }
    Ok(())
}

/// Dedents lines in the given range using the specified indent style.
pub fn dedent_lines(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
    style: &IndentStyle,
) -> Result<()> {
    // Process from last to first to maintain char positions
    for line_idx in (start_line..end_line.min(buffer.len_lines())).rev() {
        let line = buffer.line(line_idx)?.to_string();
        let to_remove = match style {
            IndentStyle::Tabs => {
                if line.starts_with('\t') {
                    1
                } else {
                    // Fall back: remove up to 4 leading spaces for mixed content
                    line.chars().take_while(|c| *c == ' ').count().min(4)
                }
            }
            IndentStyle::Spaces(n) => line.chars().take_while(|c| *c == ' ').count().min(*n),
        };
        if to_remove > 0 {
            let line_start = buffer.line_to_char(line_idx)?;
            buffer.remove(line_start, line_start + to_remove)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_ascending() {
        let mut buf = TextBuffer::from("cherry\napple\nbanana");
        sort_lines(&mut buf, 0, 3, &SortOptions::default()).unwrap();
        assert_eq!(buf.to_string(), "apple\nbanana\ncherry");
    }

    #[test]
    fn test_sort_descending() {
        let mut buf = TextBuffer::from("cherry\napple\nbanana");
        sort_lines(
            &mut buf,
            0,
            3,
            &SortOptions {
                order: SortOrder::Descending,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(buf.to_string(), "cherry\nbanana\napple");
    }

    #[test]
    fn test_sort_case_insensitive() {
        let mut buf = TextBuffer::from("Banana\napple\nCherry");
        sort_lines(
            &mut buf,
            0,
            3,
            &SortOptions {
                case_sensitive: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(buf.to_string(), "apple\nBanana\nCherry");
    }

    #[test]
    fn test_sort_numeric() {
        let mut buf = TextBuffer::from("10\n2\n1\n20");
        sort_lines(
            &mut buf,
            0,
            4,
            &SortOptions {
                numeric: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(buf.to_string(), "1\n2\n10\n20");
    }

    #[test]
    fn test_remove_consecutive_duplicates() {
        let mut buf = TextBuffer::from("a\na\nb\nb\nc");
        let removed = remove_consecutive_duplicates(&mut buf, 0, 5).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_remove_all_duplicates() {
        let mut buf = TextBuffer::from("a\nb\na\nc\nb");
        let removed = remove_all_duplicates(&mut buf, 0, 5).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_remove_empty_lines() {
        let mut buf = TextBuffer::from("a\n\nb\n\nc");
        let removed = remove_empty_lines(&mut buf, 0, 5).unwrap();
        assert_eq!(removed, 2);
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_duplicate_line() {
        let mut buf = TextBuffer::from("a\nb\nc");
        duplicate_line(&mut buf, 1).unwrap();
        assert_eq!(buf.to_string(), "a\nb\nb\nc");
    }

    #[test]
    fn test_move_line_up() {
        let mut buf = TextBuffer::from("a\nb\nc");
        assert!(move_line_up(&mut buf, 1).unwrap());
        assert_eq!(buf.to_string(), "b\na\nc");
    }

    #[test]
    fn test_move_line_down() {
        let mut buf = TextBuffer::from("a\nb\nc");
        assert!(move_line_down(&mut buf, 0).unwrap());
        assert_eq!(buf.to_string(), "b\na\nc");
    }

    #[test]
    fn test_convert_case() {
        assert_eq!(
            convert_case("hello world", CaseConversion::Upper),
            "HELLO WORLD"
        );
        assert_eq!(
            convert_case("HELLO WORLD", CaseConversion::Lower),
            "hello world"
        );
        assert_eq!(
            convert_case("hello world", CaseConversion::TitleCase),
            "Hello World"
        );
    }

    #[test]
    fn test_indent_dedent_spaces_4() {
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("a\nb\nc");
        indent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "    a\n    b\n    c");

        dedent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_indent_dedent_spaces_2() {
        let style = IndentStyle::Spaces(2);
        let mut buf = TextBuffer::from("a\nb\nc");
        indent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "  a\n  b\n  c");

        dedent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_indent_dedent_tabs() {
        let style = IndentStyle::Tabs;
        let mut buf = TextBuffer::from("a\nb\nc");
        indent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "\ta\n\tb\n\tc");

        dedent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_dedent_tabs_mixed_content() {
        // With Tabs style, dedent should fall back to removing spaces
        // when no leading tab is found.
        let style = IndentStyle::Tabs;
        let mut buf = TextBuffer::from("    a\n\tb");
        dedent_lines(&mut buf, 0, 2, &style).unwrap();
        assert_eq!(buf.to_string(), "a\nb");
    }

    #[test]
    fn test_dedent_spaces_partial() {
        // When line has fewer leading spaces than the indent width,
        // dedent removes only what's available.
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("  a\n    b");
        dedent_lines(&mut buf, 0, 2, &style).unwrap();
        assert_eq!(buf.to_string(), "a\nb");
    }

    #[test]
    fn test_indent_double() {
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("a");
        indent_lines(&mut buf, 0, 1, &style).unwrap();
        indent_lines(&mut buf, 0, 1, &style).unwrap();
        assert_eq!(buf.to_string(), "        a");
    }

    // ── sort edge cases ──────────────────────────────────────────────

    #[test]
    fn test_sort_out_of_bounds() {
        let mut buf = TextBuffer::from("a\nb");
        assert!(sort_lines(&mut buf, 0, 10, &SortOptions::default()).is_err());
    }

    #[test]
    fn test_sort_empty_range() {
        let mut buf = TextBuffer::from("b\na");
        sort_lines(&mut buf, 1, 1, &SortOptions::default()).unwrap();
        assert_eq!(buf.to_string(), "b\na"); // no change
    }

    #[test]
    fn test_sort_single_line() {
        let mut buf = TextBuffer::from("only");
        sort_lines(&mut buf, 0, 1, &SortOptions::default()).unwrap();
        assert_eq!(buf.to_string(), "only");
    }

    #[test]
    fn test_sort_partial_range() {
        let mut buf = TextBuffer::from("c\nb\na\nd");
        sort_lines(&mut buf, 0, 3, &SortOptions::default()).unwrap();
        // Only first 3 lines sorted, 'd' stays
        assert_eq!(buf.to_string(), "a\nb\nc\nd");
    }

    #[test]
    fn test_sort_numeric_descending() {
        let mut buf = TextBuffer::from("1\n10\n2\n20");
        sort_lines(
            &mut buf,
            0,
            4,
            &SortOptions {
                numeric: true,
                order: SortOrder::Descending,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(buf.to_string(), "20\n10\n2\n1");
    }

    #[test]
    fn test_sort_numeric_mixed_with_text() {
        let mut buf = TextBuffer::from("10\nabc\n2");
        sort_lines(
            &mut buf,
            0,
            3,
            &SortOptions {
                numeric: true,
                ..Default::default()
            },
        )
        .unwrap();
        // "abc" can't parse as number, falls back to string comparison
        // 2 < 10 (numeric), "abc" is compared as string
        assert_eq!(buf.to_string(), "2\n10\nabc");
    }

    // ── dedup edge cases ─────────────────────────────────────────────

    #[test]
    fn test_consecutive_dedup_no_duplicates() {
        let mut buf = TextBuffer::from("a\nb\nc");
        let removed = remove_consecutive_duplicates(&mut buf, 0, 3).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_all_dedup_no_duplicates() {
        let mut buf = TextBuffer::from("a\nb\nc");
        let removed = remove_all_duplicates(&mut buf, 0, 3).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_consecutive_dedup_all_same() {
        let mut buf = TextBuffer::from("a\na\na\na");
        let removed = remove_consecutive_duplicates(&mut buf, 0, 4).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(buf.to_string(), "a");
    }

    #[test]
    fn test_all_dedup_all_same() {
        let mut buf = TextBuffer::from("a\na\na\na");
        let removed = remove_all_duplicates(&mut buf, 0, 4).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(buf.to_string(), "a");
    }

    #[test]
    fn test_consecutive_dedup_keeps_non_adjacent_dupes() {
        let mut buf = TextBuffer::from("a\nb\na\nb");
        let removed = remove_consecutive_duplicates(&mut buf, 0, 4).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(buf.to_string(), "a\nb\na\nb");
    }

    // ── move line boundary cases ─────────────────────────────────────

    #[test]
    fn test_move_line_up_first_line_noop() {
        let mut buf = TextBuffer::from("a\nb\nc");
        assert!(!move_line_up(&mut buf, 0).unwrap());
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_move_line_down_last_line_noop() {
        let mut buf = TextBuffer::from("a\nb\nc");
        assert!(!move_line_down(&mut buf, 2).unwrap());
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_move_line_up_out_of_bounds() {
        let mut buf = TextBuffer::from("a\nb");
        assert!(!move_line_up(&mut buf, 10).unwrap());
    }

    #[test]
    fn test_move_line_down_out_of_bounds() {
        let mut buf = TextBuffer::from("a\nb");
        assert!(!move_line_down(&mut buf, 10).unwrap());
    }

    #[test]
    fn test_move_line_up_last_line() {
        let mut buf = TextBuffer::from("a\nb\nc");
        assert!(move_line_up(&mut buf, 2).unwrap());
        assert_eq!(buf.to_string(), "a\nc\nb");
    }

    #[test]
    fn test_move_line_down_first_line() {
        let mut buf = TextBuffer::from("a\nb\nc");
        assert!(move_line_down(&mut buf, 0).unwrap());
        assert_eq!(buf.to_string(), "b\na\nc");
    }

    // ── duplicate line edge cases ────────────────────────────────────

    #[test]
    fn test_duplicate_last_line() {
        let mut buf = TextBuffer::from("a\nb\nc");
        duplicate_line(&mut buf, 2).unwrap();
        assert_eq!(buf.to_string(), "a\nb\nc\nc");
    }

    #[test]
    fn test_duplicate_first_line() {
        let mut buf = TextBuffer::from("a\nb\nc");
        duplicate_line(&mut buf, 0).unwrap();
        assert_eq!(buf.to_string(), "a\na\nb\nc");
    }

    #[test]
    fn test_duplicate_line_out_of_bounds() {
        let mut buf = TextBuffer::from("a");
        assert!(duplicate_line(&mut buf, 5).is_err());
    }

    #[test]
    fn test_duplicate_single_line_buffer() {
        let mut buf = TextBuffer::from("only");
        duplicate_line(&mut buf, 0).unwrap();
        assert_eq!(buf.to_string(), "only\nonly");
    }

    // ── remove_empty_lines edge cases ────────────────────────────────

    #[test]
    fn test_remove_empty_lines_none() {
        let mut buf = TextBuffer::from("a\nb\nc");
        let removed = remove_empty_lines(&mut buf, 0, 3).unwrap();
        assert_eq!(removed, 0);
        assert_eq!(buf.to_string(), "a\nb\nc");
    }

    #[test]
    fn test_remove_empty_lines_whitespace_only() {
        let mut buf = TextBuffer::from("a\n   \nb");
        let removed = remove_empty_lines(&mut buf, 0, 3).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(buf.to_string(), "a\nb");
    }

    // ── convert_case edge cases ──────────────────────────────────────

    #[test]
    fn test_convert_case_empty_string() {
        assert_eq!(convert_case("", CaseConversion::Upper), "");
        assert_eq!(convert_case("", CaseConversion::Lower), "");
        assert_eq!(convert_case("", CaseConversion::TitleCase), "");
    }

    #[test]
    fn test_convert_case_title_with_hyphens() {
        assert_eq!(
            convert_case("hello-world", CaseConversion::TitleCase),
            "Hello-World"
        );
    }

    #[test]
    fn test_convert_case_title_with_underscores() {
        assert_eq!(
            convert_case("hello_world", CaseConversion::TitleCase),
            "Hello_World"
        );
    }

    #[test]
    fn test_convert_case_unicode() {
        assert_eq!(convert_case("café", CaseConversion::Upper), "CAFÉ");
        assert_eq!(convert_case("MÜNCHEN", CaseConversion::Lower), "münchen");
    }

    // ── indent/dedent partial range ──────────────────────────────────

    #[test]
    fn test_indent_partial_range() {
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("a\nb\nc\nd");
        indent_lines(&mut buf, 1, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "a\n    b\n    c\nd");
    }

    #[test]
    fn test_dedent_partial_range() {
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("    a\n    b\n    c\n    d");
        dedent_lines(&mut buf, 1, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "    a\nb\nc\n    d");
    }

    #[test]
    fn test_indent_beyond_buffer() {
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("a\nb");
        // end_line beyond buffer should be clamped
        indent_lines(&mut buf, 0, 100, &style).unwrap();
        assert_eq!(buf.to_string(), "    a\n    b");
    }

    #[test]
    fn test_dedent_no_indent_noop() {
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("abc\ndef");
        dedent_lines(&mut buf, 0, 2, &style).unwrap();
        assert_eq!(buf.to_string(), "abc\ndef");
    }
}
