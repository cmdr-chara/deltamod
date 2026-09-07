const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '../..');
const source = fs.readFileSync(path.join(root, 'web/index.js'), 'utf8');
async function setup(page) {
    await page.setViewportSize({width: 1280, height: 800});
    await page.setContent('<button class="theme-replay-event">Replay awakening</button>');
    await page.addStyleTag({path: path.join(root, 'web/index.css')});
    const assets = Object.fromEntries(fs.readdirSync(path.join(root,'web/themes/img/the-knight-transition')).map(name => [name, 'data:image/png;base64,' + fs.readFileSync(path.join(root,'web/themes/img/the-knight-transition',name)).toString('base64')]));
    await page.evaluate(assets => {
        window.theme = {transitionEffect: 'roaring-knight-monochrome-awakening'};
        window.themeAssetUrl = (theme, kind, file) => assets[file.split('/').pop()];
        window.applyThemeStyles = () => {};
        window.deltamodBackend = {invoke: async () => false};
        window.__completed = 0;
    },assets);
    await page.addScriptTag({content: source.slice(0, source.indexOf('var themeTransitionGeneration = 0;') + 'var themeTransitionGeneration = 0;'.length).split('\n').filter(line => /^var themeTransition/.test(line)).join('\n') + '\n' + source.slice(source.indexOf('const ROARING_KNIGHT_TRANSITION'), source.indexOf('function setThemeVideoAudioEnabled'))});
    await page.evaluate(() => {
        window.completeRoaringKnightAwakening = () => { themeTransitionCompleted = true; window.__completed++; };
    });
}
test('awakening stages three cuts and cleans up after revealing', async ({page}, info) => {
    await setup(page);
    await page.clock.install();
    await page.evaluate(() => playRoaringKnightTransition());
    await page.clock.runFor(2800);
    await expect(page.locator('.theme-transition-fountain span')).toHaveCount(16);
    await page.evaluate(() => document.getAnimations().forEach(animation => { animation.currentTime = 2800; animation.pause(); }));
    await page.screenshot({path: info.outputPath('knight-arrival.png')});
    await page.clock.runFor(1450);
    await expect(page.locator('.theme-transition-rift.is-cut')).toHaveCount(3);
    await page.screenshot({path: info.outputPath('knight-three-cuts.png')});
    await page.clock.runFor(1200);
    expect(await page.evaluate(() => window.__completed)).toBe(1);
    await page.clock.runFor(1400);
    await expect(page.locator('#theme-transition-overlay')).toHaveCount(0);
});
test('Escape cancels every phase and replay creates only one event', async ({page}) => {
    await setup(page);
    await page.clock.install();
    await page.evaluate(() => { playRoaringKnightTransition(); playRoaringKnightTransition(); });
    await expect(page.locator('#theme-transition-overlay')).toHaveCount(1);
    await page.keyboard.press('Escape');
    await page.clock.runFor(8000);
    await expect(page.locator('#theme-transition-overlay')).toHaveCount(0);
    expect(await page.evaluate(() => window.__completed)).toBe(1);
    expect(await page.evaluate(() => themeTransitionPhaseTimers.length)).toBe(0);
});
test('reduced motion completes immediately without the event', async ({page}) => {
    await setup(page);
    await page.emulateMedia({reducedMotion: 'reduce'});
    await page.evaluate(() => playRoaringKnightTransition());
    await expect(page.locator('#theme-transition-overlay')).toHaveCount(0);
    expect(await page.evaluate(() => window.__completed)).toBe(1);
});
test('background decodes through three consecutive explicit loops', async ({page}) => {
    await page.setContent('<video muted></video>');
    const data = fs.readFileSync(path.join(root,'web/themes/video/the-knight.webm')).toString('base64');
    await page.evaluate(data => {
        const video = document.querySelector('video');
        video.src = 'data:video/webm;base64,' + data;
        video.dataset.source = 'knight';
        window.getThemeBackgroundVideo = () => video;
        window.fallBackFromThemeVideo = error => { window.__error = String(error); };
        window.__loops = 0;
        document.hasFocus = () => true;
    },data);
    await page.addScriptTag({content: source.slice(source.indexOf('function loopThemeBackgroundVideo'),source.indexOf('function applyThemeBackground'))});
    await page.evaluate(async () => {
        const video = document.querySelector('video');
        video.onended = () => { window.__loops++; loopThemeBackgroundVideo(); };
        await video.play();
    });
    await expect.poll(() => page.evaluate(() => window.__loops), {timeout: 42000}).toBeGreaterThanOrEqual(3);
    expect(await page.evaluate(() => window.__error)).toBeUndefined();
    expect(await page.evaluate(() => document.querySelector('video').paused)).toBe(false);
});
