<p align="center">
  <img width="180" alt="Deltamod logo" src="./web/img/gblogo-outline.png">
</p>

<h1 align="center">Deltamod Community</h1>

<p align="center">A community-maintained desktop mod manager for DELTARUNE and other supported GameMaker games.</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/cmdr-chara/deltamod?include_prereleases&amp;sort=semver&amp;style=flat-square"></a>
  <a href="./LICENSE.txt"><img alt="License: EUPL 1.2" src="https://img.shields.io/badge/license-EUPL--1.2-4c8bf5?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://github.com/cmdr-chara/deltamod/releases">Download</a> ·
  <a href="https://gamebanana.com/tools/20575">GameBanana</a> ·
  <a href="https://github.com/cmdr-chara/deltamod/issues">Issues</a>
</p>

Deltamod Community manages multiple game installations, imports local mods, browses GameBanana, Nexus Mods, and ModDB catalogues, and applies selected patches transactionally before launch. It installs beside official Deltamod and uses a separate profile.

Supported games: **DELTARUNE**, **DELTARUNE Demo**, **DELTARUNE Demo (LTS)**, **UNDERTALE**, **Undertale Yellow**, and **Pizza Tower**. Compatibility still depends on the game version and how each mod is packaged.

## Platform status

| Setup | Stability | Notes |
| --- | --- | --- |
| Windows | In stabilization | NSIS beta builds are intentionally unsigned; Windows may show an unknown-publisher warning |
| Native Linux | Experimental | AppImage build; configurable Wine-compatible launcher |
| Native macOS | Unsupported | No maintained native build |
| Windows build through Wine/CrossOver | Unofficial | Generally usable; mention emulation when reporting issues |

## Run from source

Install [Node.js](https://nodejs.org/), clone the repository, and install its dependencies:

```console
git clone https://github.com/cmdr-chara/deltamod.git
cd deltamod
npm ci
```

Run the app and automated checks:

```console
npm run dev
npm test
npm run typecheck
npm run security:audit
```

Patching uses the GPL-3.0-only [G3MTool](https://github.com/y114git/G3MTool) executable. `npm run acquire:g3mtool` downloads the pinned Windows or Linux archive, enforces its size and SHA-256 checksums, validates its contents, and installs it under the ignored `tools/g3mtool/` directory. Release builds also publish the exact corresponding G3MTool source archive. See [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md).

## Import from official Deltamod

On first launch, Community detects the standard official profile and offers **Import from Deltamod**. It stages and validates the copy before committing it; the official profile is never modified. Settings provides the same action later for importing changes. Conflicting installations are copied separately, conflicting themes are renamed, and conflicting mod package IDs are quarantined for review. GameBanana may request a new login when credentials cannot be migrated securely.

## Mod catalogues

The Mod Shop keeps GameBanana as the default source and adds Nexus Mods and ModDB for DELTARUNE and UNDERTALE. Nexus single sign-on is implemented and becomes active after Nexus Mods issues the application slug. Until registration is complete, beta testers can use their own personal API key under **Options → Nexus Mods**. Credentials are validated and encrypted with the operating system’s credential protection; they are never bundled with the application. Premium API downloads are imported when the archive is Deltamod-compatible, while restricted downloads open the Nexus website for confirmation.

ModDB shows the recent downloads exposed by its official RSS feeds, clearly labels that list as incomplete, and links to the full game catalogue. Because ModDB archives are not necessarily Deltamod packages, Community opens their download page and leaves installation manual instead of claiming compatibility it cannot verify.

## Build

| Target | Command | Output |
| --- | --- | --- |
| Windows x64 | `npm run build-windows` | NSIS installer |
| Linux x64 | `npm run build-linux` | AppImage |

Artifacts are written to `dist/` by Electron Builder. Tags named `community-v<package version>` run the unsigned beta release workflow and create a GitHub prerelease. It refuses to publish when unit tests, the Electron workflow test, the production dependency audit, G3MTool provenance, or version matching fail. Authenticode signing can be enabled later for stable releases without changing the application data format.

## License

Deltamod Community is licensed under the [European Union Public Licence 1.2](./LICENSE.txt). Community contributions and modifications are identified in the [copyright notice](./NOTICE.md). Third-party components retain their respective licenses.
