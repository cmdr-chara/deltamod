const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const configPath = path.join(root, 'src-tauri', 'tauri.conf.json');
const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));

function resolveFromTauri(relativePath) {
    return path.resolve(root, 'src-tauri', relativePath);
}

function readBmpDimensions(filePath) {
    const data = fs.readFileSync(filePath);
    expect(data.subarray(0, 2).toString('ascii')).toBe('BM');
    return {
        width: data.readInt32LE(18),
        height: Math.abs(data.readInt32LE(22))
    };
}

describe('Windows installer branding', () => {
    it('uses the Deltamod NSIS artwork and release metadata', () => {
        const nsis = config.bundle.windows.nsis;

        expect(config.bundle.publisher).toBe('Deltamod Community contributors');
        expect(config.bundle.homepage).toBe('https://github.com/cmdr-chara/deltamod');
        expect(config.bundle.licenseFile).toBe('../LICENSE.txt');
        expect(nsis.displayLanguageSelector).toBe(false);
        expect(nsis.startMenuFolder).toBe('Deltamod Community');

        const header = resolveFromTauri(nsis.headerImage);
        const sidebar = resolveFromTauri(nsis.sidebarImage);
        expect(fs.existsSync(header)).toBe(true);
        expect(fs.existsSync(sidebar)).toBe(true);
        expect(readBmpDimensions(header)).toEqual({ width: 150, height: 57 });
        expect(readBmpDimensions(sidebar)).toEqual({ width: 164, height: 314 });
    });

    it('builds the release shell as a Windows GUI executable', () => {
        const main = fs.readFileSync(path.join(root, 'src-tauri', 'src', 'main.rs'), 'utf8');
        expect(main).toContain('#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]');
    });

    it('ships the themed standalone setup route and build contract', () => {
        const packageJson = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
        const setupHtml = fs.readFileSync(path.join(root, 'web', 'installer', 'index.html'), 'utf8');
        const setupScript = fs.readFileSync(path.join(root, 'web', 'installer', 'index.js'), 'utf8');
        const shell = fs.readFileSync(path.join(root, 'src-tauri', 'src', 'main.rs'), 'utf8');
        const workflow = fs.readFileSync(path.join(root, '.github', 'workflows', 'tauri-release.yml'), 'utf8');

        expect(packageJson.scripts['build:installer']).toBe('node scripts/build-installer.js');
        expect(fs.existsSync(path.join(root, 'web', 'installer', 'index.css'))).toBe(true);
        expect(setupHtml).toContain('id="installer-form"');
        expect(setupScript).toContain("installerInstall");
        expect(setupScript).toContain("installer-progress");
        expect(shell).toContain('DELTAMOD_INSTALLER_MODE');
        expect(shell).toContain('DELTAMOD_INSTALLER_SHA256');
        expect(shell).toContain('failed its integrity check');
        expect(shell).toContain('https://github.com/');
        expect(shell).toContain('Deltamod.Community_{version}_x64-setup.exe');
        expect(shell).toContain('stable Deltamod release package is not available yet');
        expect(shell).toContain('release server took too long to respond');
        expect(shell).toContain('Release package found; starting transfer');
        expect(shell).toContain('Secure channel ready; waiting for the release server');
        expect(shell).toContain('could not create the selected installation folder');
        expect(shell).toContain('launcher was not found in the selected folder');
        expect(shell).toContain('Moving Deltamod Community to the selected folder');
        expect(workflow).toContain('Build Deltamod-themed setup shell');
    });
});
