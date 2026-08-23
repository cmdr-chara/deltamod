<p align="center">
  <img src="./web/img/gblogo-outline.png" width="360" alt="Deltamod Community">
</p>

<h1 align="center">Deltamod Community</h1>

<p align="center">
  <strong>Your mods. Your worlds. One launcher.</strong><br>
  A community-built mod manager for DELTARUNE, UNDERTALE,<br>and other supported GameMaker games.
</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/cmdr-chara/deltamod?sort=semver&amp;style=for-the-badge&amp;label=release&amp;color=7c5cff"></a>
  <a href="https://github.com/cmdr-chara/deltamod/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/cmdr-chara/deltamod/total?style=for-the-badge&amp;color=ef476f"></a>
  <a href="https://github.com/cmdr-chara/deltamod/stargazers"><img alt="GitHub stars" src="https://img.shields.io/github/stars/cmdr-chara/deltamod?style=for-the-badge&amp;color=ffd166"></a>
  <a href="https://github.com/cmdr-chara/deltamod/issues"><img alt="Open issues" src="https://img.shields.io/github/issues/cmdr-chara/deltamod?style=for-the-badge&amp;color=06d6a0"></a>
  <a href="./LICENSE.txt"><img alt="License: EUPL 1.2" src="https://img.shields.io/badge/license-EUPL--1.2-4c8bf5?style=for-the-badge"></a>
</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases/latest"><strong>Download Deltamod</strong></a>
  &nbsp;•&nbsp;
  <a href="#getting-started">Getting started</a>
  &nbsp;•&nbsp;
  <a href="#supported-games">Supported games</a>
  &nbsp;•&nbsp;
  <a href="./SUPPORT.md">Get help</a>
</p>

<p align="center">
  <img src="./art/thumbnail.png" alt="Deltamod Community artwork">
</p>

---

## What is Deltamod?

Deltamod Community helps you install, organize, and launch mods without manually moving game files around for every setup.

You can:

- browse compatible mods from **GameBanana**, **ModDB**, and **Nexus Mods**;
- import mod archives you already downloaded;
- keep different mod setups separated with installations, profiles, and collections;
- apply supported GameMaker patches, including UndertaleModTool `.csx` patches on compatible platforms;
- switch between eight interface languages and community themes.

If you only want to play with mods, you do **not** need Node.js, Rust, Git, or any development tools.

## Download and install

Get the newest stable build from the official GitHub release page:

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases/latest">
    <img alt="Download the latest Deltamod Community release" src="https://img.shields.io/badge/DOWNLOAD-LATEST-ef476f?style=for-the-badge&amp;logo=github">
  </a>
</p>

Choose the installer for your operating system. Windows and macOS packages are currently unsigned because this community project does not have paid platform-signing credentials.

> [!IMPORTANT]
> Download Deltamod only from this repository's official **Releases** page. Each stable release includes `SHA256SUMS.txt` so you can verify your download before installing it. Deltamod does not currently update itself automatically, so install newer releases manually when they are published.

### Windows

1. Download the Windows setup `.exe` from the latest release.
2. Check its SHA-256 hash against `SHA256SUMS.txt` on the release page.
3. Run the installer and open **Deltamod Community**.

Windows may show an **Unknown publisher** warning because the installer is not code-signed. Verify that you downloaded it from the official release and that the checksum matches before continuing.

### macOS

1. Download the DMG matching your Mac: **Apple Silicon** or **Intel**.
2. Open the DMG and move **Deltamod Community** to Applications.
3. Launch it from Applications.

The app is not currently notarized. After verifying the release checksum, macOS may require you to allow the app from **System Settings → Privacy & Security**.

Not sure which Mac you have? Open **Apple menu → About This Mac** and check whether it shows an Apple chip or an Intel processor.

### Linux

The official Linux release is a Debian package (`.deb`). Install it with your graphical package manager or from a terminal:

