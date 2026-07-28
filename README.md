<p align="center">
  <img width="180" alt="Deltamod logo" src="./web/img/gblogo-outline.png">
</p>

<h1 align="center">Deltamod</h1>

<p align="center">A desktop mod manager for DELTARUNE and other supported GameMaker games.</p>

<p align="center">
  <a href="https://github.com/deltamodders/deltamod/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/deltamodders/deltamod?style=flat-square"></a>
  <a href="./LICENSE.txt"><img alt="License: EUPL 1.2" src="https://img.shields.io/badge/license-EUPL--1.2-4c8bf5?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://github.com/deltamodders/deltamod/releases">Download</a> ·
  <a href="https://gamebanana.com/tools/20575">GameBanana</a> ·
  <a href="https://github.com/deltamodders/deltamod/issues">Issues</a>
</p>

Deltamod manages multiple game installations, imports local mods, browses compatible GameBanana releases, and lets you enable mods or select variants before launching.

Supported games: **DELTARUNE**, **DELTARUNE Demo**, **DELTARUNE Demo (LTS)**, **UNDERTALE**, **Undertale Yellow**, and **Pizza Tower**. Compatibility still depends on the game version and how each mod is packaged.

## Platform status

| Setup | Stability | Notes |
| --- | --- | --- |
| Windows | Stable | Official builds, developer-tested, supported, and auto-updating |
| Native Linux | Experimental | AppImage available; limited developer testing; DELTARUNE requires Proton; no auto-updates |
| Native macOS | Unsupported | No maintained native build |
| Windows build through Wine/CrossOver | Unofficial | Generally usable; mention emulation when reporting issues |

## Run from source

Install [Node.js](https://nodejs.org/), clone the repository, and install its dependencies:

```console
git clone https://github.com/deltamodders/deltamod.git
cd deltamod
npm install
```

Add a compatible G3MTool executable as `tools/G3MTool-win32.exe` on Windows or `tools/G3MTool-linux` on Linux, then run:

```console
npm test
```

`npm test` launches Electron in developer mode; it is not an automated test suite.

## Build

| Target | Command | Output |
| --- | --- | --- |
| Windows x64 | `npm run build-windows` | Portable executable |
| Windows x86 | `npm run build-win32` | Portable executable |
| Linux x64 | `npm run build-linux` | AppImage |

Artifacts are written to `dist/`. The optional Windows installer uses `installbuilder/project.xml` and requires InstallBuilder Enterprise; it is not required for source development or portable builds.

## License

Deltamod is licensed under the [European Union Public Licence 1.2](./LICENSE.txt). Third-party components retain their respective licenses.
