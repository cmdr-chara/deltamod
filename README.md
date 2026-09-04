<p align="center">
  <img src="./web/img/gblogo-outline.png" width="360" alt="Deltamod Community">
</p>

<h1 align="center">Deltamod Community</h1>

<p align="center">
  <strong>Install, organize, and launch GameMaker mods without juggling game files.</strong><br>
  A community-built mod manager for DELTARUNE, UNDERTALE, and other supported games.
</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases/latest"><img alt="Latest release: 2.0.18" src="https://img.shields.io/badge/latest_release-2.0.18-7c5cff?style=for-the-badge"></a>
  <a href="https://github.com/cmdr-chara/deltamod/releases"><img alt="Downloads" src="https://img.shields.io/github/downloads/cmdr-chara/deltamod/total?style=for-the-badge&amp;color=ef476f"></a>
  <a href="./LICENSE.txt"><img alt="License: EUPL 1.2" src="https://img.shields.io/badge/license-EUPL--1.2-4c8bf5?style=for-the-badge"></a>
</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases/latest"><strong>Download the latest release</strong></a>
  &nbsp;•&nbsp;
  <a href="#get-started">Get started</a>
  &nbsp;•&nbsp;
  <a href="#supported-games-and-platforms">Compatibility</a>
  &nbsp;•&nbsp;
  <a href="./SUPPORT.md">Get help</a>
</p>

<p align="center">
  <img src="./art/thumbnail.png" alt="Deltamod Community artwork">
</p>

## Why Deltamod Community?

Deltamod Community keeps each mod setup organized and gives you a safer path back when an installation goes wrong.

- **Find mods in one place.** Browse compatible content from GameBanana, ModDB, and Nexus Mods, or import a local archive.
- **Keep setups separate.** Use installations, profiles, and collections instead of repeatedly moving game files by hand.
- **Patch with recovery in mind.** Supported operations use staging, verification, and rollback paths before publishing changes to a game installation.
- **Understand what is installed.** Inspect mod health, versions, files, conflicts, and operation history from the Installed Mods view.
- **Use the platform you already have.** Native packages are available for Windows, Linux, Intel Mac, and Apple Silicon Mac.

You do **not** need Node.js, Rust, Git, or other development tools to use the app.

## Download

The current build is the **2.0.18 unsigned Tauri release**. Open the release page, expand **Assets**, and download one installer from this table:

| Your computer | Download this file |
| --- | --- |
| Windows PC with a 64-bit Intel or AMD processor | `Deltamod-Community_2.0.18_x86_64-pc-windows-msvc-setup.exe` |
| Mac with an Apple chip (M1 or newer) | `Deltamod.Community_2.0.18_aarch64.dmg` |
| Mac with an Intel processor | `Deltamod.Community_2.0.18_x64.dmg` |
| Debian, Ubuntu, Linux Mint, or another Debian-based x64 distribution | `Deltamod.Community_2.0.18_amd64.deb` |

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases/latest">
    <img alt="Download Deltamod Community 2.0.18" src="https://img.shields.io/badge/DOWNLOAD-2.0.18-ef476f?style=for-the-badge&amp;logo=github">
  </a>
</p>

> [!TIP]
> Most players need only one `.exe`, `.dmg`, or `.deb`. The `.json` evidence files, signatures, checksums, tool source archives, and GitHub-generated source archives are not installers.

> [!WARNING]
> Windows and macOS packages are not currently code-signed, and the macOS packages are not notarized. Your operating system may show an unknown-publisher warning. Download only from this repository, verify the attached `SHA256SUMS.txt`, and install manually when a newer release is published.

<details>
<summary><strong>Installation and checksum help</strong></summary>

### Windows

1. Download the Windows setup `.exe`.
2. Compare its SHA-256 hash with the matching entry in `SHA256SUMS.txt`.
3. Run the installer and open **Deltamod Community**.

Windows SmartScreen may show **Windows protected your PC** because the installer is unsigned. After verifying the download, choose **More info → Run anyway** if you decide to proceed.

### macOS

1. Check **Apple menu → About This Mac** to see whether your Mac uses an Apple chip or Intel processor.
2. Download the matching DMG and verify it against `SHA256SUMS.txt`.
3. Open the DMG and move **Deltamod Community** to Applications.

If Gatekeeper blocks the app after verification, try opening it once, then use **System Settings → Privacy & Security → Open Anyway**.

### Linux

Install the Debian package with your graphical package manager or run this command from the download directory:

```console
sudo apt install ./Deltamod.Community_2.0.18_amd64.deb
```

### Verify a checksum

| System | Command |
| --- | --- |
| Windows PowerShell | `Get-FileHash -Algorithm SHA256 -LiteralPath '.\Deltamod-Community_2.0.18_x86_64-pc-windows-msvc-setup.exe'` |
| macOS Apple Silicon | `shasum -a 256 Deltamod.Community_2.0.18_aarch64.dmg` |
| macOS Intel | `shasum -a 256 Deltamod.Community_2.0.18_x64.dmg` |
| Linux x64 | `sha256sum Deltamod.Community_2.0.18_amd64.deb` |