```console
sudo apt install ./deltamod-community-*.deb
```

Linux support is currently experimental. The official package targets x64 Debian-based systems.

> [!TIP]
> Files such as `.blockmap`, `.yml`, source bundles, or GitHub's automatic source archives are not normal installers. Use the `.exe`, `.dmg`, or `.deb` intended for your platform.

## Getting started

A normal first setup looks like this:

1. **Open Deltamod Community.**
2. **Add or select your game installation.** Deltamod can keep multiple installations separate.
3. **Choose a mod.** Browse a supported catalogue or import a local archive.
4. **Install the mod.** Deltamod prepares the package and adds it to your mod setup.
5. **Choose which mods you want active.** Keep separate setups when different mods or game versions should not mix.
6. **Patch and launch the game** from Deltamod.

Individual mods can require a specific game version or conflict with other mods. Read the mod author's instructions before installing it.

## Where can I get mods?

| Source | How it works |
| --- | --- |
| **GameBanana** | Browse supported catalogue entries directly in Deltamod. |
| **ModDB** | Browse supported listings and open compatible downloads. |
| **Nexus Mods** | Sign in from **Options → Nexus Mods** to browse Nexus content. |
| **Local archive** | Import a compatible mod archive you already downloaded. |

### Nexus Mods accounts

Nexus Mods sign-in uses your browser and returns you to Deltamod after authorization.

- **Premium users:** compatible archives can be downloaded through the Nexus Mods API.
- **Non-Premium users:** Nexus may require you to confirm the download on its website; once downloaded, import the archive into Deltamod.
- **No Nexus account:** GameBanana, ModDB, and local imports continue to work normally.

Deltamod does not ask you to paste a Nexus password or client secret into the app.

## Supported games

| Game | Support |
| --- | :---: |
| **DELTARUNE** | ✅ |
| **DELTARUNE Demo** | ✅ |
| **DELTARUNE Demo (LTS)** | ✅ |
| **UNDERTALE** | ✅ |
| **Undertale Yellow** | ✅ |
| **Pizza Tower** | ✅ |

Support for a game does not mean every mod works with every version of that game. Check the mod's own compatibility notes first.

## Platform support

| Platform | Status | Notes |
| --- | :---: | --- |
| Windows x64 | **Stable** | Recommended platform. |
| Linux x64 | **Experimental** | Official release is a `.deb` package. |
| macOS Apple Silicon | **Experimental** | Native app; UndertaleModTool `.csx` patches are unavailable. |
| macOS Intel | **Experimental** | Native app with `.csx` patch support. |
| Wine / CrossOver | **Unofficial** | May work, but is not an official release target. |

UndertaleModTool `.csx` patches are supported on Windows x64, Linux x64, and Intel Macs. Upstream does not currently provide the required CLI binary for Apple Silicon.

## Before installing mods

Modding changes how a game runs, so a few precautions are worth taking:

- **Back up important saves and game files** before experimenting with mods.
- Use mods from authors and sources you trust.
- Check that a mod supports your exact game version.
- Avoid combining mods unless their authors say they are compatible.
- Treat `.csx` patches as executable mod scripts and review their source/trustworthiness before allowing them to run.
- Do not upload copyrighted game files, passwords, tokens, or other private data when asking for support.

Deltamod includes validation, staging, and restore mechanisms around supported patch operations, but those safeguards are not a replacement for your own backup.

## See it in action

### Manage your setups

<p align="center">
  <img src="./art/readme/deltamod-app-tour.gif" width="960" alt="Deltamod Community navigating between the patch menu, installed mods, installations, and collections">
</p>

<p align="center"><sub>Move between your patch list, installed mods, game installations, and collections.</sub></p>

### Discover and install mods

<p align="center">
  <img src="./art/readme/deltamod-mod-shop.gif" width="960" alt="Deltamod Community Mod Shop preview and import progress">
</p>

