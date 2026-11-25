//! Helpers for folders and for comparing paths.

use std::path::{Path, PathBuf};

/// An absolute folder named by an environment variable.
pub fn env_dir(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

pub fn home_dir() -> Option<PathBuf> {
    env_dir(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
}

/// Windows does not tell `C:\Dev` from `c:\dev\`.
pub fn same_path(left: &Path, right: &Path) -> bool {
    let plain = |path: &Path| {
        let text = path.to_string_lossy().replace('\\', "/");
        let text = text.trim_end_matches('/').to_string();
        if cfg!(windows) {
            text.to_lowercase()
        } else {
            text
        }
    };
    plain(left) == plain(right)
}

/// Adds `path` unless it is listed already. True when it was added.
pub fn push_path(list: &mut Vec<PathBuf>, path: PathBuf) -> bool {
    if list.iter().any(|item| same_path(item, &path)) {
        return false;
    }
    list.push(path);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_path_ignores_slash_style_and_a_trailing_slash() {
        assert!(same_path(Path::new("a\\b"), Path::new("a/b/")));
        assert!(!same_path(Path::new("a/b"), Path::new("a/c")));
    }

    #[test]
    fn same_path_ignores_case_only_on_windows() {
        assert_eq!(
            same_path(Path::new("/A/b"), Path::new("/a/b")),
            cfg!(windows)
        );
    }

    #[test]
    fn push_path_adds_a_path_once() {
        let mut list = vec![PathBuf::from("/srv/code/api")];
        assert!(!push_path(&mut list, PathBuf::from("/srv/code/api/")));
        assert!(push_path(&mut list, PathBuf::from("/srv/code/web")));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn env_dir_rejects_a_missing_variable() {
        assert!(env_dir("DEVASSIST_NO_SUCH_VARIABLE").is_none());
    }
}
