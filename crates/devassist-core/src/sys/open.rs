//! Hand a web page or a folder to the desktop.

use std::path::Path;
use std::process::{Command, Stdio};

/// Opens a web page in the default browser.
pub fn url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("{url} is not a web address"));
    }
    let mut cmd = command(Path::new(url));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|err| format!("could not open {url}: {err}"))?;
    Ok(())
}

/// Opens a folder in the file manager.
pub fn folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let mut cmd = command(path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|err| format!("could not open {}: {err}", path.display()))?;
    Ok(())
}

fn command(target: &Path) -> Command {
    #[cfg(feature = "testing")]
    super::note(super::Reach::Program);
    #[cfg(windows)]
    {
        let mut cmd = Command::new("explorer");
        cmd.arg(target);
        cmd
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(target);
        cmd
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(target);
        cmd
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_refuses_anything_but_https() {
        let err = url("http://example.com").unwrap_err();
        assert_eq!(err, "http://example.com is not a web address");
        assert!(url("file:///etc/passwd").is_err());
    }

    #[test]
    fn folder_refuses_what_is_not_a_folder() {
        let missing = std::env::temp_dir().join("devassist-no-such-folder");
        let err = folder(&missing).unwrap_err();
        assert_eq!(err, format!("{} is not a folder", missing.display()));
    }
}