<p align="center"><sub>Browse the Mod Shop, preview content, and follow download and import progress. Catalogue entries shown here use local demo data.</sub></p>

### Personalize the app

<p align="center">
  <img src="./art/readme/deltamod-personalization.gif" width="960" alt="Selecting a Deltamod theme and changing the interface language">
</p>

<p align="center"><sub>Choose a visual theme and switch between eight interface languages.</sub></p>

## Troubleshooting

### The game or mod does not work

- Confirm the mod supports your exact game version.
- Try the mod by itself in a separate setup to rule out conflicts.
- Check the mod author's installation notes and known issues.
- If the problem is caused only by one mod, report it to that mod's author first.

### Windows says the publisher is unknown

The Windows build is currently unsigned. Download it only from the official release page and verify its SHA-256 checksum against `SHA256SUMS.txt` before deciding whether to run it.

### macOS blocks the app

The macOS build is currently unsigned and not notarized. Verify the release checksum first, then use **System Settings → Privacy & Security** if macOS requires manual approval.

### Nexus download does not start

If you use a Non-Premium Nexus Mods account, Nexus may require the download to be confirmed on its website. Download the archive there, then import it into Deltamod.

For account authorization problems, try signing in again from **Options → Nexus Mods**.

### I still need help

Read [SUPPORT.md](./SUPPORT.md) or [open an issue](https://github.com/cmdr-chara/deltamod/issues/new/choose).

When reporting a problem, include your Deltamod version, operating system, game/version, mod/source, reproduction steps, and sanitized logs or screenshots when useful.

For security vulnerabilities, **do not open a public issue**. Follow [SECURITY.md](./SECURITY.md) and use GitHub private vulnerability reporting.

## About Deltamod Community

Deltamod Community is an independent community fork of Deltamod. It was created in response to concerns about censorship in the original repository and aims to preserve freedom of creative expression while respecting applicable laws, platform rules, and community safety.

This project is independent from the original Deltamod project and is not affiliated with or endorsed by Toby Fox.

## For contributors and developers

You do not need this section to install or use Deltamod.

Development requires [Node.js 22](https://nodejs.org/). Tauri builds additionally require the platform prerequisites for Tauri development.

```console
git clone https://github.com/cmdr-chara/deltamod.git
cd deltamod
npm ci
npm run dev
```

Common checks:

```console
npm test
npm run typecheck
npm run security:audit
```

Build commands:

| Target | Command |
| --- | --- |
| Windows | `npm run build-windows` |
| Linux | `npm run build-linux` |
| macOS | `npm run build-macos` |
| Tauri | `npm run build:tauri` |

<details>
<summary>Technical details: Nexus integration</summary>

Deltamod uses **OAuth 2.0 Authorization Code with PKCE S256** for Nexus Mods sign-in. The registered loopback callback is fixed at `http://127.0.0.1:52817/callback`; the client **never falls back to a dynamic port**.

Nexus catalogue browsing uses a **bounded result page** of 50 items and fetches only the requested page. Nexus quota handling honors `Retry-After` and quota reset metadata so requests can pause and retry instead of blindly hammering the API.

</details>

Patching uses [G3MTool](https://github.com/y114git/G3MTool) and the [UndertaleModTool CLI](https://github.com/UnderminersTeam/UndertaleModTool). For contribution, release, and third-party details, see:

- [CONTRIBUTING.md](./CONTRIBUTING.md)
- [RELEASE-GATE.md](./docs/RELEASE-GATE.md)
- [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)
- [NOTICE.md](./NOTICE.md)

## License

Deltamod Community is licensed under the [European Union Public Licence 1.2](./LICENSE.txt). Attribution and bundled third-party software are documented in [NOTICE.md](./NOTICE.md) and [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).

---

<p align="center">
  Made with <span aria-label="determination">❤️</span> by the Deltamod community.<br>
  <sub>An independent fan project. Not affiliated with or endorsed by Toby Fox.</sub>
</p>