Delete the installer and download it again if the hashes do not match exactly.

</details>

## Get started

1. Open Deltamod Community and add your game installation.
2. Browse the Mod Shop or import a compatible mod archive.
3. Install the mod and choose the profile or collection where it belongs.
4. Review compatibility or conflict information, then patch and launch the game.
5. Keep separate profiles for mods or game versions that should not be combined.

Always check the mod author's compatibility notes. A supported game does not mean every mod supports every version of that game.

## See it in action

### Discover and install mods

<p align="center">
  <img src="./art/readme/deltamod-mod-shop.gif" width="960" alt="Deltamod Community browsing mods and following import progress">
</p>

### Manage separate setups

<p align="center">
  <img src="./art/readme/deltamod-app-tour.gif" width="960" alt="Deltamod Community navigating installed mods, installations, and collections">
</p>

<details>
<summary><strong>Personalize the app</strong></summary>

<p align="center">
  <img src="./art/readme/deltamod-personalization.gif" width="960" alt="Selecting a Deltamod theme and changing the interface language">
</p>

Deltamod Community includes community themes and eight interface languages.

</details>

## Supported games and platforms

| Game | Support |
| --- | :---: |
| DELTARUNE | ✅ |
| DELTARUNE Demo | ✅ |
| DELTARUNE Demo (LTS) | ✅ |
| UNDERTALE | ✅ |
| Undertale Yellow | ✅ |
| Pizza Tower | ✅ |

| Platform | Status | Notes |
| --- | :---: | --- |
| Windows x64 | **Stable** | Recommended platform |
| Linux x64 | **Experimental** | Official package targets Debian-based distributions |
| macOS Apple Silicon | **Experimental** | Native app; UndertaleModTool `.csx` patches are unavailable |
| macOS Intel | **Experimental** | Native app with `.csx` patch support |
| Wine / CrossOver | **Unofficial** | May work, but is not a release target |

UndertaleModTool `.csx` patches are supported on Windows x64, Linux x64, and Intel Macs. The required upstream CLI is not currently available for Apple Silicon.

## Mod sources

| Source | How it works |
| --- | --- |
| **GameBanana** | Browse supported catalogue entries inside Deltamod |
| **ModDB** | Browse listings and open compatible downloads |
| **Nexus Mods** | Sign in from **Options → Nexus Mods** to browse Nexus content |
| **Local archive** | Import a compatible archive you already downloaded |

Nexus Premium members can download compatible archives through the API. Nexus may require non-Premium members to confirm a download on its website and then import the downloaded archive. GameBanana, ModDB, and local imports do not require a Nexus account.

## Play safely

- Back up important saves and game files before experimenting.
- Use mods from authors and sources you trust.
- Check that each mod supports your exact game version.
- Avoid combining mods unless their authors say they are compatible.
- Treat `.csx` patches as executable scripts and review their source or trustworthiness before running them.

Deltamod adds validation, staging, and recovery around supported operations, but these safeguards do not replace your own backup.

## Help and contributing

- Read [SUPPORT.md](./SUPPORT.md) or [open a bug report](https://github.com/cmdr-chara/deltamod/issues/new/choose).
- Read [CONTRIBUTING.md](./CONTRIBUTING.md) before proposing a change.
- Report security vulnerabilities privately by following [SECURITY.md](./SECURITY.md).

Never upload copyrighted game files, passwords, tokens, or other private data when requesting help.

<details>
<summary><strong>Developer setup</strong></summary>

Development requires Node.js 22. Rust and the platform prerequisites for Tauri are required for native builds.

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

### Nexus integration contract

Deltamod uses **OAuth 2.0 Authorization Code with PKCE S256** for Nexus Mods sign-in. The registered loopback callback is fixed at `http://127.0.0.1:52817/callback`; the client **never falls back to a dynamic port**.

Nexus catalogue browsing uses a **bounded result page** of 50 items and fetches only the requested page. Quota handling honors `Retry-After` and quota reset metadata so requests can pause and retry instead of repeatedly hitting the API while limited.

See [CONTRIBUTING.md](./CONTRIBUTING.md) for native checks and [RELEASE-GATE.md](./docs/RELEASE-GATE.md) for release requirements.

</details>

## About this community fork

Deltamod Community is an independent, community-maintained fork of Deltamod. It continues development across the application, native layer, recovery model, tests, and release system while supporting lawful creative expression and community safety.

The project is not affiliated with or endorsed by Toby Fox. Patching uses [G3MTool](https://github.com/y114git/G3MTool) and the [UndertaleModTool CLI](https://github.com/UnderminersTeam/UndertaleModTool).

## License and attribution

Deltamod Community is licensed under the [European Union Public Licence 1.2](./LICENSE.txt). Attribution, ownership boundaries, provenance, and bundled third-party software are documented in [NOTICE.md](./NOTICE.md), [COPYRIGHT.md](./COPYRIGHT.md), [PROVENANCE.md](./PROVENANCE.md), and [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).

---

<p align="center">
  Made with <span aria-label="determination">❤️</span> by the Deltamod community.<br>
  <sub>An independent fan project. Not affiliated with or endorsed by Toby Fox.</sub>
</p>
