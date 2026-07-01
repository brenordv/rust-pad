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

/// Whitespace-trim mode for line operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMode {
    /// Trim trailing whitespace only.
    Trailing,
    /// Trim leading whitespace only.
    Leading,
    /// Trim both leading and trailing whitespace.
    Both,
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

/// Counts the leading ASCII spaces on `line_str`.
fn leading_space_count(line_str: &str) -> usize {
    line_str.chars().take_while(|c| *c == ' ').count()
}

/// Returns the number of leading-whitespace chars a *single-line* dedent would
/// remove from `line_idx` under `style`. Reads the buffer without mutating it.
///
/// This is the per-line rule used for the `Tabs` style and for single-line
/// dedent (no selection). Multi-line selection dedent goes through
/// [`dedent_removed_for_line`], which for `Spaces` applies a block rule that
/// preserves relative indentation.
///
/// Returns `0` when the line index is out of bounds.
pub fn leading_indent_removable(
    buffer: &TextBuffer,
    line_idx: usize,
    style: &IndentStyle,
) -> usize {
    let Ok(line) = buffer.line(line_idx) else {
        return 0;
    };
    let line_str = line.to_string();
    match style {
        IndentStyle::Tabs => {
            if line_str.starts_with('\t') {
                1
            } else {
                // Fall back: remove up to 4 leading spaces for mixed content
                leading_space_count(&line_str).min(4)
            }
        }
        IndentStyle::Spaces(n) => leading_space_count(&line_str).min(*n),
    }
}

/// Uniform number of leading spaces a `Spaces(w)` **block** dedent removes from
/// every line in `[start_line, end_line)`.
///
/// Returns `0` for [`IndentStyle::Tabs`] (tabs dedent per line, not as a block)
/// and when no line in the range is *constraining* — i.e. no line has both
/// non-whitespace content and at least one leading space. Otherwise the amount
/// is the smallest leading-space count among constraining lines, capped at the
/// indent width `w`.
///
/// Blank / whitespace-only lines and already-flush lines are excluded from the
/// minimum so they never freeze progress toward column 0. Reads the buffer
/// without mutating it.
pub fn block_dedent_amount(
    buffer: &TextBuffer,
    start_line: usize,
    end_line: usize,
    style: &IndentStyle,
) -> usize {
    let IndentStyle::Spaces(w) = *style else {
        return 0;
    };
    let mut min_lead: Option<usize> = None;
    for line_idx in start_line..end_line.min(buffer.len_lines()) {
        let Ok(line) = buffer.line(line_idx) else {
            continue;
        };
        let line_str = line.to_string();
        let lead = leading_space_count(&line_str);
        if lead > 0 && !line_str.trim().is_empty() {
            min_lead = Some(min_lead.map_or(lead, |m| m.min(lead)));
        }
    }
    min_lead.map_or(0, |m| m.min(w))
}

/// Per-line removal for a block dedent of `[start_line, end_line)`, given the
/// already-computed block amount `k`. Kept private so [`dedent_lines`] and
/// [`dedent_removed_for_line`] share one formula.
fn removed_for_line_with_k(
    buffer: &TextBuffer,
    line: usize,
    k: usize,
    style: &IndentStyle,
) -> usize {
    match style {
        IndentStyle::Spaces(_) => {
            if k == 0 {
                return 0;
            }
            buffer
                .line(line)
                .map_or(0, |l| k.min(leading_space_count(&l.to_string())))
        }
        IndentStyle::Tabs => leading_indent_removable(buffer, line, style),
    }
}

/// Number of leading-whitespace chars a dedent of the block `[start_line,
/// end_line)` removes from `line`.
///
/// For [`IndentStyle::Spaces`] this is `min(block amount, leading spaces on the
/// line)`, a uniform block outdent that preserves the relative indentation
/// between lines (see [`block_dedent_amount`]). For [`IndentStyle::Tabs`] it is
/// the unchanged per-line [`leading_indent_removable`].
///
/// Returns `0` when `line` is out of bounds. Callers editing many lines should
/// prefer [`dedent_lines`] (which computes the block amount once); this helper
/// recomputes it per call and is intended for the few cursor lines whose
/// columns must track the edit.
pub fn dedent_removed_for_line(
    buffer: &TextBuffer,
    line: usize,
    start_line: usize,
    end_line: usize,
    style: &IndentStyle,
) -> usize {
    let k = block_dedent_amount(buffer, start_line, end_line, style);
    removed_for_line_with_k(buffer, line, k, style)
}

