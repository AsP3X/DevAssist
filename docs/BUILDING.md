# Building

## What you need

- [Rust](https://rustup.rs/) 1.88 or newer (stable)
- [Git](https://git-scm.com/) on your `PATH`. The app runs `git`, and so do
  the tests.

Everything else comes from crates.io. One crate (`ring`, for TLS) compiles C
code, which uses the C compiler the Rust toolchain already requires: the MSVC
build tools on Windows.

## Build and run

```
cargo run
cargo build --release
```

The release executable is `target\release\devassist.exe`. It is a single file
and can be copied anywhere.

To try the app without touching your real settings, give it a settings file of
its own:

```
cargo run -- --config scratch.toml
```

## Checks

These are what CI runs. All three must pass before a pull request is merged.

```
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

About the tests:

- They are unit tests inside the source files (`#[cfg(test)] mod tests`).
- Several build throwaway repositories with real `git` under the system's
  temporary folder and remove them again.
- Screen tests draw the interface into an in-memory terminal and check the
  text, so no real terminal is needed.
- No test talks to GitHub, opens a browser or touches the clipboard. The one
  exception is marked `#[ignore]` and only runs when asked for: it downloads
  the latest release and checks it against its checksum.

  ```
  cargo test -- --ignored live_download
  ```

## Installers

`installer\build.ps1` builds the release executable and both installers into
`target\installer`:

| File | Made with |
|---|---|
| `devassist-<version>-x64.msi` | [WiX Toolset](https://wixtoolset.org/) 4 or 5, from `installer\devassist.wxs` |
| `devassist-<version>-setup.exe` | [Inno Setup](https://jrsoftware.org/isinfo.php) 6.3 or newer, from `installer\devassist.iss` |
| `devassist-<version>-x64.exe` | A copy of the release executable; this is what self-update downloads |
| `SHA256SUMS.txt` | The SHA-256 of the files above |

```
dotnet tool install --global wix --version 5.0.2
winget install JRSoftware.InnoSetup
powershell -ExecutionPolicy Bypass -File installer\build.ps1
```

The version comes from `Cargo.toml`. An installer whose tool is missing is
skipped, and the script says how to get the tool.

WiX 5.0.2 is named on purpose: from version 6 on, the WiX project asks
organisations for a maintenance fee.

Two identifiers in the installer files must never change, because they are how
Windows recognises a newer version of the same program: the `UpgradeCode` in
`devassist.wxs` and the `AppId` in `devassist.iss`.

## Project layout

| File | What is in it |
|---|---|
| `src/main.rs` | Start-up, the terminal loop, `--doctor` |
| `src/app.rs` | The app's state and everything the keys do |
| `src/ui.rs` | Drawing: the overview and every dialog |
| `src/gitops.rs` | Everything that runs `git`: status, fetch, update, clone, remove |
| `src/github.rs` | The GitHub API: listing repositories, the sign-in token |
| `src/config.rs` | The settings file and the command line |
| `src/job.rs` | The model of one update or clone run, for the progress dialog |
| `src/update.rs` | Looking for a newer release and installing it |
| `src/settings.rs` | The settings dialog's form |
| `src/pathprompt.rs` | The folder dialog, with Tab completion |
| `src/select.rs`, `src/clipboard.rs` | Selecting text with the mouse and copying it |
| `installer/` | The installer definitions and their build script |
| `.github/workflows/` | CI and the release build |

## Other systems

The code has branches for Linux and macOS (opening folders, file permissions,
the settings location), and nothing in it is Windows-only by design. But
releases, CI and day-to-day use are on Windows, so treat other systems as
untested. Reports and fixes are welcome.
