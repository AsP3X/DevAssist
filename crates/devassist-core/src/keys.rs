//! A key the user chose by name, such as `ctrl-p` or `alt-f2`.

/// A key, apart from the modifiers held with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    F(u8),
}

/// One key with the modifiers held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyCombo {
    pub key: Key,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Default for KeyCombo {
    /// `ctrl-p`.
    fn default() -> Self {
        Self {
            key: Key::Char('p'),
            ctrl: true,
            alt: false,
            shift: false,
        }
    }
}

impl KeyCombo {
    /// Reads a name: modifiers `ctrl`, `alt` and `shift`, then a letter,
    /// a digit or `f1` to `f12`, joined with `-` or `+`. A letter or digit
    /// needs `ctrl` or `alt`: alone it would be taken from every text.
    pub fn parse(name: &str) -> Result<Self, String> {
        let lower = name.trim().to_ascii_lowercase();
        let mut parts: Vec<&str> = lower.split(['-', '+']).map(str::trim).collect();
        let Some(key) = parts.pop().filter(|key| !key.is_empty()) else {
            return Err("name a key, such as ctrl-p or f2".to_string());
        };
        let (mut ctrl, mut alt, mut shift) = (false, false, false);
        for part in parts {
            match part {
                "ctrl" | "control" => ctrl = true,
                "alt" => alt = true,
                "shift" => shift = true,
                other => return Err(format!("{other:?} is not ctrl, alt or shift")),
            }
        }
        let function = key
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .filter(|number| (1..=12).contains(number));
        let mut letters = key.chars();
        let key = match (function, letters.next(), letters.next()) {
            (Some(number), _, _) => Key::F(number),
            (None, Some(ch), None) if ch.is_ascii_alphanumeric() => Key::Char(ch),
            _ => return Err(format!("{key:?} is not a letter, a digit or f1 to f12")),
        };
        if matches!(key, Key::Char(_)) && !ctrl && !alt {
            return Err("a letter or digit needs ctrl or alt, such as ctrl-k".to_string());
        }
        if ctrl && !alt && key == Key::Char('c') {
            return Err("ctrl-c copies and quits; choose another key".to_string());
        }
        Ok(Self {
            key,
            ctrl,
            alt,
            shift,
        })
    }

    /// The name as hints show it and as the settings file holds it.
    pub fn label(&self) -> String {
        let mut label = String::new();
        if self.ctrl {
            label.push_str("ctrl-");
        }
        if self.alt {
            label.push_str("alt-");
        }
        if self.shift {
            label.push_str("shift-");
        }
        match self.key {
            Key::F(number) => label.push_str(&format!("f{number}")),
            Key::Char(ch) => label.push(ch),
        }
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_read_whatever_their_case_and_joiner() {
        let combo = KeyCombo::parse("Ctrl-P").unwrap();
        assert_eq!(combo, KeyCombo::default());
        assert_eq!(combo.label(), "ctrl-p");
        assert_eq!(KeyCombo::parse("ctrl+k").unwrap().label(), "ctrl-k");
        assert_eq!(
            Key::F(2),
            KeyCombo::parse(" f2 ").unwrap().key,
            "a function key needs no modifier"
        );
        assert_eq!(
            KeyCombo::parse("alt-shift-f12").unwrap().label(),
            "alt-shift-f12"
        );
        assert_eq!(KeyCombo::parse("ctrl-alt-1").unwrap().label(), "ctrl-alt-1");
    }

    #[test]
    fn a_name_that_cannot_work_is_refused_with_the_reason() {
        let refused = |name: &str| KeyCombo::parse(name).unwrap_err();
        assert!(refused("p").contains("needs ctrl or alt"));
        assert!(refused("shift-p").contains("needs ctrl or alt"));
        assert!(refused("ctrl-c").contains("copies and quits"));
        assert!(refused("super-p").contains("is not ctrl, alt or shift"));
        assert!(refused("ctrl-enter").contains("is not a letter"));
        assert!(refused("f13").contains("is not a letter"));
        assert!(refused("").contains("name a key"));
        assert!(refused("ctrl-").contains("name a key"));
    }
}
