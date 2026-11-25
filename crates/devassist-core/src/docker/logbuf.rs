//! Lines that keep arriving: the newest are followed, the reader can stay
//! back, and a filter lets through only the lines that match it.

use std::collections::VecDeque;
use std::iter::Peekable;
use std::str::Chars;

/// How many lines are kept. The oldest make room for new ones.
pub const CAP: usize = 5_000;

/// The lines, and where the reader is in them.
pub struct LogBuffer {
    pub title: String,
    lines: VecDeque<String>,
    /// The newest line stays on screen.
    pub follow: bool,
    /// The first line shown, counted among the lines that match.
    pub scroll: usize,
    pub filter: String,
    /// The filter field has the keyboard.
    pub typing: bool,
    /// Why no more lines will come, once that is so.
    pub ended: Option<String>,
}

impl LogBuffer {
    /// An empty log that follows what arrives.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            lines: VecDeque::new(),
            follow: true,
            scroll: 0,
            filter: String::new(),
            typing: false,
            ended: None,
        }
    }

    /// Adds a line. Escape sequences, such as the ones that colour text,
    /// and other control characters are left out; a tab becomes four
    /// spaces.
    pub fn push(&mut self, line: &str) {
        if self.lines.len() >= CAP
            && let Some(dropped) = self.lines.pop_front()
            && !self.follow
            && matches(&dropped, &self.filter.to_lowercase())
        {
            // The lines on screen stay where they are.
            self.scroll = self.scroll.saturating_sub(1);
        }
        self.lines.push_back(clean(line));
    }

    /// The stream is over. The note, such as "ended" or an error, is
    /// shown in the title and before the hints.
    pub fn end(&mut self, note: impl Into<String>) {
        self.ended = Some(note.into());
    }

    /// True while the filter field has the keyboard.
    pub fn typing(&self) -> bool {
        self.typing
    }

    /// Leaves the filter field and drops what was typed in it.
    pub fn drop_typed_filter(&mut self) {
        self.typing = false;
        self.set_filter(String::new());
    }

    /// Adds pasted text to the filter while it is being typed.
    pub fn paste(&mut self, text: &str) {
        if !self.typing {
            return;
        }
        let mut filter = self.filter.clone();
        filter.extend(text.chars().filter(|ch| !ch.is_control()));
        self.set_filter(filter);
    }

    /// The lines the filter lets through, oldest first.
    pub fn shown(&self) -> impl Iterator<Item = &str> {
        let needle = self.filter.to_lowercase();
        self.lines
            .iter()
            .filter(move |line| matches(line, &needle))
            .map(String::as_str)
    }

    /// The last row while the filter is not being typed. `shown` is how
    /// many lines match.
    pub fn status(&self, shown: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(note) = &self.ended {
            parts.push(note.clone());
        }
        parts.push(if self.follow {
            "following".to_string()
        } else {
            "paused · end follows".to_string()
        });
        let total = self.lines.len();
        let unit = if total == 1 { "line" } else { "lines" };
        if self.filter.is_empty() {
            parts.push(format!("{} {unit}", grouped(total)));
            parts.push("/ filter · esc close".to_string());
        } else {
            parts.push(format!("{} of {} {unit}", grouped(shown), grouped(total)));
            parts.push(format!("filter: {}", self.filter));
            parts.push("/ filter · esc clear".to_string());
        }
        parts.join(" · ")
    }

    /// How many lines match the filter.
    pub fn count(&self) -> usize {
        if self.filter.is_empty() {
            return self.lines.len();
        }
        let needle = self.filter.to_lowercase();
        self.lines
            .iter()
            .filter(|line| matches(line, &needle))
            .count()
    }

    /// Changes the filter and goes to the newest line that matches it.
    pub fn set_filter(&mut self, filter: String) {
        if filter != self.filter {
            self.filter = filter;
            self.follow = true;
        }
    }
}

/// Whether `line` holds `needle`, which is in lowercase, in any case.
fn matches(line: &str, needle: &str) -> bool {
    needle.is_empty() || line.to_lowercase().contains(needle)
}

/// `line` without escape sequences and control characters, and with four
/// spaces for each tab.
fn clean(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => skip_escape(&mut chars),
            '\t' => out.push_str("    "),
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}

/// Skips the rest of an escape sequence whose Esc was just read.
fn skip_escape(chars: &mut Peekable<Chars>) {
    match chars.next() {
        // A control sequence, such as a colour: up to its final letter.
        Some('[') => {
            for ch in chars.by_ref() {
                if ('@'..='~').contains(&ch) {
                    break;
                }
            }
        }
        // A string, such as a window title or a link: up to a bell or
        // to Esc and a backslash.
        Some(']' | 'P' | 'X' | '^' | '_') => {
            while let Some(ch) = chars.next() {
                if ch == '\x07' {
                    break;
                }
                if ch == '\x1b' {
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
            }
        }
        // Any other sequence: its in-between characters and one more.
        Some(ch) if (' '..='/').contains(&ch) => {
            for ch in chars.by_ref() {
                if !(' '..='/').contains(&ch) {
                    break;
                }
            }
        }
        _ => {}
    }
}

/// A number with a comma between its thousands.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A log holding `line 0` to `line <count - 1>`.
    fn numbered(count: usize) -> LogBuffer {
        let mut view = LogBuffer::new("logs api");
        for index in 0..count {
            view.push(&format!("line {index}"));
        }
        view
    }

    #[test]
    fn the_cap_drops_the_oldest_lines() {
        let view = numbered(CAP + 10);
        assert_eq!(view.lines.len(), CAP);
        assert_eq!(view.lines.front().map(String::as_str), Some("line 10"));
        let last = format!("line {}", CAP + 9);
        assert_eq!(view.lines.back(), Some(&last));
    }

    #[test]
    fn colour_codes_and_control_characters_are_stripped() {
        let mut view = LogBuffer::new("logs");
        view.push("\x1b[31mred\x1b[0m");
        view.push("\x1b[1;32mok\x1b[0m done\r");
        view.push("\x1b]0;window title\x07shown");
        view.push("\x1b]8;;http://example.test\x1b\\link\x1b]8;;\x1b\\ after");
        view.push("a\tb\x07\x08");
        view.push("\x1b(Bplain\x1b");
        let lines: Vec<&str> = view.lines.iter().map(String::as_str).collect();
        assert_eq!(
            lines,
            vec!["red", "ok done", "shown", "link after", "a    b", "plain"]
        );
    }
}
