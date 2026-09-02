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

    it('restores each track only after its own metadata has loaded', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const switchSource = renderer.slice(
            renderer.indexOf('function switchMenuAudioSource'),
            renderer.indexOf('function loopMenuAudio')
        );

        expect(switchSource).toContain('rememberMenuAudioPosition();');
        expect(switchSource).toContain("audio.addEventListener('loadedmetadata', restorePosition, { once: true });");
        expect(switchSource).toContain('audio.load();');
        expect(switchSource.indexOf("audio.addEventListener('loadedmetadata'"))
            .toBeLessThan(switchSource.indexOf('audio.src = source;'));
        expect(switchSource).not.toContain('if (audio.readyState >= 1)');
    });

    it('does not move the entire theme background', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'index.css'), 'utf8');

        expect(renderer).toContain('delete background.dataset.themeMotion;');
        expect(renderer).not.toContain("const motionVariants = ['drift-left', 'drift-right', 'breathe', 'rise'];");
        expect(css).not.toContain('@keyframes theme-background-breathe');
    });

    it('keeps the Roaring Knight loop free from synthetic horizontal tear lines', () => {
        const generator = fs.readFileSync(
            path.join(projectRoot, 'scripts', 'generate-bundled-game-themes.ps1'),
            'utf8'
        );

        expect(generator).not.toContain('$tearPen');
        expect(generator).not.toContain('Sparse horizontal tears');
    });

    it('uses only the custom sidebar tooltip instead of stacking a native title tooltip', () => {
        const shell = fs.readFileSync(path.join(projectRoot, 'web', 'index.html'), 'utf8');
        const sidebar = shell.slice(
            shell.indexOf('<div class="sidebar">'),
            shell.indexOf('<button\n        id="language-wheel-toggle"')
        );

        expect(sidebar).toContain('aria-label="Options"');
        expect(sidebar).not.toMatch(/class="sidebar-button"[^>]*\stitle=/);
    });

    it('keeps UNDERTALE location themes fullscreen and free from synthetic overlays', () => {
        const themeFiles = [
            'undertale-ruins.theme.json',
            'undertale-snowdin.theme.json',
            'undertale-waterfall.theme.json',
            'undertale-void.theme.json',
            'undertale-hotland.theme.json',
            'undertale-core.theme.json',
            'undertale-true-lab.theme.json',
            'undertale-new-home.theme.json',
            'undertale.theme.json'
        ];

        themeFiles.forEach(themeFile => {
            const theme = JSON.parse(fs.readFileSync(
                path.join(projectRoot, 'web', 'themes', 'data', themeFile),
                'utf8'
            ));
            expect(theme.backgroundSize).toBe('cover');
            expect(theme.animatedEnvironment).toBeUndefined();
            expect(theme.animatedSprites).toBeUndefined();
        });

        const barrier = JSON.parse(fs.readFileSync(
            path.join(projectRoot, 'web', 'themes', 'data', 'undertale-void.theme.json'),
            'utf8'
        ));
        expect(barrier.backgroundSize).toBe('cover');
        expect(barrier.backgroundPosition).toBe('center bottom');
    });

    it('shortens the Roaring Knight post-slash exit by twenty-five percent', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const css = fs.readFileSync(path.join(projectRoot, 'web', 'index.css'), 'utf8');

        expect(renderer).toContain('}, 1575);');
        expect(renderer).toContain('setTimeout(clearThemeTransition, 2100)');
        expect(css).toContain('transition-duration: 490ms;');
    });

    it('persists the Roaring Knight awakening and skips repeat transitions', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const preparation = renderer.slice(
            renderer.indexOf('async function prepareThemeTransition'),
            renderer.indexOf('function completeRoaringKnightAwakening')
        );
        const completion = renderer.slice(
            renderer.indexOf('function completeRoaringKnightAwakening'),
            renderer.indexOf('function playRoaringKnightTransition')
        );
        const transition = renderer.slice(
            renderer.indexOf('function playRoaringKnightTransition'),
            renderer.indexOf('function synchronizeThemeTransition')
        );

        expect(renderer).toContain("const ROARING_KNIGHT_AWAKENED_FLAG = 'ROARING_KNIGHT_AWAKENED';");
        expect(preparation).toContain("invoke('getUniqueFlag', [ROARING_KNIGHT_AWAKENED_FLAG])");
        expect(completion).toContain("invoke('setUniqueFlag', [ROARING_KNIGHT_AWAKENED_FLAG, true])");
        expect(completion).toContain('themeTransitionCompleted = true;');
        expect(transition).toContain('if (themeTransitionCompleted) return;');
        expect(renderer).toContain('await prepareThemeTransition(theme);');
        expect(renderer).toContain('if (themeTransitionCompleted) {\n        applyRoaringKnightPalette(true);');
        expect(renderer).toContain('bootTheme(activeThemeVisualConfig());');
        expect(renderer).toContain('setRoaringKnightSpriteSet(false);');
    });

    it('pauses menu music while the application is in the background', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const suspend = renderer.slice(
            renderer.indexOf('function suspendApplicationMedia()'),
            renderer.indexOf('async function resumeApplicationMedia()')
        );
        const resume = renderer.slice(
            renderer.indexOf('async function resumeApplicationMedia()'),
            renderer.indexOf("window.addEventListener('blur'")
        );

        expect(suspend).toContain('rememberMenuAudioPosition();');
        expect(suspend).toContain('audio.pause();');
        expect(resume).toContain('menuAudioWasPlayingBeforeWindowInactive');
        expect(resume).toContain('await audio.play().catch(() => {});');
        expect(renderer).toContain("document.addEventListener('visibilitychange'");
    });

    it('preserves the loaded background video and playback position during standby', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const suspend = renderer.slice(
            renderer.indexOf('function suspendThemeBackgroundVideo()'),
            renderer.indexOf('function resumeThemeBackgroundVideo')
        );
        const resume = renderer.slice(
            renderer.indexOf('function resumeThemeBackgroundVideo'),
            renderer.indexOf('function scheduleThemeVideoSuspension')
        );

        expect(suspend).toContain('currentTime: Number.isFinite(video.currentTime)');
        expect(suspend).not.toContain("removeAttribute('src')");
        expect(suspend).not.toContain('video.load()');
        expect(resume).toContain('video.currentTime = targetTime');
        expect(resume).not.toContain('video.src = expectedSource');
        expect(resume).not.toContain('video.load()');
    });

    it('uses bundled and layered Roaring Knight movement and attack sounds', () => {
        const renderer = fs.readFileSync(path.join(projectRoot, 'web', 'index.js'), 'utf8');
        const transitionSounds = renderer.slice(
            renderer.indexOf('const ROARING_KNIGHT_SFX'),
            renderer.indexOf('const ROARING_KNIGHT_MONOCHROME_SPRITES')
        );
        const sequence = renderer.slice(
            renderer.indexOf('async function playRoaringKnightSoundSequence'),
            renderer.indexOf('function activeThemeVisualConfig')
        );

        expect(transitionSounds).toContain('knight-move.wav');
        expect(transitionSounds).toContain('knight-impact.wav');
        expect(sequence.match(/ROARING_KNIGHT_SFX\.move/g)).toHaveLength(3);
        expect(sequence.match(/ROARING_KNIGHT_SFX\.slash/g)).toHaveLength(3);
        expect(sequence).toContain('ROARING_KNIGHT_SFX.impact');
        expect(sequence).toContain('ROARING_KNIGHT_SFX.damage');
        expect(renderer).toContain('void playRoaringKnightSoundSequence();');

        expect(fs.existsSync(path.join(
            projectRoot,
            'web',
            'themes',
            'sfx',
            'the-knight-transition',
            'knight-move.wav'
        ))).toBe(true);
        expect(fs.existsSync(path.join(
            projectRoot,
            'web',
            'themes',
            'sfx',
            'the-knight-transition',
            'knight-impact.wav'
        ))).toBe(true);
    });
});
