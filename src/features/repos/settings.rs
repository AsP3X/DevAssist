//! The settings dialog's form: what is typed, which field has focus, and
//! how the text becomes values for the settings file.

use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::atoms::ui::input::{self, Alt};
use crate::features::repos::github::RepoSpec;
use crate::features::repos::gitops::Protocol;
use crate::shell::config::{self, Saved};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Accounts,
    Pinned,
    Protocol,
    /// Minutes between automatic pulls of the watched repositories.
    Watch,
    /// Whether a fetch also saves the commits of all pull requests.
    PullRequests,
    Title,
}

impl Field {
    pub const ALL: [Field; 6] = [
        Field::Accounts,
        Field::Pinned,
        Field::Protocol,
        Field::Watch,
        Field::PullRequests,
        Field::Title,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Accounts => "Accounts",
            Field::Pinned => "Pinned repos",
            Field::Protocol => "Clone with",
            Field::Watch => "Auto-pull every",
            Field::PullRequests => "Pull requests",
            Field::Title => "Title",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Field::Accounts => "Users or organizations that Add lists. Separate with commas.",
            Field::Pinned => "Single repositories to list as well, as owner/name.",
            Field::Protocol => "auto follows the clones already in this folder.",
            Field::Watch => "Minutes between pulls of the repositories watched with w.",
            Field::PullRequests => "on saves the commits of every pull request with each fetch.",
            Field::Title => "Shown in the top left. Empty uses the folder name.",
        }
    }

    /// Name used for mouse hit areas.
    pub fn key(self) -> &'static str {
        match self {
            Field::Accounts => "accounts",
            Field::Pinned => "pinned",
            Field::Protocol => "protocol",
            Field::Watch => "watch",
            Field::PullRequests => "pulls",
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
    /// Minutes between automatic pulls, as typed.
    pub interval: String,
    /// Save the commits of all pull requests with each fetch.
    pub pull_requests: bool,
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
        watch_interval: u32,
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
            interval: watch_interval.to_string(),
            // Off until the caller says what the setting is.
            pull_requests: false,
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
            KeyCode::Left | KeyCode::Right if self.focus == Field::PullRequests => {
                self.pull_requests = !self.pull_requests;
            }
            KeyCode::Char(ch) if !ctrl && !alt && self.focus == Field::PullRequests => {
                match ch.to_ascii_lowercase() {
                    ' ' => self.pull_requests = !self.pull_requests,
                    'y' => self.pull_requests = true,
                    'n' => self.pull_requests = false,
                    _ => {}
                }
            }
            KeyCode::Char(ch) if !ctrl && !alt && self.focus == Field::Protocol => {
                match ch.to_ascii_lowercase() {
                    ' ' => self.cycle_protocol(1),
                    'a' => self.protocol = Protocol::Auto,
                    'h' => self.protocol = Protocol::Https,
                    's' => self.protocol = Protocol::Ssh,
                    _ => {}
                }
            }
            _ => self.type_key(key),
        }
        Outcome::Stay
    }

    /// An editing key for the field that has the focus.
    fn type_key(&mut self, key: KeyEvent) {
        let text = match self.focus {
            Field::Accounts => &mut self.accounts,
            Field::Pinned => &mut self.pinned,
            Field::Watch => &mut self.interval,
            Field::Title => &mut self.title,
            Field::Protocol | Field::PullRequests => return,
        };
        if input::edit(text, key, Alt::Ignored) {
            if self.focus == Field::Accounts {
                self.accounts_edited = true;
            }
            self.error = None;
        }
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
        } else if let Some(value) = name.strip_prefix("pulls:") {
            self.focus = Field::PullRequests;
            self.pull_requests = value == "on";
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
        let watch_interval = match config::parse_watch_interval(&self.interval) {
            Ok(minutes) => minutes,
            Err(err) => return self.reject(Field::Watch, err),
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
            update_check: true,
            watched: Vec::new(),
            watch_interval,
            pull_requests: self.pull_requests,
            shell: Default::default(),
            look: Default::default(),
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
            Field::Watch => &mut self.interval,
            Field::Title => &mut self.title,
            Field::Protocol | Field::PullRequests => return,
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
            5,
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
