//! Looks for a newer release on GitHub and installs it in place, so an
//! update does not need the installer again.
//!
//! The download is the plain executable a release publishes next to the
//! installers. It is checked against the release's checksum file before
//! anything on disk changes. The running program cannot be overwritten on
//! Windows, but it can be renamed: the old file is moved aside, the new
//! one takes its name, and the old one is deleted at the next start.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::sys::http::{self, Profile};

/// The repository whose releases are the updates.
pub const REPOSITORY: &str = "AsP3X/DevAssist";
/// The version of the running program.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The release file that lists the SHA-256 of every other file.
const SUMS_NAME: &str = "SHA256SUMS.txt";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// The version without the tag's leading `v`.
    pub version: String,
    /// What the release page says is new.
    pub notes: String,
    /// The release page, for people who want the installer.
    pub page: String,
    /// The executable for this system, when the release has one.
    pub download: Option<Asset>,
    pub sums: Option<Asset>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    /// The API address of the file. It serves private repositories too.
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    url: String,
    #[serde(default)]
    size: u64,
}

/// Reads what GitHub says about a release.
pub fn parse_release(json: &str) -> Result<Release, String> {
    let api: ApiRelease = serde_json::from_str(json)
        .map_err(|err| format!("unexpected answer from GitHub: {err}"))?;
    let asset = |wanted: &dyn Fn(&str) -> bool| {
        api.assets
            .iter()
            .find(|asset| wanted(&asset.name))
            .map(|asset| Asset {
                name: asset.name.clone(),
                url: asset.url.clone(),
                size: asset.size,
            })
    };
    Ok(Release {
        version: api.tag_name.trim_start_matches(['v', 'V']).to_string(),
        notes: api.body.clone().unwrap_or_default().trim().to_string(),
        page: api.html_url.clone(),
        download: asset(&|name| is_download_for_this_system(name)),
        sums: asset(&|name| name == SUMS_NAME),
    })
}

/// Releases carry the program for 64-bit Windows as `devassist-<version>-x64.exe`.
pub fn is_windows_x64_exe(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.ends_with("-x64.exe") && !name.contains("setup")
}

fn is_download_for_this_system(name: &str) -> bool {
    cfg!(all(windows, target_arch = "x86_64")) && is_windows_x64_exe(name)
}

/// "1.2.3" or "v1.2.3" as numbers. What follows a `-` or `+` is ignored.
pub fn parse_version(text: &str) -> Option<[u64; 3]> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut numbers = [0u64; 3];
    for number in &mut numbers {
        *number = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(numbers)
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

/// Asks GitHub for the latest release. None when the running version is
/// as new, or when nothing has been released yet.
pub fn check(token: Option<&str>) -> Result<Option<Release>, String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let response = get(&url, token, "application/vnd.github+json")?;
    // No release yet, or a repository this sign-in cannot see.
    if response.status().as_u16() == 404 {
        return Ok(None);
    }
    let body = succeeded(response)?
        .body_mut()
        .read_to_string()
        .map_err(|err| err.to_string())?;
    let release = parse_release(&body)?;
    Ok(is_newer(&release.version, VERSION).then_some(release))
}

/// Downloads the release's executable, checks it, and puts it in place of
/// the running one. `on_progress` hears bytes done and bytes in total.
pub fn install(
    release: &Release,
    token: Option<&str>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let asset = release.download.as_ref().ok_or_else(|| {
        format!(
            "Version {} has no download for this system. Get it from {}",
            release.version, release.page
        )
    })?;
    let sums = release.sums.as_ref().ok_or_else(|| {
        format!(
            "Version {} has no checksum file, so the download cannot be verified.",
            release.version
        )
    })?;
    let exe = std::env::current_exe().map_err(|err| format!("where is this program? {err}"))?;
    let fresh = sibling(&exe, "new");

    let listed = download_text(&sums.url, token)?;
    let expected = checksum_for(&listed, &asset.name)
        .ok_or_else(|| format!("{SUMS_NAME} does not list {}.", asset.name))?;

    let mut file = fs::File::create(&fresh).map_err(|err| {
        if err.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "DevAssist cannot write to {}. It is installed for all users, so updating needs \
                 the installer from {}",
                exe.parent().unwrap_or(&exe).display(),
                release.page
            )
        } else {
            format!("could not write {}: {err}", fresh.display())
        }
    })?;
    let fetched = download_into(&asset.url, token, &mut file, asset.size, &mut on_progress);
    drop(file);
    let checked = fetched.and_then(|()| {
        let actual = sha256_file(&fresh)?;
        if actual.eq_ignore_ascii_case(&expected) {
            Ok(())
        } else {
            Err("The download does not match its checksum. Nothing was changed.".to_string())
        }
    });
    if let Err(err) = checked.and_then(|()| swap_in(&fresh, &exe)) {
        let _ = fs::remove_file(&fresh);
        return Err(err);
    }
    Ok(())
}

