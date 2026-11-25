# Releasing

A release is made by pushing a version tag. GitHub Actions builds everything
and publishes it; installed copies of DevAssist then find it on their own.

## Steps

1. Set the new version in `Cargo.toml`, under `[workspace.package]`, and run `cargo build` so `Cargo.lock`
   follows.
2. Add a section for the version to `CHANGELOG.md`, headed `## [1.2.3] - YYYY-MM-DD`.
   That section becomes the release notes, and it is what people see in the
   app before they install the update.
3. Commit, and push to `master`. Wait for CI to pass.
4. Tag that commit and push the tag:

   ```
   git tag v1.2.3
   git push origin v1.2.3
   ```

The tag must be `v` plus the version in `Cargo.toml`. The release workflow
stops if they differ.

## What the workflow does

`.github/workflows/release.yml` runs on a tag that looks like `v1.2.3`:

1. Checks that the tag matches `Cargo.toml`.
2. Runs the tests.
3. Builds the release executable and both installers with `installer\build.ps1`.
4. Publishes a GitHub release for the tag with these files:

   | File | For |
   |---|---|
   | `devassist-<version>-setup.exe` | People installing with the wizard |
   | `devassist-<version>-x64.msi` | Installing for all users, deployment |
   | `devassist-<version>-x64.exe` | The plain program, and self-update |
   | `SHA256SUMS.txt` | Checking any of the files |

Versions follow [semantic versioning](https://semver.org/). A release must not
be marked as a pre-release or left as a draft if installed copies should see
it: the app asks GitHub for the latest full release.

## How self-update works

The code is in `crates/devassist-core/src/update.rs`; the dialog is in `src/shell/updater.rs`.

1. **Look.** At start-up, and when the user presses `v`, the app asks
   `https://api.github.com/repos/AsP3X/DevAssist/releases/latest`. If the tag
   is a higher version than the running one, an update is available.
2. **Download.** When the user confirms, it downloads
   `devassist-<version>-x64.exe` next to the running executable, as
   `devassist.exe.new`.
3. **Check.** It downloads `SHA256SUMS.txt` and compares the SHA-256 of the
   download with the listed one. On a mismatch nothing is changed.
4. **Swap.** Windows does not allow overwriting a running program, but it
   allows renaming it. The running file becomes `devassist.exe.old`, the new
   one becomes `devassist.exe`. If that fails, the old file is put back.
5. **Restart.** The app offers to restart. The leftover `.old` file is deleted
   at the next start.

Things that follow from this design:

- The file names matter. The updater looks for a file ending in `-x64.exe`
  that is not the setup, and for `SHA256SUMS.txt`. Do not rename them.
- The app must be able to write to its own folder. An install in
  `Program Files` cannot update itself; the app says so and names the release
  page.
- The checksum guards against a broken download. What makes the download
  trustworthy is that it comes from this repository's releases over HTTPS. The
  files are not code-signed.
- Only 64-bit Windows has a download. On other systems the app reports that
  the release has none.

## Signing

The executables and installers are not code-signed, so Windows SmartScreen
warns on first run. Signing needs a code-signing certificate; with one, a
signing step belongs in `installer\build.ps1` before the checksums are
written.
