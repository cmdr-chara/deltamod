<p align="center">
  <img width="180" alt="Deltamod logo" src="./web/img/gblogo-outline.png">
</p>

<h1 align="center">Deltamod Community</h1>

<p align="center">Install and manage mods for DELTARUNE, UNDERTALE, and other supported GameMaker games.</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/cmdr-chara/deltamod?include_prereleases&amp;sort=semver&amp;style=flat-square&amp;cacheSeconds=300"></a>
  <a href="./LICENSE.txt"><img alt="License: EUPL 1.2" src="https://img.shields.io/badge/license-EUPL--1.2-4c8bf5?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases">Download</a> ·
  <a href="https://gamebanana.com/tools/20575">GameBanana</a> ·
  <a href="https://github.com/cmdr-chara/deltamod/issues">Issues</a>
</p>

## Download

| System | Download |
| --- | --- |
| Windows 64-bit | **[Download installer](https://github.com/cmdr-chara/deltamod/releases/download/community-v2.0.3-beta.3/deltamod-community-2.0.3-beta.3-win-x64.exe)** |
| Mac with Apple chip | [Download DMG](https://github.com/cmdr-chara/deltamod/releases/download/community-v2.0.3-beta.3/deltamod-community-2.0.3-beta.3-mac-arm64.dmg) |
| Mac with Intel chip | [Download DMG](https://github.com/cmdr-chara/deltamod/releases/download/community-v2.0.3-beta.3/deltamod-community-2.0.3-beta.3-mac-x64.dmg) |
| Linux 64-bit | [Download AppImage](https://github.com/cmdr-chara/deltamod/releases/download/community-v2.0.3-beta.3/deltamod-community-2.0.3-beta.3-linux-x86_64.AppImage) |

This is a beta. Windows and macOS builds are not signed yet.

### Install on Windows

1. Open the downloaded installer.
2. If Windows shows **Windows protected your PC**, click **More info**.
3. Click **Run anyway**.
4. Finish the installer and open **Deltamod Community**.

### Install on macOS

1. Open the downloaded DMG and move Deltamod Community to Applications.
2. If macOS blocks it, open **System Settings > Privacy & Security**.
3. Click **Open Anyway**.

To check your Mac type, open **Apple menu > About This Mac** and look for
**Chip: Apple** or **Processor: Intel**.

### Install on Linux

Make the AppImage executable, then open it:

```console
chmod +x deltamod-community-*.AppImage
./deltamod-community-*.AppImage
```

Do not download `.blockmap`, `.yml`, G3MTool source, or GitHub's automatic
source archives unless you are a developer.

## Features

- Install and remove compatible mod packages.
- Import local mod archives.
- Apply UndertaleModTool `.csx` script patches after an explicit safety warning.
- Browse and install supported GameBanana mods.
- Browse ModDB listings and open their download pages.
- Keep multiple game installations and mod setups.
- Import data from official Deltamod without changing the official profile.

Nexus Mods support is present but disabled while the application registration
is pending. When enabled, it uses single-sign-on only, requests a bounded result page,
and respects quota responses using the server's `Retry-After` value. You
do not need Nexus Mods to use Deltamod Community.

## Supported games

- DELTARUNE
- DELTARUNE Demo
- DELTARUNE Demo (LTS)
- UNDERTALE
- Undertale Yellow
- Pizza Tower

Individual mods may only work with specific game versions.

## Platform status

| Platform | Status |
| --- | --- |
| Windows 64-bit | Beta |
| Linux 64-bit | Experimental |
| macOS Apple and Intel | Experimental |
| Wine or CrossOver | Unofficial |

UndertaleModTool `.csx` patches currently require Windows x64, Linux x64, or
an Intel Mac because upstream does not publish an Apple Silicon CLI binary.
Script patches use `<patch type="csx" patch="scripts/patch.csx" to="data.win" />`.
Deltamod snapshots the complete mod directory before execution, so scripts can
load companion files relative to their staged script path without accessing a
partially changed package.

## Help

- [Report a bug](https://github.com/cmdr-chara/deltamod/issues/new/choose)
- [View all releases](https://github.com/cmdr-chara/deltamod/releases)
- [GameBanana page](https://gamebanana.com/tools/20575)
- For security problems, read [SECURITY.md](./SECURITY.md).

When reporting a problem, include your operating system, game, mod, and the
steps that caused it.

## Development

Requires [Node.js 22](https://nodejs.org/).

```console
git clone https://github.com/cmdr-chara/deltamod.git
cd deltamod
npm ci
npm run dev
```

Checks:

```console
npm test
npm run typecheck
npm run security:audit
```

Builds are written to `dist/`:

| System | Command |
| --- | --- |
| Windows | `npm run build-windows` |
| Linux | `npm run build-linux` |
| macOS | `npm run build-macos` |

Patching uses [G3MTool](https://github.com/y114git/G3MTool) and the
[UndertaleModTool CLI](https://github.com/UnderminersTeam/UndertaleModTool).
Run `npm run acquire:g3mtool` and `npm run acquire:undertale-mod-tool` to
download and verify the pinned builds. Licensing and source details are in
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).

## License

[EUPL 1.2](./LICENSE.txt). See [NOTICE.md](./NOTICE.md) and
[THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md) for attribution and bundled
third-party software.