/// Dedents lines in the given range using the specified indent style.
///
/// `Spaces` uses the block rule from [`block_dedent_amount`] (relative
/// indentation preserved); `Tabs` removes one leading tab per line.
pub fn dedent_lines(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
    style: &IndentStyle,
) -> Result<()> {
    let k = block_dedent_amount(buffer, start_line, end_line, style);
    // Process from last to first to maintain char positions.
    for line_idx in (start_line..end_line.min(buffer.len_lines())).rev() {
        let to_remove = removed_for_line_with_k(buffer, line_idx, k, style);
        if to_remove > 0 {
            let line_start = buffer.line_to_char(line_idx)?;
            buffer.remove(line_start, line_start + to_remove)?;
        }
    }
    Ok(())
}

/// Trims whitespace from each line in `[start_line, end_line)` according to
/// `mode`. Returns the number of lines that changed.
///
/// Returns `Ok(0)` (and leaves the buffer untouched) when the range is empty
/// or out of bounds.
pub fn trim_lines(
    buffer: &mut TextBuffer,
    start_line: usize,
    end_line: usize,
    mode: TrimMode,
) -> Result<usize> {
    if start_line >= end_line || end_line > buffer.len_lines() {
        return Ok(0);
    }

    let lines = read_lines_trimmed(buffer, start_line, end_line)?;
    let trimmed: Vec<&str> = lines
        .iter()
        .map(|l| match mode {
            TrimMode::Trailing => l.trim_end(),
            TrimMode::Leading => l.trim_start(),
            TrimMode::Both => l.trim(),
        })
        .collect();

    let changed = trimmed
        .iter()
        .zip(lines.iter())
        .filter(|(t, original)| **t != original.as_str())
        .count();

    if changed == 0 {
        return Ok(0);
    }

    write_lines_back(buffer, start_line, end_line, &trimmed)?;
    Ok(changed)
}

