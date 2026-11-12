//! A key the user chose by name, such as `ctrl-p` or `alt-f2`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// One key with the modifiers held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyCombo {
    code: KeyCode,
    ctrl: bool,
    alt: bool,
    shift: bool,
}

impl Default for KeyCombo {
    /// `ctrl-p`.
    fn default() -> Self {
        Self {
            code: KeyCode::Char('p'),
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
        let mut combo = Self {
            code: KeyCode::Null,
            ctrl: false,
            alt: false,
            shift: false,
        };
        for part in parts {
            match part {
                "ctrl" | "control" => combo.ctrl = true,
                "alt" => combo.alt = true,
                "shift" => combo.shift = true,
                other => return Err(format!("{other:?} is not ctrl, alt or shift")),
            }
        }
        let function = key
            .strip_prefix('f')
            .and_then(|number| number.parse::<u8>().ok())
            .filter(|number| (1..=12).contains(number));
        let mut letters = key.chars();
        combo.code = match (function, letters.next(), letters.next()) {
            (Some(number), _, _) => KeyCode::F(number),
            (None, Some(ch), None) if ch.is_ascii_alphanumeric() => KeyCode::Char(ch),
            _ => return Err(format!("{key:?} is not a letter, a digit or f1 to f12")),
        };
        if matches!(combo.code, KeyCode::Char(_)) && !combo.ctrl && !combo.alt {
            return Err("a letter or digit needs ctrl or alt, such as ctrl-k".to_string());
        }
        if combo.ctrl && !combo.alt && combo.code == KeyCode::Char('c') {
            return Err("ctrl-c copies and quits; choose another key".to_string());
        }
        Ok(combo)
    }

    /// Whether this is the key that was pressed.
    pub fn matches(&self, key: KeyEvent) -> bool {
        let held = |modifier| key.modifiers.contains(modifier);
        let same_key = match (self.code, key.code) {
            (KeyCode::Char(wanted), KeyCode::Char(pressed)) => {
                wanted == pressed.to_ascii_lowercase()
            }
            (wanted, pressed) => wanted == pressed,
        };
        // Shift is only compared where it was asked for or changes nothing
        // else: a terminal reports ctrl-shift-p as an upper-case P.
        same_key
            && self.ctrl == held(KeyModifiers::CONTROL)
            && self.alt == held(KeyModifiers::ALT)
            && (self.shift == held(KeyModifiers::SHIFT) || matches!(self.code, KeyCode::Char(_)))
            && (!self.shift || held(KeyModifiers::SHIFT))
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
        match self.code {
            KeyCode::F(number) => label.push_str(&format!("f{number}")),
            KeyCode::Char(ch) => label.push(ch),
            _ => {}
        }
        label
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn names_are_read_whatever_their_case_and_joiner() {
        let combo = KeyCombo::parse("Ctrl-P").unwrap();
        assert_eq!(combo, KeyCombo::default());
        assert_eq!(combo.label(), "ctrl-p");
        assert_eq!(KeyCombo::parse("ctrl+k").unwrap().label(), "ctrl-k");
        assert_eq!(
            KeyCode::F(2),
            KeyCombo::parse(" f2 ").unwrap().code,
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

    #[test]
    fn only_the_named_key_with_its_modifiers_matches() {
        let combo = KeyCombo::default();
        assert!(combo.matches(pressed(KeyCode::Char('p'), KeyModifiers::CONTROL)));
        // Some terminals report the letter in upper case, or with shift.
        assert!(combo.matches(pressed(KeyCode::Char('P'), KeyModifiers::CONTROL)));
        assert!(combo.matches(pressed(
            KeyCode::Char('P'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )));
        assert!(!combo.matches(pressed(KeyCode::Char('p'), KeyModifiers::NONE)));
        assert!(!combo.matches(pressed(KeyCode::Char('p'), KeyModifiers::ALT)));
        assert!(!combo.matches(pressed(
            KeyCode::Char('p'),
            KeyModifiers::CONTROL | KeyModifiers::ALT
        )));
        assert!(!combo.matches(pressed(KeyCode::Char('k'), KeyModifiers::CONTROL)));

        let f2 = KeyCombo::parse("f2").unwrap();
        assert!(f2.matches(pressed(KeyCode::F(2), KeyModifiers::NONE)));
        assert!(!f2.matches(pressed(KeyCode::F(2), KeyModifiers::SHIFT)));
        assert!(!f2.matches(pressed(KeyCode::F(3), KeyModifiers::NONE)));

        let shifted = KeyCombo::parse("ctrl-shift-k").unwrap();
        assert!(shifted.matches(pressed(
            KeyCode::Char('K'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )));
        assert!(!shifted.matches(pressed(KeyCode::Char('k'), KeyModifiers::CONTROL)));
    }
}
