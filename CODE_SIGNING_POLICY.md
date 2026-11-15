# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## What is signed

Only the Windows executable, `Hopper-windows-x64.exe`, is signed. It is built by GitHub
Actions (`.github/workflows/release.yml`) on GitHub-hosted runners from the `main` branch of
this public repository and published on the
[download page](https://p1e2r.github.io/hopper/) and the
[Releases](https://github.com/p1e2r/hopper/releases) page. Nothing built anywhere else is
submitted for signing.

The macOS and Linux builds are not signed through SignPath.

## Team roles

- Committers and reviewers: [p1e2r](https://github.com/p1e2r)
- Approvers: [p1e2r](https://github.com/p1e2r)

Every signing request is approved by hand before it is signed. All team members use
multi-factor authentication on GitHub and SignPath.

## Privacy

Hopper does not collect telemetry and has no accounts. It only talks to the internet in
these cases:

- **Modrinth** (`api.modrinth.com`, `cdn.modrinth.com`): searching, downloading the mods,
  resource packs and shaders you choose, listing Minecraft versions, and, if "Recognise files
  on Modrinth" is on in Options, sending the SHA-1 hashes of files in your Minecraft folder to
  identify them. No file contents, names or paths are sent. Modrinth's own
  [privacy policy](https://modrinth.com/legal/privacy) applies to those requests.
- **Links you click** (a project page, the GitHub repository) open in your browser.

Settings are stored locally in your OS config folder (`hopper/config.json`).
