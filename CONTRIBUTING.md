# Contributing to DevAssist

Thank you for wanting to help. This page covers reporting problems, proposing
changes, and what a pull request needs.

By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting a bug

Open a [bug report](https://github.com/AsP3X/DevAssist/issues/new/choose).
It helps a lot if you include:

- what you did, what you expected, and what happened instead
- the output of `devassist --doctor` (it prints the version, the settings and
  the state of each clone; remove anything you do not want to share)
- your Windows and Git versions (`git --version`)

Do not report security problems in a public issue. See [SECURITY.md](SECURITY.md).

## Suggesting a change

Open a [feature request](https://github.com/AsP3X/DevAssist/issues/new/choose)
and describe the problem you want solved before the solution. For anything
larger than a small fix, please open an issue before writing code, so nobody
builds something that does not fit.

## Working on the code

[docs/BUILDING.md](docs/BUILDING.md) has the full details. In short:

```
git clone https://github.com/AsP3X/DevAssist
cd DevAssist
cargo run -- --config scratch.toml
```

`--config scratch.toml` keeps your experiments away from your real settings.

Before you open a pull request, these must pass. CI runs the same three:

```
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

### What a good change looks like

- **One thing per pull request.** A fix and an unrelated clean-up are two
  pull requests.
- **Tests come with it.** Logic gets a unit test next to the code. Anything
  on screen gets a screen test in `src/ui.rs`: those draw into an in-memory
  terminal and check the text. Git behaviour is tested against a real
  throwaway repository; see the tests at the end of `src/gitops.rs`.
- **Tests stay local.** No test may call GitHub, open a browser, change the
  clipboard, or write outside the system's temporary folder.
- **It never loses work.** DevAssist does not merge, rebase, reset or force
  anything in a user's clone, and it deletes a folder only after asking. A
  change that weakens this needs a very good reason.
- **Text is plain.** Messages say what happened and what to do, in ordinary
  words.
- **Comments explain why.** A comment says what the code cannot say for
  itself.

### Commits

Write the subject line as a short sentence that says what the change does,
for example `Show commits on remote branches in the sync column`. Explain the
reason in the body when it is not obvious.

### Changelog

If users will notice the change, add a line under `## [Unreleased]` in
[CHANGELOG.md](CHANGELOG.md).

## Pull requests

1. Fork the repository and branch from `master`.
2. Make the change, with tests.
3. Run the three checks above.
4. Open the pull request and fill in the template.

A maintainer reviews it. Small fixes are usually quick; larger changes can take
a few rounds.

## License of contributions

DevAssist is licensed under the Apache License 2.0 with the Commons Clause
([LICENSE](LICENSE)). By submitting a contribution you agree that it is
licensed under those same terms, and you confirm that you have the right to
submit it.
