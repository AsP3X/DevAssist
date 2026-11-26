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

/// `path` with its `.` and `..` worked out by the text alone, the way
/// Windows reads them: `C:\code\x\..\api\.` is `C:\code\api`. A `..` at
/// the root stays at the root; one that a relative path starts with
/// stays. On Windows `\\?\C:\code` is written `C:\code` and
/// `\\?\UNC\server\share` is `\\server\share`. Reads nothing: a link or
/// a junction is not followed.
pub fn tidy(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    // How many plain names `out` ends with, which a `..` can take back.
    let mut names = 0usize;
    for part in path.components() {
        match part {
            Component::Prefix(prefix) => out.push(plain_prefix(prefix)),
            Component::RootDir => out.push(Component::RootDir.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir if names > 0 => {
                out.pop();
                names -= 1;
            }
            Component::ParentDir => {
                if !out.has_root() {
                    out.push("..");
                }
            }
            Component::Normal(name) => {
                out.push(name);
                names += 1;
            }
        }
    }
    out
}

/// A path's prefix without the `\\?\` that only tells Windows to read the
/// rest as it is.
#[cfg(windows)]
fn plain_prefix(prefix: std::path::PrefixComponent<'_>) -> std::ffi::OsString {
    use std::path::Prefix;
    match prefix.kind() {
        Prefix::VerbatimDisk(letter) => format!("{}:", char::from(letter)).into(),
        Prefix::VerbatimUNC(server, share) => {
            let mut text = std::ffi::OsString::from(r"\\");
            text.push(server);
            text.push(r"\");
            text.push(share);
            text
        }
        _ => prefix.as_os_str().to_os_string(),
    }
}

/// Other systems have no prefix in a path.
#[cfg(not(windows))]
fn plain_prefix(prefix: std::path::PrefixComponent<'_>) -> std::ffi::OsString {
    prefix.as_os_str().to_os_string()
}

/// Whether `path` is `folder` or lies inside it, comparing as `same_path`
/// does: case does not count on Windows. Both are read with their `.` and
/// `..` worked out first (`tidy`), by the text alone.
pub fn is_within(path: &Path, folder: &Path) -> bool {
    let folder = tidy(folder);
    tidy(path)
        .ancestors()
        .any(|above| same_path(above, &folder))
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
    fn a_path_is_within_a_folder_as_same_path_compares_them() {
        let api = Path::new("/srv/code/api");
        assert!(is_within(Path::new("/srv/code/api/sub"), api));
        assert!(
            is_within(Path::new("/srv/code/api/"), api),
            "a folder is within itself"
        );
        assert!(!is_within(Path::new("/srv/code/api-copy"), api));
        assert!(!is_within(Path::new("/srv/code"), api));
        // Case counts only where `same_path` counts it.
        assert_eq!(
            is_within(Path::new("/srv/Code/API/sub"), api),
            cfg!(windows)
        );
    }

    #[test]
    fn dots_are_worked_out_by_the_text_alone() {
        let same = |a: &str, b: &str| assert!(same_path(&tidy(Path::new(a)), Path::new(b)), "{a}");
        same("/srv/code/api/.", "/srv/code/api");
        same("/srv/code/x/../api", "/srv/code/api");
        same("/srv/code/api/../other.git", "/srv/code/other.git");
        same("/srv/./code//api/", "/srv/code/api");
        same("/../srv", "/srv");
        same("a/../../b", "../b");
        // Inside the checkout or not, as the dots say.
        let api = Path::new("/srv/code/api");
        assert!(is_within(Path::new("/srv/code/api/."), api));
        assert!(is_within(Path::new("/srv/code/x/../api"), api));
        assert!(is_within(Path::new("/srv/code/x/../api/sub/.."), api));
        assert!(!is_within(Path::new("/srv/code/api/../other.git"), api));
        assert!(is_within(api, Path::new("/srv/code/api/sub/..")));
        if cfg!(windows) {
            same(r"\\?\C:\code\api", r"C:\code\api");
            same(r"\\?\UNC\server\share\api", r"\\server\share\api");
            same(r"C:\code\api\..\..\..", r"C:\");
            assert!(is_within(
                Path::new(r"\\?\C:\code\api"),
                Path::new(r"c:\code\api")
            ));
        }
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
