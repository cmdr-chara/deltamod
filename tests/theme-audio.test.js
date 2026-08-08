// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');

const projectRoot = path.join(__dirname, '..');

describe('theme audio playback', () => {
    it('restores ANOTHER HIM to the original 48 second tempo', () => {
        const theme = JSON.parse(fs.readFileSync(
            path.join(projectRoot, 'web', 'themes', 'data', 'anotherhim.theme.json'),
            'utf8'
        ));

        expect(theme.musicPlaybackRate).toBe(1.25);
        expect(theme.musicPreservesPitch).toBe(false);
    });

    it('resets playback settings for non-theme music', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');

        expect(renderer).toContain('configureMenuAudioPlayback(source);');
        expect(renderer).toContain("const configuredRate = isThemeTrack ? Number(theme?.musicPlaybackRate) : 1;");
        expect(renderer).toContain("theme?.musicPreservesPitch !== false");
    });
});
