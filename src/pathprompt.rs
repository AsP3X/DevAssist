//! The dialog that asks for a folder: where new clones go, or where to
//! look for clones that already exist.

use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::atoms::sys::paths;
use crate::atoms::ui::input::{self, Alt};
use crate::github::RemoteRepo;

/// How many completion candidates the dialog keeps to show.
const MATCH_LIMIT: usize = 40;

#[derive(Clone)]
pub enum Purpose {
    /// Clone these repositories into the folder.
    Clone(Vec<RemoteRepo>),
    /// Add the clones that already sit in the folder to the overview.
    Import,
}

pub enum Outcome {
    Stay,
    Accept(PathBuf),
    Close,
}

pub struct PathPrompt {
    pub purpose: Purpose,
    pub input: String,
    pub error: Option<String>,
    /// Folders that Tab found for what is typed.
    pub matches: Vec<String>,
}

impl PathPrompt {
    pub fn new(purpose: Purpose, start: &Path) -> Self {
        Self {
            purpose,
            input: start.display().to_string(),
            error: None,
            matches: Vec::new(),
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => match self.folder() {
                Ok(folder) => return Outcome::Accept(folder),
                Err(err) => {
                    self.matches.clear();
                    self.error = Some(err);
                }
            },
            KeyCode::Tab => self.complete(),
            KeyCode::Char('w') | KeyCode::Char('W') if ctrl => self.edit(delete_segment),
            _ => {
                if input::edit(&mut self.input, key, Alt::Types) {
                    self.error = None;
                    self.matches.clear();
                }
            }
        }
        Outcome::Stay
    }

    /// A pasted path. Quotes around it, as Explorer's "Copy as path" adds
    /// them, are dropped.
    pub fn paste(&mut self, text: &str) {
        let text = text.trim().trim_matches('"').to_string();
        self.edit(|input| input.push_str(&text));
    }

    fn edit(&mut self, change: impl FnOnce(&mut String)) {
        change(&mut self.input);
        self.error = None;
        self.matches.clear();
    }

    /// The folder that is typed, checked for what it will be used for.
    pub fn folder(&self) -> Result<PathBuf, String> {
        let folder = resolve(&self.input)?;
        match &self.purpose {
            Purpose::Import if !folder.is_dir() => {
                Err(format!("{} is not a folder.", folder.display()))
            }
            _ => Ok(folder),
        }
    }

    /// Where a single repository ends up, to show under the field.
    pub fn target(&self) -> Option<PathBuf> {
        match &self.purpose {
            Purpose::Clone(remotes) if remotes.len() == 1 => {
                Some(resolve(&self.input).ok()?.join(&remotes[0].name))
            }
            _ => None,
        }
    }

    fn complete(&mut self) {
        self.error = None;
        match complete(&self.input) {
            Some((input, matches)) => {
                self.input = input;
                self.matches = matches;
            }
            None => self.matches.clear(),
        }
    }
}

/// Turns what was typed into a full path. `~` is the home folder.
pub fn resolve(input: &str) -> Result<PathBuf, String> {
    let text = input.trim().trim_matches('"').trim();
    if text.is_empty() {
        return Err("Type the folder.".to_string());
    }
    let path = expand_home(text);
    if !path.is_absolute() {
        return Err(format!(
            "Use a full path, such as {} or ~{MAIN_SEPARATOR}dev.",
            if cfg!(windows) { "C:\\dev" } else { "/srv/dev" }
        ));
    }
    if path.is_file() {
        return Err(format!("{} is a file, not a folder.", path.display()));
    }
    Ok(path)
}

fn expand_home(text: &str) -> PathBuf {
    let rest = match text.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => rest,
        _ => return PathBuf::from(text),
    };
    match paths::home_dir() {
        Some(home) => home.join(rest.trim_start_matches(['/', '\\'])),
        None => PathBuf::from(text),
    }
}

/// Ctrl-W: drops the last part of the path, back to the separator before it.
fn delete_segment(input: &mut String) {
    let trimmed = input.trim_end_matches(['/', '\\']);
    let keep = trimmed.rfind(['/', '\\']).map_or(0, |index| index + 1);
    input.truncate(keep);
}

