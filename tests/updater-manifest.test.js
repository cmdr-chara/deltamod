const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { generate, updaterTarget } = require('../scripts/generate-tauri-updater-manifest');

describe('Tauri updater manifest generation', () => {
    it('binds only signed Windows and macOS artifacts to the stable release tag', () => {
        const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-updater-'));
        try {
            for (const name of [
                'Deltamod_2.1.0_x64-setup.exe',
                'Deltamod_2.1.0_x64.app.tar.gz',
                'Deltamod_2.1.0_aarch64.app.tar.gz'
            ]) {
                fs.writeFileSync(path.join(directory, name), 'signed payload');
                fs.writeFileSync(path.join(directory, `${name}.sig`), 'trusted signature');
            }
            const manifest = generate(directory, 'community-v2.1.0');
            expect(manifest.version).toBe('2.1.0');
            expect(Object.keys(manifest.platforms).sort()).toEqual([
                'darwin-aarch64', 'darwin-x86_64', 'windows-x86_64'
            ]);
            expect(manifest.platforms['windows-x86_64'].url)
                .toContain('/community-v2.1.0/Deltamod_2.1.0_x64-setup.exe');
        } finally {
            fs.rmSync(directory, { recursive: true, force: true });
        }
    });

    it('rejects unclassified updater packages and duplicate targets', () => {
        expect(() => updaterTarget('Deltamod.app.tar.gz')).toThrow(/architecture/);
        const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-updater-'));
        try {
            for (const name of ['one_2.1.0_x64-setup.exe', 'two_2.1.0_x64-setup.exe']) {
                fs.writeFileSync(path.join(directory, name), 'payload');
                fs.writeFileSync(path.join(directory, `${name}.sig`), 'signature');
            }
            expect(() => generate(directory, 'community-v2.1.0')).toThrow(/Duplicate/);
        } finally {
            fs.rmSync(directory, { recursive: true, force: true });
        }
    });

    it('rejects an updater artifact built for a different version', () => {
        const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-updater-'));
        try {
            const artifact = path.join(directory, 'Deltamod_2.0.9_x64-setup.exe');
            fs.writeFileSync(artifact, 'payload');
            fs.writeFileSync(`${artifact}.sig`, 'signature');
            expect(() => generate(directory, 'community-v2.1.0')).toThrow(/not bound/);
        } finally {
            fs.rmSync(directory, { recursive: true, force: true });
        }
    });
});
