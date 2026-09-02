const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

describe('stable release note rendering', () => {
    test('folds wrapped changelog bullets into one clean release-note bullet', () => {
        const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-release-notes-'));
        try {
            const changelog = path.join(root, 'CHANGELOG.txt');
            const output = path.join(root, 'release-notes.md');
            fs.writeFileSync(changelog, [
                '# Deltamod Community 2.0.18',
                '',
                '## Highlights',
                '',
                '- Add transactional install, update, and repair,',
                '  while preserving the previous usable generation.',
                ''
            ].join('\n'));
            const result = spawnSync(process.execPath, [
                path.join(__dirname, '..', 'scripts', 'render-release-notes.js'),
                'community-v2.0.18',
                changelog,
                output
            ], { encoding: 'utf8' });

            expect(result.status, result.stderr).toBe(0);
            const rendered = fs.readFileSync(output, 'utf8');
            expect(rendered).toContain(
                '- Add transactional install, update, and repair, while preserving the previous usable generation.'
            );
            expect(rendered).not.toContain(',.');
        } finally {
            fs.rmSync(root, { recursive: true, force: true });
        }
    });
});
