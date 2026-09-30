// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export function runNativeBootChecks(install, browserDocument) {
    const results = [];
    function assert(value, message) { if (!value) throw new Error(message); }
    function fixture(reduced = false) {
        const doc = browserDocument.implementation.createHTMLDocument('Boot fixture');
        let hidden = false;
        Object.defineProperty(doc, 'hidden', { get: () => hidden });
        doc.body.innerHTML = '<div id="deltamod-boot-root"></div><video id="theme-background-video"></video><div id="after-video"></div>';
        const video = doc.querySelector('video');
        let videoTime = 0;
        Object.defineProperty(video, 'currentTime', { get: () => videoTime, set: value => { videoTime = value; } });
        const scope = new EventTarget();
        const motion = new EventTarget();
        motion.matches = reduced;
        const timers = new Map();
        const frames = new Map();
        let time = 0;
        let sequence = 0;
        Object.assign(scope, {
            document: doc, CSS, Event, performance: { now: () => time, mark() {} },
            matchMedia: () => motion,
            requestAnimationFrame(callback) { const id = ++sequence; frames.set(id, callback); return id; },
            cancelAnimationFrame(id) { frames.delete(id); },
            setTimeout(callback, delay) { const id = ++sequence; timers.set(id, { callback, at: time + delay }); return id; },
            clearTimeout(id) { timers.delete(id); }
        });
        function advance(amount) {
            const end = time + amount;
            do {
                time = Math.min(end, time + 16);
                for (const [id, timer] of [...timers]) {
                    if (timer.at <= time && timers.delete(id)) timer.callback();
                }
                for (const [id, callback] of [...frames]) {
                    if (frames.delete(id)) callback(time);
                }
            } while (time < end);
        }
        const api = install(scope);
        return { api, scope, doc, video, timers, frames, advance, host: doc.getElementById('deltamod-boot-root'),
            hide(value) { hidden = value; doc.dispatchEvent(new Event('visibilitychange')); } };
    }
    function check(name, body) {
        try { body(); results.push({ name, passed: true }); }
        catch (error) { results.push({ name, passed: false, error: error.message }); }
    }
    check('real progress and safe text, not a cinematic delay', () => {
        const f = fixture();
        f.api.setTheme({ themeColor: '#00ff00' });
        f.api.setProgress(0.5, '<img src=x onerror=alert(1)>');
        f.advance(16);
        assert(f.doc.querySelector('.db-percent').textContent === '050%', 'real progress absent');
        assert(f.host.querySelectorAll('img').length === 0, 'status parsed as markup');
        assert(f.host.querySelectorAll('.db-progress > span').length === 32, 'progress cell count changed');
        f.api.finish();
        f.advance(16);
        assert(f.host.dataset.dismissed === 'true', 'completion waits for an animation');
        f.advance(1300);
        assert(f.host.hidden && f.host.children.length === 0, 'boot DOM retained');
        assert(f.frames.size === 0 && f.timers.size === 0, 'callbacks retained');
        f.api.fail('late');
        f.api.setTheme({ themeColor: '#ff0000' });
        assert(f.host.children.length === 0, 'late callback remounted boot');
    });
    check('video identity and explicit Chara cue survive boot', () => {
        const f = fixture();
        f.api.setTheme({ themeColor: '#ff0000', backgroundVideo: 'theme.mp4', readyAtVideoTime: 5.6 });
        assert(f.doc.querySelectorAll('video').length === 1, 'second video allocated');
        assert(f.host.contains(f.video), 'existing video not shared');
        f.api.finish();
        f.advance(100);
        assert(!f.host.dataset.dismissed, 'cue skipped');
        f.video.currentTime = 5.6;
        f.advance(64);
        assert(f.host.dataset.dismissed === 'true', 'cue did not finish boot');
        assert(f.video.parentNode === f.doc.body, 'video not restored');
        assert(f.video.nextElementSibling.id === 'after-video', 'video moved to wrong position');
        assert(f.video.currentTime === 5.6, 'playback clock reset');
        f.advance(1300);
        assert(f.frames.size + f.timers.size === 0, 'cue loop leaked');
    });
    check('failed media has a bounded cue wait without inventing readiness', () => {
        const f = fixture();
        f.api.setTheme({ backgroundVideo: 'broken.mp4', readyAtVideoTime: 5.6 });
        f.api.finish();
        f.hide(true);
        f.advance(7200);
        assert(f.host.dataset.dismissed === 'true', 'failed media stranded startup');
        f.advance(1300);
        assert(f.frames.size + f.timers.size === 0, 'hidden-media timers leaked');
    });
    check('assembled idle sprite stops scheduling animation', () => {
        const f = fixture();
        f.api.setTheme({ themeColor: '#ffffff' });
        f.advance(2000);
        assert(f.frames.size + f.timers.size === 0, 'idle boot still runs every frame');
        const canvas = f.doc.querySelector('canvas');
        const data = canvas.getContext('2d').getImageData(0, 0, 32, 32).data;
        assert(data.some((value, index) => index % 4 === 3 && value > 0), 'sprite is blank');
        f.scope.dispatchEvent(new Event('pagehide'));
        assert(f.frames.size + f.timers.size === 0, 'pagehide retained callbacks');
    });
    check('reduced motion paints once and finishes without an exit wait', () => {
        const f = fixture(true);
        f.api.setTheme({ themeColor: '#ffffff' });
        f.advance(16);
        assert(f.frames.size + f.timers.size === 0, 'reduced motion is animated');
        f.api.finish();
        f.advance(32);
        assert(f.host.hidden && f.host.children.length === 0, 'reduced-motion exit is delayed');
    });
    check('progress bursts never accumulate animation wakeups', () => {
        const f = fixture();
        f.api.setTheme({ themeColor: '#ffffff' });
        for (let index = 0; index < 12; index += 1) {
            f.api.setProgress(index / 20);
            f.advance(16);
            assert(f.timers.size <= 1, 'progress updates accumulate timers');
        }
        f.api.finish();
        f.advance(1400);
        assert(f.frames.size + f.timers.size === 0, 'progress timers survive dismissal');
    });
    check('finish without a theme does not leave an invisible overlay', () => {
        const f = fixture(true);
        f.api.finish();
        f.advance(32);
        assert(f.host.hidden, 'theme-free startup stranded');
    });
    return { passed: results.every(result => result.passed), checks: results };
}
