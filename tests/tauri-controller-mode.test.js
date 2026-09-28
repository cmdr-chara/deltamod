const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const { spawnSync } = require('node:child_process');

const root = path.join(__dirname, '..');

describe('Tauri Controller Mode packaging', () => {
    it('packages the pinned Windows utility at its fixed resource path', () => {
        const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.windows.conf.json')));
        const utility = fs.readFileSync(path.join(root, 'tools', 'cmodeutil.exe'));

        expect(config.bundle.resources['../tools/cmodeutil.exe']).toBe('tools/cmodeutil.exe');
        expect(crypto.createHash('sha256').update(utility).digest('hex').toUpperCase())
            .toBe('04ACDBB53C96CD99B01FE53A0297AC06308DDAD14B5253A3AF4F9A319985AA45');
    });

    it('keeps Windows resources when CI disables diagnostic signing', () => {
        const workflow = fs.readFileSync(path.join(root, '.github/workflows/ci.yml'), 'utf8');
        const commands = [...workflow.matchAll(/node -e "([^"\r\n]*c\.bundle\.createUpdaterArtifacts = false;[^"\r\n]*)"/g)];
        expect(commands).toHaveLength(1);
        const temporary = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-ci-config-'));
        try {
            fs.mkdirSync(path.join(temporary, 'src-tauri'));
            const original = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri/tauri.windows.conf.json'), 'utf8'));
            const target = path.join(temporary, 'src-tauri/tauri.windows.conf.json');
            fs.writeFileSync(target, JSON.stringify(original));
            const result = spawnSync(process.execPath, ['-e', commands[0][1]], {
                cwd: temporary, encoding: 'utf8', shell: false
            });
            expect(result.error).toBeUndefined();
            expect(result.status).toBe(0);
            original.bundle.createUpdaterArtifacts = false;
            expect(JSON.parse(fs.readFileSync(target, 'utf8'))).toEqual(original);
        } finally {
            fs.rmSync(temporary, { recursive: true, force: true });
        }
    });
});
