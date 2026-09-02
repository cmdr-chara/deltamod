const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const verifier = path.join(__dirname, '..', 'scripts', 'verify-tauri-package.js');

function runVerifier(bundle, ...args) {
    return spawnSync(process.execPath, [verifier, bundle, 'x86_64-pc-windows-msvc', ...args], {
        encoding: 'utf8'
    });
}

describe('Tauri package updater policy', () => {
    test('accepts an unsigned manual-download package only in explicit preview mode', () => {
        const bundle = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-unsigned-package-'));
        try {
            const installer = path.join(bundle, 'Deltamod Community_2.0.18_x64-setup.exe');
            fs.writeFileSync(installer, Buffer.alloc(1024 * 1024));

            const preview = runVerifier(bundle, '--unsigned');
            expect(preview.status, preview.stderr).toBe(0);
            expect(preview.stdout).toContain('Verified unsigned preview');

            const stable = runVerifier(bundle);
            expect(stable.status).not.toBe(0);
            expect(stable.stderr).toContain('exactly one Tauri signature');

            fs.writeFileSync(`${installer}.sig`, 'not-a-real-signature');
            const contaminatedPreview = runVerifier(bundle, '--unsigned');
            expect(contaminatedPreview.status).not.toBe(0);
            expect(contaminatedPreview.stderr).toContain('must not contain automatic updater signatures');
        } finally {
            fs.rmSync(bundle, { recursive: true, force: true });
        }
    });
});
