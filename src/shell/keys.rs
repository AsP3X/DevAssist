//! A key the user chose by name, such as `ctrl-p` or `alt-f2`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use devassist_core::keys::Key;
pub use devassist_core::keys::KeyCombo;

/// Whether this is the key that was pressed.
pub fn matches(combo: &KeyCombo, key: KeyEvent) -> bool {
    let held = |modifier| key.modifiers.contains(modifier);
    let same_key = match (combo.key, key.code) {
        (Key::Char(wanted), KeyCode::Char(pressed)) => wanted == pressed.to_ascii_lowercase(),
        (Key::F(wanted), KeyCode::F(pressed)) => wanted == pressed,
        _ => false,
    };
    // Shift is only compared where it was asked for or changes nothing
    // else: a terminal reports ctrl-shift-p as an upper-case P.
    same_key
        && combo.ctrl == held(KeyModifiers::CONTROL)
        && combo.alt == held(KeyModifiers::ALT)
        && (combo.shift == held(KeyModifiers::SHIFT) || matches!(combo.key, Key::Char(_)))
        && (!combo.shift || held(KeyModifiers::SHIFT))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn only_the_named_key_with_its_modifiers_matches() {
        let combo = KeyCombo::default();
        assert!(matches(
            &combo,
            pressed(KeyCode::Char('p'), KeyModifiers::CONTROL)
        ));
        // Some terminals report the letter in upper case, or with shift.
        assert!(matches(
            &combo,
            pressed(KeyCode::Char('P'), KeyModifiers::CONTROL)
        ));
        assert!(matches(
            &combo,
            pressed(
                KeyCode::Char('P'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )
        ));
        assert!(!matches(
            &combo,
            pressed(KeyCode::Char('p'), KeyModifiers::NONE)
        ));
        assert!(!matches(
            &combo,
            pressed(KeyCode::Char('p'), KeyModifiers::ALT)
        ));
        assert!(!matches(
            &combo,
            pressed(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL | KeyModifiers::ALT
            )
        ));
        assert!(!matches(
            &combo,
            pressed(KeyCode::Char('k'), KeyModifiers::CONTROL)
        ));

        let f2 = KeyCombo::parse("f2").unwrap();
        assert!(matches(&f2, pressed(KeyCode::F(2), KeyModifiers::NONE)));
        assert!(!matches(&f2, pressed(KeyCode::F(2), KeyModifiers::SHIFT)));
        assert!(!matches(&f2, pressed(KeyCode::F(3), KeyModifiers::NONE)));

        let shifted = KeyCombo::parse("ctrl-shift-k").unwrap();
        assert!(matches(
            &shifted,
            pressed(
                KeyCode::Char('K'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT
            )
        ));
        assert!(!matches(
            &shifted,
            pressed(KeyCode::Char('k'), KeyModifiers::CONTROL)
        ));
    }
}
