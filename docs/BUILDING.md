# Building

## What you need

- [Rust](https://rustup.rs/) 1.99 or newer (stable)
- [Git](https://git-scm.com/) on your `PATH`. The app runs `git`, and so do
  the tests.

Everything else comes from crates.io, apart from the window's forks (below). One crate (`ring`, for TLS) compiles C
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

## The window

`devassist-gui` is a second program, a window in a "liquid glass" design. It
has no functions yet: it opens and shows its frame. It is not in the
installers or the release downloads.

```
cargo run -p devassist-gui
```

The first build fetches the window's dependencies. They include forks from
GitHub, one of the egui crates and one of winit, pinned by commit in the
`[patch.crates-io]` table of the root `Cargo.toml`. The glass needs a GPU
adapter. Without one, the window draws frosted panes instead.

| Option | What it does |
|---|---|
| `--config PATH` | Reads the settings from this file |
| `--screenshot OUT.png` | Saves a picture of the window and exits |
| `--light`, `--dark` | Light or dark, whatever the settings say |
| `--backdrop KEY` | The backdrop: `lake`, `midday`, `golden`, `dusk`, `night` or `forest` |
| `--no-blur` | Does not blur the backdrop |
| `--no-sidebar` | Opens with the sidebar hidden |

At start the window reads `theme`, `backdrop` and `blur` from the settings
file. It cannot change them yet. A settings file with a mistake makes the
window print one line on standard error and open with the default look. A
wrong option prints the usage line.

To make a picture of the window, give it a file name. This opens the window
off screen for a moment and then ends:

```
cargo run -p devassist-gui -- --screenshot window.png --dark --backdrop night
```

## Checks

These are what CI runs. All three must pass before a pull request is merged.

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
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
  cargo test --workspace -- --ignored live_download
  ```

The glass has tests of its own:

- The shader's tests need a GPU adapter. Without one they skip themselves.
  Set `DEVASSIST_REQUIRE_GPU=1` to make a missing adapter a failure instead.
- The glass compares its pictures with the ones kept in
  `crates/devassist-glass/golden/`. After a change that should alter them,
  run the tests once with `DEVASSIST_BLESS=1` to keep the new pictures, and
  look at what changed before you commit.
- One test in `devassist-gui` measures how long a bake of the glass takes. It
  is marked `#[ignore]`, so it only runs when asked for:

  ```
  cargo test -p devassist-gui --release -- --ignored --nocapture measure_a_glass_bake
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

The repository is a Cargo workspace of four packages. The root package is the
terminal app: keys and drawing. `crates/devassist-core` holds the logic that
does not depend on the terminal, so that another interface can use it too. It
must not depend on `ratatui` or `crossterm`; CI checks this. The other two
packages make the window: `crates/devassist-glass` is the glass material and
`crates/devassist-gui` is the window itself.

### The terminal app (`src/`)

| File | What is in it |
|---|---|
| `src/main.rs` | Start-up: the command line, then the shell |
| `src/shell/mod.rs` | The main loop, the `Feature` trait every tool implements, what is on screen (home or a tool), and which keys and clicks are the shell's |
| `src/shell/home.rs` | The welcome screen: the logo and a card for every tool |
| `src/shell/tools.rs` | The palette: Home and every tool in a searchable list |
| `src/shell/keys.rs` | Matching a key press against a key name such as `ctrl-p` from the settings |
| `src/shell/chrome.rs` | The top line: where you are and a note |
| `src/shell/select.rs` | Selecting text with the mouse and copying it |
| `src/shell/help.rs` | The help dialog |
| `src/shell/updater.rs` | The version dialog and its state |
| `src/shell/terminal.rs` | Putting the terminal into full-screen mode and back |
| `src/features/repos/app.rs` | The repository overview: the keys, around the state in the core |
| `src/features/repos/ui.rs` | Drawing: the overview and every dialog |
| `src/features/repos/settings.rs` | The settings dialog's keys |
| `src/features/repos/mod.rs` | The tool's link to the shell, its help text, and `--doctor` |
| `src/features/docker/state.rs` | The Docker tool's keys, around the state in the core |
| `src/features/docker/view.rs` | Drawing: the four lists and the dialogs |
| `src/features/docker/mod.rs` | The tool's link to the shell, its help text, and its `--doctor` line |
| `src/atoms/ui/` | Building blocks for the screen: text fields, lists, menus, tabs, dialogs, the log view, the folder prompt, text selection |

The terminal app reaches the core through re-exports, so `crate::atoms::sys`,
`crate::features::repos::gitops` and `crate::shell::config` still work.

### The core (`crates/devassist-core/src/`)

| File | What is in it |
|---|---|
| `config.rs` | The settings file and the command line |
| `update.rs` | Looking for a newer release and installing it |
| `keys.rs` | Reading a key name such as `ctrl-p` from the settings |
| `edit.rs` | Editing one line of text |
| `list.rs` | Moving through a list and keeping the chosen row on screen |
| `prompt.rs` | The folder prompt's text and its Tab completion |
| `sys/` | The system: running programs and streaming their output, HTTP, clipboard, paths |
| `task/` | Background work: the job model, worker threads |
| `repos/state.rs` | The repository overview's state and everything it does |
| `repos/form.rs` | The settings dialog's form and what it turns into |
| `repos/gitops.rs` | Everything that runs `git`: status, fetch, update, clone, remove |
| `repos/github.rs` | The GitHub API: listing repositories, the sign-in token |
| `repos/gitprogress.rs` | Reading git's progress output |
| `docker/state.rs` | The Docker tool's state and everything it does |
| `docker/model.rs` | Containers, projects, images and volumes, read from Docker's JSON |
| `docker/cli.rs` | Every `docker` command the tool runs |
| `docker/actions.rs` | What each action runs, and what a confirmation lists |
| `docker/logbuf.rs` | A log's lines and the reader's place in them |
| `tests/` | Tests that drive each tool's state without a terminal |

The core has a Cargo feature, `testing`, for helpers that other crates' tests
use. The root package turns it on as a dev-dependency.

### The glass (`crates/devassist-glass/`)

| File | What is in it |
|---|---|
| `SOURCE.md` | Which files are copied from Quill, another project of the same author, and what was changed in them |
| `src/material.rs`, `raster.rs`, `shader.rs`, `gpu.rs`, `glass.frag` | The glass material, its shader and the GPU device, copied from Quill |
| `src/color.rs`, `sync.rs` | The two small pieces of Quill's code the others need |
| `src/pane.rs` | Draws one rounded pane over a backdrop, as glass or frosted |
| `golden/` | The kept pictures the tests compare with |

### The window (`crates/devassist-gui/`)

| File | What is in it |
|---|---|
| `src/main.rs` | Start-up: the command line, the settings, then the window |
| `src/launch.rs` | The command line, and the look the window opens in |
| `src/theme.rs` | Every colour, radius, size and duration |
| `src/assets.rs` | The built-in fonts, icon font and backdrops |
| `src/baker.rs` | Makes the glass panes as pictures and keeps them |
| `src/shell.rs` | The frame: sidebar, panel and status bar |
| `src/app.rs` | The window and its drawing loop |
| `src/screenshot.rs` | `--screenshot`: saves a picture of the window |
| `src/contrast.rs` | A test-only module that checks every text's contrast on all six backdrops |
| `assets/` | The shipped fonts, icon font and backdrops, with a `README.md` on where each comes from |

### Elsewhere

| File | What is in it |
|---|---|
| `installer/` | The installer definitions and their build script |
| `.github/workflows/` | CI and the release build |

A tool is a feature in the code. It is built from atoms and never imports
another feature. Its state lives in the core; the feature in `src/` adds the
keys and the drawing. An atom names no feature type. The shell knows the tools
only through the `Feature` trait; `main.rs` hands them to it. The designs
are in `docs/superpowers/specs/`.

### Adding a tool

1. Put the tool's state in `crates/devassist-core/src/<name>/`.
2. Make `src/features/<name>/` with a struct that implements
   `shell::Feature`, wrapping that state.
3. Add it to the list in `main.rs`.

That is the whole registration. From the trait the shell makes the tool's
card on the welcome screen (`name`, `about`, `status`), its line in the
palette, its place in the top line and its page in the help dialog
(`help`). The shell also gives every tool text selection and the version
dialog. The tool draws into the area it is given, below the top line, and
reports `modal()` while a text field or one of its dialogs has the
keyboard, so the palette key and `?` stay out of its way.

The Docker tool has one test that talks to a real engine. It only touches
containers, an image tag and a volume it creates, named `devassist-test-*`:

```
cargo test --workspace -- --ignored live_docker
```

## Other systems

The code has branches for Linux and macOS (opening folders, file permissions,
the settings location), and nothing in it is Windows-only by design. But
releases, CI and day-to-day use are on Windows, so treat other systems as
untested. Reports and fixes are welcome.