/// Tab: completes the last part of the path to a folder that exists.
/// Returns the new input and the folders that still fit. None when the
/// input has no folder to look in yet.
fn complete(input: &str) -> Option<(String, Vec<String>)> {
    let split = input.rfind(['/', '\\'])? + 1;
    let (head, typed) = input.split_at(split);
    let wanted = typed.to_lowercase();
    let mut names: Vec<String> = std::fs::read_dir(expand_home(head))
        .ok()?
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.to_lowercase().starts_with(&wanted))
        // Hidden folders only when asked for.
        .filter(|name| wanted.starts_with('.') || !name.starts_with('.'))
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    match names.as_slice() {
        [] => Some((input.to_string(), Vec::new())),
        [only] => Some((format!("{head}{only}{MAIN_SEPARATOR}"), Vec::new())),
        many => {
            let shared = common_prefix(many);
            let typed = if shared.chars().count() > typed.chars().count() {
                shared
            } else {
                typed.to_string()
            };
            names.truncate(MATCH_LIMIT);
            Some((format!("{head}{typed}"), names))
        }
    }
}

/// What all names start with, ignoring case, in the first name's spelling.
fn common_prefix(names: &[String]) -> String {
    let Some(first) = names.first() else {
        return String::new();
    };
    let mut shared: Vec<char> = first.chars().collect();
    for name in &names[1..] {
        let same = shared
            .iter()
            .zip(name.chars())
            .take_while(|(left, right)| left.to_lowercase().eq(right.to_lowercase()))
            .count();
        shared.truncate(same);
    }
    shared.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn temp(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("devassist-prompt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn a_folder_must_be_a_full_path() {
        assert!(resolve("").is_err());
        assert!(resolve("dev").is_err());
        assert!(resolve("./dev").is_err());
        let root = temp("resolve");
        assert_eq!(resolve(&root.display().to_string()).unwrap(), root);
        // A folder that does not exist yet is fine: cloning creates it.
        assert_eq!(
            resolve(&format!("\"{}\"", root.join("new").display())).unwrap(),
            root.join("new")
        );
        fs::write(root.join("file.txt"), "x").unwrap();
        assert!(resolve(&root.join("file.txt").display().to_string()).is_err());
        if let Some(home) = paths::home_dir() {
            assert_eq!(resolve("~").unwrap(), home);
            assert_eq!(resolve("~/dev").unwrap(), home.join("dev"));
        }
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn tab_completes_folder_names() {
        let root = temp("tab");
        for name in ["apps", "api-old", "api-new", ".hidden", "zeta"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        fs::write(root.join("apple.txt"), "x").unwrap();
        let sep = MAIN_SEPARATOR;
        let base = format!("{}{sep}", root.display());

        // One match is completed and ends with a separator.
        let (input, matches) = complete(&format!("{base}Z")).unwrap();
        assert_eq!(input, format!("{base}zeta{sep}"));
        assert!(matches.is_empty());

        // Several: as far as they agree, and the candidates are listed.
        let (input, matches) = complete(&format!("{base}a")).unwrap();
        assert_eq!(input, format!("{base}ap"));
        assert_eq!(matches, ["api-new", "api-old", "apps"]);
        let (input, matches) = complete(&format!("{base}api")).unwrap();
        assert_eq!(input, format!("{base}api-"));
        assert_eq!(matches, ["api-new", "api-old"]);

        // Files and hidden folders stay out; nothing fits is no change.
        let (_, matches) = complete(&base).unwrap();
        assert_eq!(matches, ["api-new", "api-old", "apps", "zeta"]);
        let (input, matches) = complete(&format!("{base}nope")).unwrap();
        assert_eq!(input, format!("{base}nope"));
        assert!(matches.is_empty());
        assert!(complete("no-separator").is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_prompt_edits_checks_and_accepts() {
        let root = temp("keys");
        fs::create_dir_all(root.join("work")).unwrap();
        let mut prompt = PathPrompt::new(Purpose::Import, &root);
        assert_eq!(prompt.input, root.display().to_string());

        // Import needs a folder that exists.
        prompt.paste(&format!("{MAIN_SEPARATOR}missing"));
        assert!(matches!(prompt.on_key(key(KeyCode::Enter)), Outcome::Stay));
        assert!(prompt.error.as_deref().unwrap().contains("is not a folder"));

        // Typing clears the error; ctrl-w drops the last part.
        prompt.on_key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert!(prompt.error.is_none());
        assert_eq!(prompt.input, format!("{}{MAIN_SEPARATOR}", root.display()));
        prompt.on_key(key(KeyCode::Char('w')));
        prompt.on_key(key(KeyCode::Tab));
        assert_eq!(
            prompt.input,
            format!("{}{MAIN_SEPARATOR}work{MAIN_SEPARATOR}", root.display())
        );
        match prompt.on_key(key(KeyCode::Enter)) {
            Outcome::Accept(folder) => assert!(paths::same_path(&folder, &root.join("work"))),
            _ => panic!("the folder should be accepted"),
        }
        assert!(matches!(prompt.on_key(key(KeyCode::Esc)), Outcome::Close));

        prompt.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(prompt.input.is_empty());
        assert!(prompt.target().is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
