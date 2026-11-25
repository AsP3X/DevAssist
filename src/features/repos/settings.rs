//! The key handling of the settings dialog. The form's state, and what it
//! turns into, are in `devassist_core::repos::form`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::atoms::ui::input::{self, Alt};
use crate::features::repos::gitops::Protocol;
pub use devassist_core::repos::form::{Field, Form, Outcome, PROTOCOLS};

pub fn on_key(form: &mut Form, key: KeyEvent) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => return Outcome::Close,
        KeyCode::Enter => return Outcome::Save,
        KeyCode::Char('s') if ctrl => return Outcome::Save,
        KeyCode::Tab | KeyCode::Down => form.step(1),
        KeyCode::BackTab | KeyCode::Up => form.step(-1),
        KeyCode::Left if form.focus == Field::Protocol => form.cycle_protocol(-1),
        KeyCode::Right if form.focus == Field::Protocol => form.cycle_protocol(1),
        KeyCode::Left | KeyCode::Right if form.focus == Field::PullRequests => {
            form.pull_requests = !form.pull_requests;
        }
        KeyCode::Char(ch) if !ctrl && !alt && form.focus == Field::PullRequests => {
            match ch.to_ascii_lowercase() {
                ' ' => form.pull_requests = !form.pull_requests,
                'y' => form.pull_requests = true,
                'n' => form.pull_requests = false,
                _ => {}
            }
        }
        KeyCode::Char(ch) if !ctrl && !alt && form.focus == Field::Protocol => {
            match ch.to_ascii_lowercase() {
                ' ' => form.cycle_protocol(1),
                'a' => form.protocol = Protocol::Auto,
                'h' => form.protocol = Protocol::Https,
                's' => form.protocol = Protocol::Ssh,
                _ => {}
            }
        }
        _ => type_key(form, key),
    }
    Outcome::Stay
}

/// An editing key for the field that has the focus.
fn type_key(form: &mut Form, key: KeyEvent) {
    let text = match form.focus {
        Field::Accounts => &mut form.accounts,
        Field::Pinned => &mut form.pinned,
        Field::Watch => &mut form.interval,
        Field::Title => &mut form.title,
        Field::Protocol | Field::PullRequests => return,
    };
    if input::edit(text, key, Alt::Ignored) {
        if form.focus == Field::Accounts {
            form.accounts_edited = true;
        }
        form.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn form() -> Form {
        Form::new(
            false,
            &["AsP3X".to_string()],
            &[],
            Protocol::Auto,
            5,
            None,
            PathBuf::from("devassist.toml"),
        )
    }

    fn press(form: &mut Form, code: KeyCode) -> Outcome {
        on_key(form, KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn typing_goes_to_the_focused_field_and_tab_moves_on() {
        let mut form = form();
        for ch in ", ada".chars() {
            press(&mut form, KeyCode::Char(ch));
        }
        assert_eq!(form.accounts, "AsP3X, ada");
        press(&mut form, KeyCode::Tab);
        assert_eq!(form.focus, Field::Pinned);
        press(&mut form, KeyCode::BackTab);
        press(&mut form, KeyCode::BackTab);
        assert_eq!(form.focus, Field::Title);
        assert_eq!(press(&mut form, KeyCode::Enter), Outcome::Save);
        assert_eq!(press(&mut form, KeyCode::Esc), Outcome::Close);
    }

    #[test]
    fn the_protocol_field_cycles_instead_of_taking_text() {
        let mut form = form();
        form.focus = Field::Protocol;
        press(&mut form, KeyCode::Right);
        assert_eq!(form.protocol, Protocol::Https);
        press(&mut form, KeyCode::Char('s'));
        assert_eq!(form.protocol, Protocol::Ssh);
        press(&mut form, KeyCode::Right);
        assert_eq!(form.protocol, Protocol::Auto);
        form.click("protocol:ssh");
        assert_eq!(form.protocol, Protocol::Ssh);
        form.click("field:title");
        assert_eq!(form.focus, Field::Title);
    }

    #[test]
    fn saving_pull_requests_is_switched_like_the_protocol() {
        let mut form = form();
        assert!(!form.validate().unwrap().pull_requests);
        form.focus = Field::PullRequests;
        press(&mut form, KeyCode::Right);
        assert!(form.pull_requests);
        press(&mut form, KeyCode::Char(' '));
        assert!(!form.pull_requests);
        // Typing does not land in a text field.
        press(&mut form, KeyCode::Char('y'));
        assert!(form.pull_requests);
        assert_eq!(form.accounts, "AsP3X");
        form.click("pulls:off");
        assert!(!form.pull_requests);
        form.click("pulls:on");
        assert_eq!(form.focus, Field::PullRequests);
        assert!(form.validate().unwrap().pull_requests);
    }

    #[test]
    fn the_interval_is_a_number_of_minutes() {
        let mut form = form();
        assert_eq!(form.interval, "5");
        form.focus = Field::Watch;
        press(&mut form, KeyCode::Backspace);
        for ch in "30".chars() {
            press(&mut form, KeyCode::Char(ch));
        }
        assert_eq!(form.validate().unwrap().watch_interval, 30);
        form.interval = "soon".into();
        form.focus = Field::Title;
        assert!(form.validate().is_none());
        assert_eq!(form.focus, Field::Watch);
        form.interval = "0".into();
        assert!(form.validate().is_none());
        // Empty goes back to the default.
        form.interval.clear();
        assert_eq!(form.validate().unwrap().watch_interval, 5);
    }

    #[test]
    fn the_signed_in_account_fills_only_an_untouched_welcome_form() {
        let mut form = Form::new(
            true,
            &[],
            &[],
            Protocol::Auto,
            5,
            None,
            PathBuf::from("devassist.toml"),
        );
        form.suggest_account("AsP3X");
        assert_eq!(form.accounts, "AsP3X");
        press(&mut form, KeyCode::Backspace);
        form.accounts.clear();
        form.suggest_account("AsP3X");
        assert!(form.accounts.is_empty());
    }
}
