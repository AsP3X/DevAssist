//! The settings dialog's form: what is typed, which field has focus, and
//! how the text becomes values for the settings file.

use std::path::PathBuf;

use crate::config::{self, Saved};
use crate::repos::github::RepoSpec;
use crate::repos::gitops::Protocol;

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
    pub accounts_edited: bool,
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

    pub fn step(&mut self, delta: isize) {
        let count = Field::ALL.len() as isize;
        let index = Field::ALL
            .iter()
            .position(|field| *field == self.focus)
            .unwrap_or(0) as isize;
        self.focus = Field::ALL[(index + delta).rem_euclid(count) as usize];
    }

    pub fn cycle_protocol(&mut self, delta: isize) {
        let count = PROTOCOLS.len() as isize;
        let index = PROTOCOLS
            .iter()
            .position(|protocol| *protocol == self.protocol)
            .unwrap_or(0) as isize;
        self.protocol = PROTOCOLS[(index + delta).rem_euclid(count) as usize];
    }

    pub fn edit(&mut self, change: impl FnOnce(&mut String)) {
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
}
