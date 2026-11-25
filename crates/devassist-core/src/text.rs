//! Fitting text into a number of cells, the way the terminal app and the window both do it.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);
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

/// A path cut to `width` characters by leaving out its middle: the drive
/// or root stays, and as many of the last folders as fit, such as
/// `C:\…\work\api`. The end says the most, so that is what is kept.
pub fn short_path(path: &str, width: usize) -> String {
    if UnicodeWidthStr::width(path) <= width {
        return path.to_string();
    }
    let separator = |ch: char| ch == '\\' || ch == '/';
    // The root: "C:\", "/" or nothing for a relative path.
    let root: String = match path.find(separator) {
        Some(at) if at <= 2 => path[..=at].to_string(),
        _ => String::new(),
    };
    let room = width.saturating_sub(UnicodeWidthStr::width(root.as_str()) + 1);
    // The longest end that fits.
    let mut tail = String::new();
    let mut used = 0;
    for ch in path.chars().rev() {
        let wide = UnicodeWidthChar::width(ch).unwrap_or(0);
        if used + wide > room {
            break;
        }
        used += wide;
        tail.insert(0, ch);
    }
    // Start it at a folder, unless that would throw most of it away.
    if !tail.starts_with(separator)
        && let Some(at) = tail.find(separator)
        && at * 2 <= tail.len()
    {
        tail = tail[at..].to_string();
    }
    if tail.is_empty() {
        return fit(path, width).trim_end().to_string();
    }
    format!("{root}…{tail}")
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
        assert_eq!(fit("中文字", 4), "中… ");
    }

    #[test]
    fn a_long_path_loses_its_middle() {
        let long = r"C:\Users\someone\Documents\projects\work\api";
        assert_eq!(short_path(long, 60), long);
        assert_eq!(short_path(long, 20), r"C:\…\work\api");
        assert_eq!(short_path(long, 12), r"C:\…\api");
        assert_eq!(short_path("/srv/code/projects/work/api", 16), "/…/work/api");
        assert!(UnicodeWidthStr::width(short_path(long, 7).as_str()) <= 7);
        assert_eq!(short_path("development/api", 30), "development/api");
        assert_eq!(short_path("", 12), "");
    }
}
