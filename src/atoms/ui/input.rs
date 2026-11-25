//! Typing into one line of text. Text is added and removed at its end;
//! there is no cursor to move.

use devassist_core::edit::delete_word;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What a character typed with Alt held does.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Alt {
    /// It is typed. For fields that may need AltGr characters, such as `\`.
    Types,
    /// It is left to the caller.
    Ignored,
}

/// Applies an editing key to `text`. True when the key was an edit, even
/// one that changed nothing, such as Backspace on an empty line.
///
/// Backspace removes the last character, Ctrl-U clears the line, Ctrl-W
/// removes the last word, and a character typed without Ctrl is added.
pub fn edit(text: &mut String, key: KeyEvent, alt: Alt) -> bool {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt_held = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char('u' | 'U') if ctrl => text.clear(),
        KeyCode::Char('w' | 'W') if ctrl => delete_word(text),
        KeyCode::Char(ch) if !ctrl && (alt == Alt::Types || !alt_held) => text.push(ch),
        _ => return false,
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn plain(code: KeyCode) -> KeyEvent {
        key(code, KeyModifiers::NONE)
    }

    fn ctrl(letter: char) -> KeyEvent {
        key(KeyCode::Char(letter), KeyModifiers::CONTROL)
    }

    #[test]
    fn characters_are_added_and_backspace_takes_the_last_one() {
        let mut text = String::new();
        assert!(edit(&mut text, plain(KeyCode::Char('a')), Alt::Ignored));
        let shifted = key(KeyCode::Char('B'), KeyModifiers::SHIFT);
        assert!(edit(&mut text, shifted, Alt::Ignored));
        assert_eq!(text, "aB");
        assert!(edit(&mut text, plain(KeyCode::Backspace), Alt::Ignored));
        assert_eq!(text, "a");
        // Backspace on nothing is still an edit: the caller reacts to it.
        text.clear();
        assert!(edit(&mut text, plain(KeyCode::Backspace), Alt::Ignored));
        assert_eq!(text, "");
    }

    #[test]
    fn ctrl_u_clears_the_line_in_either_case() {
        for letter in ['u', 'U'] {
            let mut text = "hello world".to_string();
            assert!(edit(&mut text, ctrl(letter), Alt::Ignored));
            assert_eq!(text, "");
        }
    }

    #[test]
    fn ctrl_w_removes_the_last_word_and_the_space_before_it() {
        for letter in ['w', 'W'] {
            let mut text = "hello world".to_string();
            assert!(edit(&mut text, ctrl(letter), Alt::Ignored));
            assert_eq!(text, "hello");
        }
        let mut text = "one  two   ".to_string();
        edit(&mut text, ctrl('w'), Alt::Ignored);
        assert_eq!(text, "one");
    }

    #[test]
    fn alt_types_only_where_the_field_asks_for_it() {
        let alt = key(KeyCode::Char('\\'), KeyModifiers::ALT);
        let mut text = String::new();
        assert!(!edit(&mut text, alt, Alt::Ignored));
        assert_eq!(text, "");
        assert!(edit(&mut text, alt, Alt::Types));
        assert_eq!(text, "\\");
        // Ctrl never types, with or without Alt.
        let both = key(
            KeyCode::Char('x'),
            KeyModifiers::CONTROL | KeyModifiers::ALT,
        );
        assert!(!edit(&mut text, both, Alt::Types));
        assert_eq!(text, "\\");
    }

    #[test]
    fn other_keys_are_left_to_the_caller() {
        let mut text = "abc".to_string();
        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::Up,
            KeyCode::Left,
        ] {
            assert!(!edit(&mut text, plain(code), Alt::Types));
        }
        assert!(!edit(&mut text, ctrl('s'), Alt::Types));
        assert_eq!(text, "abc");
    }
}
