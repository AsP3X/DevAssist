//! The key handling of the folder dialog. The prompt's state, and what Tab
//! and Ctrl-W do to it, are in `devassist_core::prompt`.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::input::{self, Alt};
use devassist_core::prompt::delete_segment;
pub use devassist_core::prompt::{PathPrompt, resolve};

pub enum Outcome {
    Stay,
    Accept(std::path::PathBuf),
    Close,
}

pub fn on_key<P>(prompt: &mut PathPrompt<P>, key: KeyEvent) -> Outcome {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Esc => return Outcome::Close,
        KeyCode::Enter => match prompt.folder() {
            Ok(folder) => return Outcome::Accept(folder),
            Err(err) => {
                prompt.matches.clear();
                prompt.error = Some(err);
            }
        },
        KeyCode::Tab => prompt.complete(),
        KeyCode::Char('w') | KeyCode::Char('W') if ctrl => prompt.edit(delete_segment),
        _ => {
            if input::edit(&mut prompt.input, key, Alt::Types) {
                prompt.error = None;
                prompt.matches.clear();
            }
        }
    }
    Outcome::Stay
}

#[cfg(test)]
mod tests {
    use super::*;
    use devassist_core::sys::paths;
    use std::fs;
    use std::path::{MAIN_SEPARATOR, PathBuf};

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
    fn the_prompt_edits_checks_and_accepts() {
        let root = temp("keys");
        fs::create_dir_all(root.join("work")).unwrap();
        let mut prompt = PathPrompt::new((), &root, true);
        assert_eq!(prompt.input, root.display().to_string());

        // This prompt needs a folder that exists.
        prompt.paste(&format!("{MAIN_SEPARATOR}missing"));
        assert!(matches!(
            on_key(&mut prompt, key(KeyCode::Enter)),
            Outcome::Stay
        ));
        assert!(prompt.error.as_deref().unwrap().contains("is not a folder"));

        // Typing clears the error; ctrl-w drops the last part.
        on_key(
            &mut prompt,
            KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL),
        );
        assert!(prompt.error.is_none());
        assert_eq!(prompt.input, format!("{}{MAIN_SEPARATOR}", root.display()));
        on_key(&mut prompt, key(KeyCode::Char('w')));
        on_key(&mut prompt, key(KeyCode::Tab));
        assert_eq!(
            prompt.input,
            format!("{}{MAIN_SEPARATOR}work{MAIN_SEPARATOR}", root.display())
        );
        match on_key(&mut prompt, key(KeyCode::Enter)) {
            Outcome::Accept(folder) => assert!(paths::same_path(&folder, &root.join("work"))),
            _ => panic!("the folder should be accepted"),
        }
        assert!(matches!(
            on_key(&mut prompt, key(KeyCode::Esc)),
            Outcome::Close
        ));

        on_key(
            &mut prompt,
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        );
        assert!(prompt.input.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_prompt_can_take_a_folder_that_is_not_there_yet() {
        let root = temp("fresh");
        let mut prompt = PathPrompt::new("clone", &root.join("new"), false);
        assert_eq!(prompt.purpose, "clone");
        // AltGr characters are typed: a path needs `\` on some keyboards.
        on_key(
            &mut prompt,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT),
        );
        assert!(prompt.input.ends_with("newx"));
        match on_key(&mut prompt, key(KeyCode::Enter)) {
            Outcome::Accept(folder) => assert!(paths::same_path(&folder, &root.join("newx"))),
            _ => panic!("the folder should be accepted"),
        }
        let _ = fs::remove_dir_all(&root);
    }
}