/// Joins lines in `[start_line, end_line)` into a single line with exactly one
/// space between each pair of joined lines.
///
/// Trailing whitespace of every-line-except-last and leading whitespace of
/// every-line-except-first are stripped before joining, so each junction has
/// exactly one ASCII space. The leading whitespace of the first line and the
/// trailing whitespace of the last line are preserved verbatim.
///
/// Returns the number of newlines collapsed (`end_line - start_line - 1` when
/// the operation runs, `0` when the range covers fewer than 2 lines or is out
/// of bounds).
///
/// # Errors
///
/// Propagates buffer access errors from the underlying read/write helpers.
pub fn join_lines(buffer: &mut TextBuffer, start_line: usize, end_line: usize) -> Result<usize> {
    if end_line > buffer.len_lines() || end_line.saturating_sub(start_line) < 2 {
        return Ok(0);
    }

    let lines = read_lines_trimmed(buffer, start_line, end_line)?;
    let last_idx = lines.len() - 1;

    let mut joined =
        String::with_capacity(lines.iter().map(String::len).sum::<usize>() + lines.len());
    for (i, line) in lines.iter().enumerate() {
        let slice = match (i == 0, i == last_idx) {
            (true, true) => line.as_str(), // single line — unreachable due to len check
            (true, false) => line.trim_end(), // first: keep leading, drop trailing
            (false, true) => line.trim_start(), // last: keep trailing, drop leading
            (false, false) => line.trim(), // middle: drop both
        };
        if i > 0 {
            joined.push(' ');
        }
        joined.push_str(slice);
    }

    write_lines_back(buffer, start_line, end_line, &[joined.as_str()])?;
    Ok(last_idx)
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
        // Block dedent removes the common minimum (2), capped at width (4), from
        // every line — preserving the 2-space gap between the lines rather than
        // smushing both to column 0.
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("  a\n    b");
        dedent_lines(&mut buf, 0, 2, &style).unwrap();
        assert_eq!(buf.to_string(), "a\n  b");
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
    fn leading_indent_removable_spaces_tabs_mixed() {
        // Spaces style: removable is min(leading spaces, width).
        let buf = TextBuffer::from("      a\n  b\nc");
        assert_eq!(
            leading_indent_removable(&buf, 0, &IndentStyle::Spaces(4)),
            4
        );
        assert_eq!(
            leading_indent_removable(&buf, 1, &IndentStyle::Spaces(4)),
            2
        );
        assert_eq!(
            leading_indent_removable(&buf, 2, &IndentStyle::Spaces(4)),
            0
        );

        // Tabs style: a leading tab counts as 1; otherwise up to 4 spaces.
        let buf = TextBuffer::from("\ta\n      b\nc");
        assert_eq!(leading_indent_removable(&buf, 0, &IndentStyle::Tabs), 1);
        assert_eq!(leading_indent_removable(&buf, 1, &IndentStyle::Tabs), 4);
        assert_eq!(leading_indent_removable(&buf, 2, &IndentStyle::Tabs), 0);

        // Out of bounds is 0.
        assert_eq!(leading_indent_removable(&buf, 99, &IndentStyle::Tabs), 0);
    }

    #[test]
    fn test_dedent_no_indent_noop() {
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("abc\ndef");
        dedent_lines(&mut buf, 0, 2, &style).unwrap();
        assert_eq!(buf.to_string(), "abc\ndef");
    }

    // ── block dedent (relative-indentation preserving) ───────────────

    #[test]
    fn block_dedent_preserves_relative_gaps() {
        // The brief's JSON: leading spaces (1, 2, 3, 1). Each Shift+Tab removes
        // the common minimum, capped at width, preserving the ff/cc gap and
        // marching to column 0.
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from(" {\n  \"ff\":23,\n   \"cc\":32\n }");

        dedent_lines(&mut buf, 0, 4, &style).unwrap();
        assert_eq!(buf.to_string(), "{\n \"ff\":23,\n  \"cc\":32\n}");

        dedent_lines(&mut buf, 0, 4, &style).unwrap();
        assert_eq!(buf.to_string(), "{\n\"ff\":23,\n \"cc\":32\n}");

        dedent_lines(&mut buf, 0, 4, &style).unwrap();
        assert_eq!(buf.to_string(), "{\n\"ff\":23,\n\"cc\":32\n}");

        // Fully flush: a further dedent is a no-op.
        dedent_lines(&mut buf, 0, 4, &style).unwrap();
        assert_eq!(buf.to_string(), "{\n\"ff\":23,\n\"cc\":32\n}");
    }

    #[test]
    fn block_dedent_uniform_indent_strips_full_level() {
        // Consistently-indented code still loses a whole level per press, and
        // the deeper line keeps its +4 nesting.
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("    a\n        b\n    c");
        dedent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "a\n    b\nc");
    }

    #[test]
    fn block_dedent_ignores_blank_and_flush_lines() {
        // A blank line and an already-flush content line must not freeze the
        // block amount at 0 — the indented lines still dedent by their common
        // minimum (4), preserving the 2-space gap between them.
        let style = IndentStyle::Spaces(4);
        let mut buf = TextBuffer::from("flush\n\n    a\n      b");
        dedent_lines(&mut buf, 0, 4, &style).unwrap();
        assert_eq!(buf.to_string(), "flush\n\na\n  b");
    }

    #[test]
    fn block_dedent_amount_reports_capped_minimum() {
        let buf = TextBuffer::from("  a\n      b\nc");
        // min leading among constraining lines is 2, width caps at 4 → 2.
        assert_eq!(block_dedent_amount(&buf, 0, 3, &IndentStyle::Spaces(4)), 2);
        // Width 1 caps the amount below the minimum.
        assert_eq!(block_dedent_amount(&buf, 0, 3, &IndentStyle::Spaces(1)), 1);
    }

    #[test]
    fn block_dedent_amount_zero_without_constraining_lines() {
        // A flush content line plus a blank line contribute no constraint.
        let buf = TextBuffer::from("a\n   \nb");
        assert_eq!(block_dedent_amount(&buf, 0, 3, &IndentStyle::Spaces(4)), 0);
        // Tabs never report a block amount (per-line rule).
        let tabbed = TextBuffer::from("\ta\n\t\tb");
        assert_eq!(block_dedent_amount(&tabbed, 0, 2, &IndentStyle::Tabs), 0);
    }

    #[test]
    fn block_dedent_tabs_unchanged() {
        // Tabs dedent one tab per line — deeper lines stay deeper.
        let style = IndentStyle::Tabs;
        let mut buf = TextBuffer::from("\ta\n\t\tb\n\t\t\tc");
        dedent_lines(&mut buf, 0, 3, &style).unwrap();
        assert_eq!(buf.to_string(), "a\n\tb\n\t\tc");
    }

    #[test]
    fn dedent_removed_for_line_matches_block_edit() {
        // The per-line helper the cursor-delta math uses must agree with what
        // dedent_lines actually removes.
        let style = IndentStyle::Spaces(4);
        let buf = TextBuffer::from(" {\n  \"ff\":23,\n   \"cc\":32\n }");
        // k = min(1,2,3) capped at 4 = 1; every constraining line loses 1.
        assert_eq!(dedent_removed_for_line(&buf, 0, 0, 4, &style), 1);
        assert_eq!(dedent_removed_for_line(&buf, 1, 0, 4, &style), 1);
        assert_eq!(dedent_removed_for_line(&buf, 2, 0, 4, &style), 1);
        assert_eq!(dedent_removed_for_line(&buf, 3, 0, 4, &style), 1);
        // Out-of-bounds line removes nothing.
        assert_eq!(dedent_removed_for_line(&buf, 99, 0, 4, &style), 0);
    }

    // ── trim_lines ───────────────────────────────────────────────────

    #[test]
    fn trim_lines_trailing_strips_only_end() {
        let mut buf = TextBuffer::from("hello   ");
        let changed = trim_lines(&mut buf, 0, 1, TrimMode::Trailing).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn trim_lines_leading_strips_only_start() {
        let mut buf = TextBuffer::from("   hello");
        let changed = trim_lines(&mut buf, 0, 1, TrimMode::Leading).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn trim_lines_both_strips_both_ends() {
        let mut buf = TextBuffer::from("   hello   ");
        let changed = trim_lines(&mut buf, 0, 1, TrimMode::Both).unwrap();
        assert_eq!(changed, 1);
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn trim_lines_preserves_inner_whitespace() {
        let mut buf = TextBuffer::from("  a  b  ");
        trim_lines(&mut buf, 0, 1, TrimMode::Both).unwrap();
        assert_eq!(buf.to_string(), "a  b");
    }

    #[test]
    fn trim_lines_multiline_processes_each_line() {
        let mut buf = TextBuffer::from("  one  \n  two  \n  three  ");
        let changed = trim_lines(&mut buf, 0, 3, TrimMode::Both).unwrap();
        assert_eq!(changed, 3);
        assert_eq!(buf.to_string(), "one\ntwo\nthree");

        let mut buf = TextBuffer::from("  one  \n  two  \n  three  ");
        trim_lines(&mut buf, 0, 3, TrimMode::Trailing).unwrap();
        assert_eq!(buf.to_string(), "  one\n  two\n  three");

        let mut buf = TextBuffer::from("  one  \n  two  \n  three  ");
        trim_lines(&mut buf, 0, 3, TrimMode::Leading).unwrap();
        assert_eq!(buf.to_string(), "one  \ntwo  \nthree  ");
    }

    #[test]
    fn trim_lines_no_change_returns_zero() {
        let mut buf = TextBuffer::from("clean\nlines");
        let changed = trim_lines(&mut buf, 0, 2, TrimMode::Both).unwrap();
        assert_eq!(changed, 0);
        assert_eq!(buf.to_string(), "clean\nlines");
    }

    #[test]
    fn trim_lines_partial_range_only_touches_range() {
        let mut buf = TextBuffer::from("  a  \n  b  \n  c  \n  d  ");
        trim_lines(&mut buf, 1, 3, TrimMode::Both).unwrap();
        assert_eq!(buf.to_string(), "  a  \nb\nc\n  d  ");
    }

    #[test]
    fn trim_lines_empty_lines_remain_empty() {
        let mut buf = TextBuffer::from("\n\n");
        let changed = trim_lines(&mut buf, 0, 3, TrimMode::Both).unwrap();
        assert_eq!(changed, 0);
        assert_eq!(buf.to_string(), "\n\n");
    }

    #[test]
    fn trim_lines_tabs_treated_as_whitespace() {
        let mut buf = TextBuffer::from("\t\thello\t");
        trim_lines(&mut buf, 0, 1, TrimMode::Trailing).unwrap();
        assert_eq!(buf.to_string(), "\t\thello");

        let mut buf = TextBuffer::from("\t\thello\t");
        trim_lines(&mut buf, 0, 1, TrimMode::Leading).unwrap();
        assert_eq!(buf.to_string(), "hello\t");

        let mut buf = TextBuffer::from("\t\thello\t");
        trim_lines(&mut buf, 0, 1, TrimMode::Both).unwrap();
        assert_eq!(buf.to_string(), "hello");
    }

    #[test]
    fn trim_lines_out_of_bounds_is_noop() {
        let mut buf = TextBuffer::from("  a  ");
        let changed = trim_lines(&mut buf, 0, 0, TrimMode::Both).unwrap();
        assert_eq!(changed, 0);
        assert_eq!(buf.to_string(), "  a  ");

        let changed = trim_lines(&mut buf, 5, 10, TrimMode::Both).unwrap();
        assert_eq!(changed, 0);
        assert_eq!(buf.to_string(), "  a  ");
    }

    // ── join_lines ───────────────────────────────────────────────────

    #[test]
    fn join_lines_two_lines_inserts_single_space() {
        let mut buf = TextBuffer::from("hello\nworld");
        assert_eq!(join_lines(&mut buf, 0, 2).unwrap(), 1);
        assert_eq!(buf.to_string(), "hello world");
    }

    #[test]
    fn join_lines_three_lines_inserts_two_spaces() {
        let mut buf = TextBuffer::from("a\nb\nc");
        assert_eq!(join_lines(&mut buf, 0, 3).unwrap(), 2);
        assert_eq!(buf.to_string(), "a b c");
    }

    #[test]
    fn join_lines_trims_inner_whitespace_to_single_space() {
        let mut buf = TextBuffer::from("hello   \n   world");
        join_lines(&mut buf, 0, 2).unwrap();
        assert_eq!(buf.to_string(), "hello world");
    }

    #[test]
    fn join_lines_preserves_leading_of_first_line() {
        let mut buf = TextBuffer::from("    hello\nworld");
        join_lines(&mut buf, 0, 2).unwrap();
        assert_eq!(buf.to_string(), "    hello world");
    }

    #[test]
    fn join_lines_preserves_trailing_of_last_line() {
        let mut buf = TextBuffer::from("hello\nworld    ");
        join_lines(&mut buf, 0, 2).unwrap();
        assert_eq!(buf.to_string(), "hello world    ");
    }

    #[test]
    fn join_lines_middle_line_trimmed_both_sides() {
        let mut buf = TextBuffer::from("a\n   b   \nc");
        join_lines(&mut buf, 0, 3).unwrap();
        assert_eq!(buf.to_string(), "a b c");
    }

    #[test]
    fn join_lines_partial_range_only_joins_range() {
        let mut buf = TextBuffer::from("a\nb\nc\nd");
        assert_eq!(join_lines(&mut buf, 1, 3).unwrap(), 1);
        assert_eq!(buf.to_string(), "a\nb c\nd");
    }

    #[test]
    fn join_lines_single_line_range_is_noop() {
        let mut buf = TextBuffer::from("abc");
        assert_eq!(join_lines(&mut buf, 0, 1).unwrap(), 0);
        assert_eq!(buf.to_string(), "abc");
    }

    #[test]
    fn join_lines_zero_line_range_is_noop() {
        let mut buf = TextBuffer::from("");
        assert_eq!(join_lines(&mut buf, 0, 0).unwrap(), 0);
        assert_eq!(buf.to_string(), "");
    }

    #[test]
    fn join_lines_out_of_bounds_is_noop() {
        let mut buf = TextBuffer::from("abc");
        assert_eq!(join_lines(&mut buf, 0, 10).unwrap(), 0);
        assert_eq!(buf.to_string(), "abc");
    }

    #[test]
    fn join_lines_empty_lines_collapse_to_single_space() {
        // lines = ["a", "", "b"]; the empty middle line preserves both
        // junctions, yielding two spaces. Documents the chosen behavior.
        let mut buf = TextBuffer::from("a\n\nb");
        join_lines(&mut buf, 0, 3).unwrap();
        assert_eq!(buf.to_string(), "a  b");
    }
}