/// Removes what an earlier update left beside the program.
pub fn clean_up() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = fs::remove_file(sibling(&exe, "old"));
        let _ = fs::remove_file(sibling(&exe, "new"));
    }
}

/// `devassist.exe` gives `devassist.exe.old` for the suffix "old".
fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    exe.with_file_name(name)
}

/// Puts `fresh` where `exe` is. `exe` is moved aside first, and moved back
/// if the new file cannot take its place.
pub fn swap_in(fresh: &Path, exe: &Path) -> Result<(), String> {
    let old = sibling(exe, "old");
    let _ = fs::remove_file(&old);
    fs::rename(exe, &old)
        .map_err(|err| format!("could not move the running program aside: {err}"))?;
    if let Err(err) = fs::rename(fresh, exe) {
        let _ = fs::rename(&old, exe);
        return Err(format!("could not put the new version in place: {err}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(exe, fs::Permissions::from_mode(0o755));
    }
    Ok(())
}

/// The SHA-256 a checksum file lists for `name`. Lines read
/// `<hex>  <name>`, as `sha256sum` writes them.
pub fn checksum_for(listed: &str, name: &str) -> Option<String> {
    listed.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|ch| ch.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        fs::File::open(path).map_err(|err| format!("could not read {}: {err}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let size = file.read(&mut chunk).map_err(|err| err.to_string())?;
        if size == 0 {
            break;
        }
        hasher.update(&chunk[..size]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

type Answer = http::Response;

/// Asks GitHub for `url`. An answer with any status comes back; an error
/// means GitHub could not be reached. The sign-in is not passed on when
/// GitHub redirects a download to its file storage.
fn get(url: &str, token: Option<&str>, accept: &str) -> Result<Answer, String> {
    http::get(
        url,
        Profile::Steady,
        &[("Accept", accept), ("X-GitHub-Api-Version", "2022-11-28")],
        token,
    )
    .map_err(|err| format!("could not reach GitHub: {err}"))
}

/// The answer, if GitHub said yes.
fn succeeded(response: Answer) -> Result<Answer, String> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(format!("GitHub answered {}", response.status().as_u16()))
    }
}

fn download_text(url: &str, token: Option<&str>) -> Result<String, String> {
    succeeded(get(url, token, "application/octet-stream")?)?
        .body_mut()
        .read_to_string()
        .map_err(|err| err.to_string())
}

fn download_into(
    url: &str,
    token: Option<&str>,
    file: &mut fs::File,
    size: u64,
    on_progress: &mut impl FnMut(u64, u64),
) -> Result<(), String> {
    let response = succeeded(get(url, token, "application/octet-stream")?)?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|length| length.to_str().ok())
        .and_then(|length| length.parse().ok())
        .unwrap_or(size);
    let mut reader = response.into_body().into_reader();
    let mut chunk = [0u8; 64 * 1024];
    let mut done = 0u64;
    on_progress(done, total);
    loop {
        let read = reader
            .read(&mut chunk)
            .map_err(|err| format!("the download stopped: {err}"))?;
        if read == 0 {
            break;
        }
        file.write_all(&chunk[..read])
            .map_err(|err| format!("could not write the download: {err}"))?;
        done += read as u64;
        on_progress(done, total.max(done));
    }
    file.flush().map_err(|err| err.to_string())?;
    if size > 0 && done != size {
        return Err(format!(
            "the download is incomplete: {done} of {size} bytes."
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real thing, against the latest release on GitHub. It needs the
    /// network, so it only runs when asked for:
    /// `cargo test -- --ignored live_download`
    #[test]
    #[ignore = "downloads the latest release from GitHub"]
    fn live_download_matches_its_checksum() {
        if !cfg!(all(windows, target_arch = "x86_64")) {
            return;
        }
        let token = crate::repos::github::token();
        let token = token.as_deref();
        let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
        let body = succeeded(get(&url, token, "application/vnd.github+json").unwrap())
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();
        let release = parse_release(&body).unwrap();
        let asset = release.download.expect("the release has the program");
        let sums = release.sums.expect("the release has checksums");
        let listed = download_text(&sums.url, token).unwrap();
        let expected = checksum_for(&listed, &asset.name).expect("the program is listed");

        let path = std::env::temp_dir().join(format!("devassist-live-{}", std::process::id()));
        let mut file = fs::File::create(&path).unwrap();
        let mut seen = (0, 0);
        download_into(
            &asset.url,
            token,
            &mut file,
            asset.size,
            &mut |done, total| {
                seen = (done, total);
            },
        )
        .unwrap();
        drop(file);
        assert_eq!(seen, (asset.size, asset.size));
        assert_eq!(sha256_file(&path).unwrap(), expected);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn versions_compare_by_number() {
        assert_eq!(parse_version("v1.2.3"), Some([1, 2, 3]));
        assert_eq!(parse_version("0.10.0-beta.1"), Some([0, 10, 0]));
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(parse_version("latest"), None);
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("v1.0.0", "0.99.99"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
        // A tag that is not a version never counts as newer.
        assert!(!is_newer("nightly", "0.3.0"));
        assert!(parse_version(VERSION).is_some());
    }

    #[test]
    fn a_release_names_its_download_and_checksums() {
        let json = r#"{
            "tag_name": "v9.1.0",
            "html_url": "https://github.com/AsP3X/DevAssist/releases/tag/v9.1.0",
            "body": "\n- New things\n",
            "assets": [
                {"name": "devassist-9.1.0-setup.exe", "url": "https://api.github.com/a/1", "size": 10},
                {"name": "devassist-9.1.0-x64.msi", "url": "https://api.github.com/a/2", "size": 20},
                {"name": "devassist-9.1.0-x64.exe", "url": "https://api.github.com/a/3", "size": 30},
                {"name": "SHA256SUMS.txt", "url": "https://api.github.com/a/4", "size": 40}
            ]
        }"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.version, "9.1.0");
        assert_eq!(release.notes, "- New things");
        assert!(release.page.ends_with("/v9.1.0"));
        assert_eq!(
            release.sums.as_ref().unwrap().url,
            "https://api.github.com/a/4"
        );
        if cfg!(all(windows, target_arch = "x86_64")) {
            let download = release.download.unwrap();
            assert_eq!(download.name, "devassist-9.1.0-x64.exe");
            assert_eq!(download.size, 30);
        } else {
            assert!(release.download.is_none());
        }
        assert!(is_windows_x64_exe("devassist-9.1.0-x64.exe"));
        assert!(!is_windows_x64_exe("devassist-9.1.0-setup.exe"));
        assert!(!is_windows_x64_exe("devassist-9.1.0-x64.msi"));

        // A release without files or notes still reads.
        let bare = parse_release(r#"{"tag_name": "v9.2.0"}"#).unwrap();
        assert!(bare.download.is_none() && bare.sums.is_none());
        assert!(parse_release("not json").is_err());
    }

    #[test]
    fn checksums_are_found_by_file_name() {
        let hash = "a".repeat(64);
        let listed = format!(
            "{}  devassist-9.1.0-setup.exe\n{}  devassist-9.1.0-x64.exe\r\n{} *starred.bin\nshort  bad.bin\n",
            "b".repeat(64),
            hash.to_uppercase(),
            "c".repeat(64),
        );
        assert_eq!(checksum_for(&listed, "devassist-9.1.0-x64.exe"), Some(hash));
        assert_eq!(checksum_for(&listed, "starred.bin"), Some("c".repeat(64)));
        assert_eq!(checksum_for(&listed, "bad.bin"), None);
        assert_eq!(checksum_for(&listed, "missing.exe"), None);
    }

    #[test]
    fn a_new_file_takes_the_place_of_the_program() {
        let root =
            std::env::temp_dir().join(format!("devassist-selfupdate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let exe = root.join("devassist.exe");
        let fresh = sibling(&exe, "new");
        assert_eq!(fresh, root.join("devassist.exe.new"));
        fs::write(&exe, "old program").unwrap();
        fs::write(&fresh, "new program").unwrap();
        // What an earlier update left is replaced.
        fs::write(sibling(&exe, "old"), "older still").unwrap();

        swap_in(&fresh, &exe).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new program");
        assert_eq!(
            fs::read_to_string(sibling(&exe, "old")).unwrap(),
            "old program"
        );
        assert!(!fresh.exists());

        // Without the new file, the program is put back as it was.
        let err = swap_in(&fresh, &exe).unwrap_err();
        assert!(err.contains("could not put the new version in place"));
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new program");

        // The well-known digest of "abc".
        fs::write(&fresh, "abc").unwrap();
        assert_eq!(
            sha256_file(&fresh).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_dir_all(&root);
    }
}
