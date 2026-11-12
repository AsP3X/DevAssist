//! Fitting text into a number of terminal cells.

use unicode_width::UnicodeWidthStr;

/// Two facts on one line. An empty one leaves no separator behind.
pub fn join_facts(first: &str, second: &str) -> String {
    match (first.is_empty(), second.is_empty()) {
        (false, false) => format!("{first}  ·  {second}"),
        (false, true) => first.to_string(),
        _ => second.to_string(),
    }
}

/// Each cell fitted to its column and the whole fitted to `total`. A
/// column of width 0 is left out.
pub fn padded_row(cells: &[&str], widths: &[usize], total: usize) -> String {
    let mut out = String::new();
    for (index, cell) in cells.iter().enumerate() {
        if widths[index] == 0 {
            continue;
        }
        out.push_str(&fit(cell, widths[index]));
    }
    fit(&out, total)
}

/// `text` in at most `width` cells, keeping its end. Not padded.
pub fn fit_end(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut kept = String::new();
    let mut used = 0;
    for ch in text.chars().rev() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width - 1 {
            break;
        }
        kept.push(ch);
        used += ch_width;
    }
    let kept: String = kept.chars().rev().collect();
    format!("…{kept}")
}

/// `text` in exactly `width` cells: padded with spaces, or cut with "…".
pub fn fit(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let current = UnicodeWidthStr::width(text);
    if current <= width {
        return format!("{text}{}", " ".repeat(width - current));
    }
    if width == 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + ch_width > width - 1 {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    let used = UnicodeWidthStr::width(out.as_str());
    if used < width {
        out.push_str(&" ".repeat(width - used));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_pads_short_text_and_cuts_long_text() {
        assert_eq!(fit("ab", 4), "ab  ");
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 1), "…");
        assert_eq!(fit("abc", 0), "");
        // A wide character that does not fit leaves a padded gap.
        assert_eq!(fit("中文字", 4), "中… ");
    }

    #[test]
    fn fit_end_keeps_the_end_and_does_not_pad() {
        assert_eq!(fit_end("ab", 4), "ab");
        assert_eq!(fit_end("abcdef", 4), "…def");
        assert_eq!(fit_end("abc", 1), "…");
        assert_eq!(fit_end("abc", 0), "");
    }

    #[test]
    fn padded_row_skips_columns_of_no_width() {
        assert_eq!(padded_row(&["a", "bb", "c"], &[2, 0, 3], 6), "a c   ");
        assert_eq!(padded_row(&["abcdef", "x"], &[4, 2], 6), "abc…x ");
    }

    #[test]
    fn join_facts_leaves_no_separator_behind() {
        assert_eq!(join_facts("a", "b"), "a  ·  b");
        assert_eq!(join_facts("a", ""), "a");
        assert_eq!(join_facts("", "b"), "b");
        assert_eq!(join_facts("", ""), "");
    }
}
