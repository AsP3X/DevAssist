//! The settings dialog's form: what is typed, which field has focus, and
//! how the text becomes values for the settings file.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::{self, Saved};
use crate::github::RepoSpec;
use crate::gitops::Protocol;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Accounts,
    Pinned,
    Protocol,
    Title,
}

impl Field {
    pub const ALL: [Field; 4] = [
        Field::Accounts,
        Field::Pinned,
        Field::Protocol,
        Field::Title,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Accounts => "Accounts",
            Field::Pinned => "Pinned repos",
            Field::Protocol => "Clone with",
            Field::Title => "Title",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Field::Accounts => "Users or organizations that Add lists. Separate with commas.",
            Field::Pinned => "Single repositories to list as well, as owner/name.",
            Field::Protocol => "auto follows the clones already in this folder.",
            Field::Title => "Shown in the top left. Empty uses the folder name.",
        }
    }

    /// Name used for mouse hit areas.
    pub fn key(self) -> &'static str {
        match self {
            Field::Accounts => "accounts",
            Field::Pinned => "pinned",
            Field::Protocol => "protocol",
            Field::Title => "title",
        }
    }
}

pub const PROTOCOLS: [Protocol; 3] = [Protocol::Auto, Protocol::Https, Protocol::Ssh];

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Save,
    Close,
}

pub struct Form {
    /// First launch: the dialog introduces itself and Esc skips the setup.
    pub welcome: bool,
    pub focus: Field,
    pub accounts: String,
    pub pinned: String,
    pub protocol: Protocol,
    pub title: String,
    pub error: Option<String>,
    pub path: PathBuf,
    accounts_edited: bool,
}

impl Form {
    pub fn new(
        welcome: bool,
        owners: &[String],
        pinned: &[RepoSpec],
        protocol: Protocol,
        title: Option<&str>,
        path: PathBuf,
    ) -> Self {
        let pinned: Vec<String> = pinned.iter().map(RepoSpec::id).collect();
        Self {
            welcome,
            focus: Field::Accounts,
            accounts: owners.join(", "),
            pinned: pinned.join(", "),
            protocol,
            title: title.unwrap_or_default().to_string(),
            error: None,
            path,
            accounts_edited: false,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => return Outcome::Save,
            KeyCode::Char('s') if ctrl => return Outcome::Save,
            KeyCode::Tab | KeyCode::Down => self.step(1),
            KeyCode::BackTab | KeyCode::Up => self.step(-1),
            KeyCode::Left if self.focus == Field::Protocol => self.cycle_protocol(-1),
            KeyCode::Right if self.focus == Field::Protocol => self.cycle_protocol(1),
            KeyCode::Backspace => self.edit(|text| {
                text.pop();
            }),
            KeyCode::Char('u') if ctrl => self.edit(String::clear),
            KeyCode::Char('w') if ctrl => self.edit(crate::app::delete_word),
            KeyCode::Char(ch) if !ctrl && !alt => {
                if self.focus == Field::Protocol {
                    match ch.to_ascii_lowercase() {
                        ' ' => self.cycle_protocol(1),
                        'a' => self.protocol = Protocol::Auto,
                        'h' => self.protocol = Protocol::Https,
                        's' => self.protocol = Protocol::Ssh,
                        _ => {}
                    }
                } else {
                    self.edit(|text| text.push(ch));
                }
            }
            _ => {}
        }
        Outcome::Stay
    }

    pub fn paste(&mut self, text: &str) {
        self.edit(|field| field.push_str(text));
    }

    /// A mouse click on a hit area named `field:<key>` or `protocol:<name>`.
    pub fn click(&mut self, name: &str) {
        if let Some(key) = name.strip_prefix("field:") {
            if let Some(field) = Field::ALL.into_iter().find(|field| field.key() == key) {
                self.focus = field;
            }
        } else if let Some(value) = name.strip_prefix("protocol:")
            && let Ok(protocol) = Protocol::parse(value)
        {
            self.focus = Field::Protocol;
            self.protocol = protocol;
        }
    }

    /// Offers the signed-in account while the field is still untouched.
    pub fn suggest_account(&mut self, login: &str) {
        if self.welcome && !self.accounts_edited && self.accounts.trim().is_empty() {
            self.accounts = login.to_string();
        }
    }

    /// Reads the form. A mistake moves the focus to its field and is shown
    /// in the dialog.
    pub fn validate(&mut self) -> Option<Saved> {
        let owners = match config::parse_owner_list(&self.accounts) {
            Ok(owners) => owners,
            Err(err) => return self.reject(Field::Accounts, err),
        };
        let pinned = match config::parse_pinned_list(&self.pinned) {
            Ok(pinned) => pinned,
            Err(err) => return self.reject(Field::Pinned, err),
        };
        let title = self.title.trim();
        self.error = None;
        Some(Saved {
            title: (!title.is_empty()).then(|| title.to_string()),
            owners,
            owners_set: true,
            pinned,
            protocol: self.protocol,
            // The app fills these in: the dialog does not edit them.
            clone_dir: None,
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
        })
    }

    fn reject(&mut self, field: Field, err: String) -> Option<Saved> {
        self.focus = field;
        self.error = Some(err);
        None
    }

    fn step(&mut self, delta: isize) {
        let count = Field::ALL.len() as isize;
        let index = Field::ALL
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0) as isize;
        self.focus = Field::ALL[(index + delta).rem_euclid(count) as usize];
    }

    fn cycle_protocol(&mut self, delta: isize) {
        let count = PROTOCOLS.len() as isize;
        let index = PROTOCOLS
            .iter()
            .position(|protocol| *protocol == self.protocol)
            .unwrap_or(0) as isize;
        self.protocol = PROTOCOLS[(index + delta).rem_euclid(count) as usize];
    }

    fn edit(&mut self, change: impl FnOnce(&mut String)) {
        let text = match self.focus {
            Field::Accounts => {
                self.accounts_edited = true;
                &mut self.accounts
            }
            Field::Pinned => &mut self.pinned,
            Field::Title => &mut self.title,
            Field::Protocol => return,
        };
        change(text);
        self.error = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> Form {
        Form::new(
            false,
            &["AsP3X".to_string()],
            &[],
            Protocol::Auto,
            None,
            PathBuf::from("devassist.toml"),
        )
    }

    fn press(form: &mut Form, code: KeyCode) -> Outcome {
        form.on_key(KeyEvent::new(code, KeyModifiers::NONE))
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
    fn a_mistake_is_reported_on_its_field() {
        let mut form = form();
        form.pinned = "justaname".into();
        form.focus = Field::Title;
        assert!(form.validate().is_none());
        assert_eq!(form.focus, Field::Pinned);
        assert!(form.error.is_some());
        form.pinned = "someone/tool".into();
        form.title = "  Desk ".into();
        let saved = form.validate().unwrap();
        assert_eq!(saved.owners, vec!["AsP3X".to_string()]);
        assert_eq!(saved.pinned[0].id(), "someone/tool");
        assert_eq!(saved.title.as_deref(), Some("Desk"));
    }

    #[test]
    fn the_signed_in_account_fills_only_an_untouched_welcome_form() {
        let mut form = Form::new(
            true,
            &[],
            &[],
            Protocol::Auto,
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
