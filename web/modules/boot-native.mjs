// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export default function installNativeBoot(scope) {
    'use strict';
    const doc = scope.document;
    const host = doc?.getElementById('deltamod-boot-root');
    if (!host || scope.DeltamodBoot) return scope.DeltamodBoot;

    // Same 32x32 sprite as AnimatedPixelHeart.tsx. No second image or React root.
    const rows = [
        '................................', '................................',
        '......######........######......', '....##......##....##......##....',
        '...#..........#..#..........#...', '..#............##............#..',
        '..#..........................#..', '.#..#......................#..#.',
        '.#...###.................##...#.', '.#..######............######..#.',
        '.#.....####...####...####.....#.', '.#....######.######.######....#.',
        '.#......####.######.####......#.', '..#.......##..####..##.......#..',
        '..#............##............#..', '...#........................#...',
        '...#.......#........#.......#...', '....#.....###......###.....#....',
        '....#....#####....#####....#....', '.....#....................#.....',
        '......#..................#......', '.......#.....######.....#.......',
        '........#.....####.....#........', '.........#.....##.....#.........',
        '..........#..........#..........', '...........#........#...........',
        '............#......#............', '.............#....#.............',
        '..............#..#..............', '...............##...............',
        '................................', '................................'
    ];
    const now = () => scope.performance.now();
    const motion = scope.matchMedia('(prefers-reduced-motion: reduce)');
    let section = null;
    let canvas = null;
    let context = null;
    let statusNode = null;
    let percentNode = null;
    let cells = [];
    let frame = null;
    let wakeTimer = null;
    let cueTimer = null;
    let exitTimer = null;
    let video = null;
    let videoAnchor = null;
    let startedAt = now();
    let previousSprite = -1;
    let previousPercent = -1;
    let previousCells = -1;
    let previousStatus = '';
    let progress = 0;
    let status = 'Starting local runtime';
    let color = '#ffffff';
    let cue = null;
    let finished = false;
    let dismissed = false;
    let disposed = false;

    function clearScheduled() {
        if (frame !== null) scope.cancelAnimationFrame(frame);
        if (wakeTimer !== null) scope.clearTimeout(wakeTimer);
        if (cueTimer !== null) scope.clearTimeout(cueTimer);
        frame = wakeTimer = cueTimer = null;
    }

    function restoreVideo() {
        if (video) video.classList.remove('db-shared-video');
        if (video && videoAnchor?.parentNode) {
            videoAnchor.parentNode.insertBefore(video, videoAnchor);
        }
        videoAnchor?.remove();
        video = videoAnchor = null;
    }

    function attachVideo(enabled) {
        if (!enabled) return restoreVideo();
        const current = doc.getElementById('theme-background-video');
        if (!current || current === video || !section) return;
        restoreVideo();
        video = current;
        videoAnchor = doc.createComment('theme-video-home');
        current.parentNode.insertBefore(videoAnchor, current);
        current.classList.add('db-shared-video');
        section.querySelector('.db-scene-base').appendChild(current);
        // The renderer still owns source, playback, audio, poster and mode.
        // Moving the existing node preserves its decoder and its cue clock.
    }

    function mount() {
        if (section || disposed) return;
        host.hidden = false;
        host.removeAttribute('aria-hidden');
        section = doc.createElement('section');
        section.className = 'deltamod-boot deltamod-boot-native';
        section.setAttribute('aria-label', 'Deltamod loading screen');
        Object.assign(section.dataset, {
            phase: 'load', autoplay: 'on', lock: 'off', glitch: 'off',
            tick: 'off', video: 'off', version: 'COMMUNITY BUILD'
        });
        // Only constant markup enters innerHTML. Theme and status are DOM values.
        section.innerHTML = '<div class="db-scene-base" aria-hidden="true"></div>'
            + '<div class="db-backdrop" aria-hidden="true"></div>'
            + '<div class="db-scene-echo" aria-hidden="true">'
            + '<span class="db-scene-slice db-scene-slice-a"></span>'
            + '<span class="db-scene-slice db-scene-slice-b"></span>'
            + '<span class="db-scene-slice db-scene-slice-c"></span></div>'
            + '<div class="db-curtain db-curtain-top" aria-hidden="true"></div>'
            + '<div class="db-curtain db-curtain-bottom" aria-hidden="true"></div>'
            + '<main class="db-main"><h1 class="db-visually-hidden">Starting Deltamod</h1>'
            + '<div class="db-sequence"><div class="db-emblem-stage">'
            + '<canvas class="db-native-heart" width="32" height="32" aria-hidden="true"></canvas>'
            + '</div><div class="db-loading"><div class="db-status-line">'
            + '<span class="db-status" aria-live="polite"></span>'
            + '<span class="db-percent">000%</span></div>'
            + '<div class="db-progress" aria-hidden="true">'
            + '<span data-state="off"></span>'.repeat(32)
            + '</div></div></div></main>';
        host.replaceChildren(section);
        canvas = section.querySelector('.db-native-heart');
        context = canvas.getContext('2d', { alpha: true });
        statusNode = section.querySelector('.db-status');
        percentNode = section.querySelector('.db-percent');
        cells = Array.from(section.querySelectorAll('.db-progress > span'));
        startedAt = now();
    }

    function drawSprite(bucket) {
        if (!context || bucket === previousSprite) return;
        previousSprite = bucket;
        context.clearRect(0, 0, 32, 32);
        context.fillStyle = color;
        rows.forEach((row, y) => {
            for (let x = 0; x < row.length; x += 1) {
                if (row[x] !== '#') continue;
                // Symmetric center-out assembly, ending on the exact sprite.
                const distance = Math.abs(x - 15.5) + Math.abs(y - 15.5);
                if (bucket === 32 || distance <= bucket) context.fillRect(x, y, 1, 1);
            }
        });
    }

    function cueReached() {
        if (cue === null) return true;
        const activeVideo = doc.getElementById('theme-background-video');
        return (activeVideo && activeVideo.currentTime >= cue)
            || now() - startedAt >= (cue + 1.5) * 1000;
    }

    function dismiss() {
        if (dismissed || disposed) return;
        dismissed = true;
        clearScheduled();
        drawSprite(32);
        section.dataset.phase = 'ready';
        section.dataset.lock = 'settled';
        section.dataset.glitch = 'off';
        restoreVideo();
        host.dataset.dismissed = 'true';
        host.setAttribute('aria-hidden', 'true');
        doc.body.classList.remove('deltamod-boot-active');
        doc.body.classList.add('deltamod-ui-entering');
        scope.performance.mark?.('deltamod-boot-dismissed');
        scope.dispatchEvent(new scope.Event('deltamod-boot-dismissed'));
        exitTimer = scope.setTimeout(() => {
            exitTimer = null;
            host.replaceChildren();
            host.hidden = true;
            doc.body.classList.remove('deltamod-ui-entering');
            dispose();
        }, motion.matches ? 0 : 1250);
    }

    function render() {
        frame = null;
        // A progress-triggered frame supersedes an older animation wakeup.
        if (wakeTimer !== null) scope.clearTimeout(wakeTimer);
        wakeTimer = null;
        if (disposed || dismissed || !section) return;
        const ready = finished && cueReached();
        const percent = Math.round(progress * 100);
        const lit = Math.round(progress * cells.length);
        const text = ready ? 'Ready' : status;
        if (percent !== previousPercent) {
            percentNode.textContent = `${String(percent).padStart(3, '0')}%`;
            previousPercent = percent;
        }
        if (lit !== previousCells || ready) {
            cells.forEach((cell, index) => {
                const next = index < lit ? (index === lit - 1 && !ready ? 'head' : 'on') : 'off';
                if (cell.dataset.state !== next) cell.dataset.state = next;
            });
            previousCells = lit;
        }
        if (text !== previousStatus) {
            statusNode.textContent = text;
            previousStatus = text;
        }
        const elapsed = now() - startedAt;
        const bucket = ready || motion.matches ? 32 : Math.min(32, Math.floor(elapsed / 45));
        drawSprite(bucket);
        const glitch = !motion.matches && elapsed > 550 && elapsed < 640 ? 'a' : 'off';
        if (section.dataset.glitch !== glitch) section.dataset.glitch = glitch;
        if (ready) return dismiss();
        // No idle 60 Hz loop: only animate assembly or an explicit video cue.
        if (!doc.hidden && ((!motion.matches && bucket < 32) || (finished && cue !== null))) {
            wakeTimer = scope.setTimeout(() => {
                wakeTimer = null;
                schedule();
            }, 32);
        }
    }

    function schedule() {
        if (disposed || dismissed || frame !== null || doc.hidden) return;
        frame = scope.requestAnimationFrame(render);
    }

    function onVisibility() {
        if (doc.hidden) {
            if (frame !== null) scope.cancelAnimationFrame(frame);
            if (wakeTimer !== null) scope.clearTimeout(wakeTimer);
            frame = wakeTimer = null;
        } else schedule();
    }

    function dispose() {
        if (disposed) return;
        disposed = true;
        clearScheduled();
        if (exitTimer !== null) scope.clearTimeout(exitTimer);
        exitTimer = null;
        restoreVideo();
        doc.removeEventListener('visibilitychange', onVisibility);
        scope.removeEventListener('pagehide', dispose);
        motion.removeEventListener?.('change', schedule);
        context = canvas = section = statusNode = percentNode = null;
        cells = [];
    }

    function finish(message) {
        if (finished || dismissed || disposed) return;
        mount();
        finished = true;
        progress = 1;
        status = message;
        // A failed or hidden video must not strand startup. This is a maximum
        // cue wait, not an artificial minimum duration or an earlier ready marker.
        if (cue !== null) {
            cueTimer = scope.setTimeout(() => {
                cueTimer = null;
                if (!disposed && !dismissed) {
                    if (frame !== null) scope.cancelAnimationFrame(frame);
                    frame = null;
                    render();
                }
            }, Math.max(0, (cue + 1.5) * 1000 - (now() - startedAt)));
        }
        schedule();
    }

    const api = Object.freeze({
        setProgress(value, message) {
            if (finished || dismissed || disposed) return;
            const numeric = Number(value);
            if (Number.isFinite(numeric)) progress = Math.max(progress, Math.min(1, Math.max(0, numeric)));
            if (typeof message === 'string' && message.trim()) status = message;
            schedule();
        },
        setTheme(theme = {}) {
            if (finished || dismissed || disposed) return;
            mount();
            const accent = theme.soulColor || theme.themeColor || '#ffffff';
            color = typeof accent === 'string' && scope.CSS?.supports('color', accent) ? accent : '#ffffff';
            section.style.setProperty('--db-theme-color', color);
            section.style.setProperty('--db-soul-color', color);
            section.style.setProperty('--db-background-image', typeof theme.backgroundImage === 'string'
                ? `url(${JSON.stringify(theme.backgroundImage)})` : 'none');
            section.dataset.video = theme.backgroundVideo ? 'on' : 'off';
            const time = Number(theme.readyAtVideoTime);
            cue = theme.readyAtVideoTime != null && Number.isFinite(time) ? Math.max(0, time) : null;
            previousSprite = -1;
            attachVideo(Boolean(theme.backgroundVideo));
            schedule();
        },
        finish() { finish('Opening your session'); },
        fail(message = 'Continuing') { finish(message); }
    });
    scope.DeltamodBoot = api;
    doc.body.classList.add('deltamod-boot-active');
    host.hidden = false;
    doc.addEventListener('visibilitychange', onVisibility);
    scope.addEventListener('pagehide', dispose, { once: true });
    motion.addEventListener?.('change', schedule);
    return api;
}
