<p align="center">
  <img width="200" alt="Deltamod logo" src="./web/img/gblogo-outline.png">
</p>

<h1 align="center">Deltamod</h1>

<p align="center">
  A desktop mod manager for DELTARUNE and other supported GameMaker games.
</p>

<p align="center">
  <a href="https://github.com/deltamodders/deltamod/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/deltamodders/deltamod?style=flat-square"></a>
  <a href="./LICENSE.txt"><img alt="License: EUPL 1.2" src="https://img.shields.io/badge/license-EUPL--1.2-4c8bf5?style=flat-square"></a>
</p>

<p align="center">
  <a href="https://github.com/deltamodders/deltamod/releases">Download</a>
  ·
  <a href="https://gamebanana.com/tools/20575">GameBanana</a>
  ·
  <a href="https://github.com/deltamodders/deltamod/issues">Report an issue</a>
</p>

## What Deltamod does

Deltamod provides one place to manage game installations and compatible mods. It can:

- keep and switch between multiple managed game installations;
- import local mods or browse compatible releases in the built-in GameBanana Mod Shop;
- enable, disable, and configure mod variants before launching a game;
- connect to GameBanana for account and collection features;
- provide custom themes and a controller-friendly interface.

## Supported games

- DELTARUNE
- DELTARUNE Demo
- DELTARUNE Demo (LTS)
- UNDERTALE
- Undertale Yellow
- Pizza Tower

Support depends on the game version and the way each mod is packaged.

## Getting started

1. Download Deltamod from the [GitHub releases](https://github.com/deltamodders/deltamod/releases) or its [GameBanana page](https://gamebanana.com/tools/20575).
2. Launch Deltamod and select a supported game.
3. Let Deltamod locate the game automatically, or choose the installation folder manually.
4. Install mods through the Mod Shop or use **Import** for a local mod package.
5. Enable the mods you want, then launch the managed installation.

Deltamod imports the selected game into its own data directory. Keep the original installation available when creating a new managed installation.

## Running from source

### Requirements

- [Node.js](https://nodejs.org/)
- A compatible G3MTool executable

Clone the repository and install the Node.js dependencies:

```console
git clone https://github.com/deltamodders/deltamod.git
cd deltamod
npm install
```

This repository does not include the G3MTool executable. Place the correct build in `tools/` using the filename expected for your platform:

| Platform | Required path |
| --- | --- |
| Windows | `tools/G3MTool-win32.exe` |
| Linux | `tools/G3MTool-linux` |

Start Deltamod in developer mode:

```console
npm test
```

Despite its name, `npm test` currently launches the Electron app with developer mode enabled; it is not an automated test suite.

## Building

Install the dependencies first, then run the package command for your target:

| Target | Command | Package format |
| --- | --- | --- |
| Windows x64 | `npm run build-windows` | Portable executable |
| Windows x86 | `npm run build-win32` | Portable executable |
| Linux x64 | `npm run build-linux` | AppImage |

Electron Builder writes unpacked files and packaged artifacts to `dist/`.

### Windows installer

The optional Windows installer uses the project at `installbuilder/project.xml` and requires a licensed or trial copy of InstallBuilder Enterprise. InstallBuilder is not required to run Deltamod from source or to create the portable Electron package.

1. Run `npm run build-windows` to generate `dist/win-unpacked`.
2. Open `installbuilder/project.xml` in InstallBuilder Enterprise.
3. Build the project.

The InstallBuilder project packages the contents of `dist/win-unpacked`. The output location follows your local InstallBuilder configuration.

## Platform packaging

| Platform | Repository configuration |
| --- | --- |
| Windows | Portable x64 and x86 packages; optional InstallBuilder installer |
| Linux | AppImage package; automatic updates are not available |
| macOS | No maintained package target |

## Contributing

Bug reports and focused pull requests are welcome. Before submitting a change:

1. keep the scope small and describe the behavior being changed;
2. run the app in developer mode with `npm test`;
3. build the affected platform package when changing packaging or startup behavior.

Use the [issue tracker](https://github.com/deltamodders/deltamod/issues) for reproducible bugs and feature proposals.

## License

Deltamod is licensed under the [European Union Public Licence 1.2](./LICENSE.txt).

Third-party components retain their respective licenses.
