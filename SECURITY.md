# Security

## Supported versions

Security fixes go into the latest release. DevAssist updates itself, so the
fix for an older version is to update.

## Reporting a vulnerability

Please do not open a public issue for a security problem.

Report it privately through GitHub: on the repository's **Security** tab,
choose **Report a vulnerability**
([direct link](https://github.com/AsP3X/DevAssist/security/advisories/new)).
Describe what you found, how to reproduce it, and what an attacker could do
with it.

You can expect a first answer within a week. Once a fix is released, the
report is published as a security advisory, with credit to you unless you
prefer otherwise.

## What DevAssist does with your data

Knowing this helps to judge a report.

- **It runs `git` and deletes folders.** It operates on the clones on its
  list. It deletes a folder only after you confirm, and only if the folder is
  a git checkout.
- **It reads a GitHub token.** From `GH_TOKEN`, `GITHUB_TOKEN`, or the GitHub
  CLI. The token is sent to `api.github.com` and, for git over https, to
  `github.com`. It is never written to disk or printed.
- **It talks to GitHub only.** To list repositories, to fetch and clone, and
  to look for and download updates.
- **It updates itself.** It downloads a new executable from this repository's
  releases over HTTPS and checks it against the release's SHA-256 list before
  replacing itself. The releases are not code-signed. Self-update can be
  switched off with `update_check = off`; the app then never installs anything
  unless you press `v` and confirm.

Problems in any of these areas are in scope. So is anything that makes
DevAssist run a command, write a file or delete a folder that the user did not
ask for, for example through a crafted repository name, branch name or
settings file.
