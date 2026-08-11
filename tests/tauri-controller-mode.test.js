const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');

describe('Tauri Controller Mode packaging', () => {
    it('packages the pinned Windows utility at its fixed resource path', () => {
        const config = JSON.parse(fs.readFileSync(path.join(root, 'src-tauri', 'tauri.conf.json')));
        const utility = fs.readFileSync(path.join(root, 'tools', 'cmodeutil.exe'));

        expect(config.bundle.resources['../tools/cmodeutil.exe']).toBe('tools/cmodeutil.exe');
        expect(crypto.createHash('sha256').update(utility).digest('hex').toUpperCase())
            .toBe('04ACDBB53C96CD99B01FE53A0297AC06308DDAD14B5253A3AF4F9A319985AA45');
    });
});
