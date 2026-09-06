(() => {
const t = (key, fallback, ...args) => window.Localization?.t(key, fallback, ...args) ?? fallback;
window.currentPageStack = {};
const nextButton = document.getElementById('next');
const log = document.getElementById('gpl');
const progress = document.getElementById('patch-progress');
const percentLabel = document.getElementById('patch-percent');
const hasLegacyNextPatchStep = window.deltamodBackend.isCommandAvailable('npsCallback');
if (nextButton && !hasLegacyNextPatchStep) nextButton.title = 'Return to the mod list';
let pending = [];
let frame = 0;
let finished = false;
const MAX_LINES = 300;
function flush() {
    frame = 0;
    if (!log.isConnected) { pending = []; return; }
    const follow = log.scrollHeight - log.scrollTop - log.clientHeight < 48;
    const fragment = document.createDocumentFragment();
    pending.forEach(message => {
        const line = document.createElement('div');
        line.textContent = message; // Native log text is not trusted HTML.
        fragment.append(line);
    });
    pending = [];
    log.append(fragment);
    while (log.childElementCount > MAX_LINES) log.firstElementChild.remove();
    if (follow) log.scrollTop = log.scrollHeight;
}
window._onClosePage ||= [];
window._onClosePage.push(() => { cancelAnimationFrame(frame); pending = []; });
window.currentPageStack.gpl = function (obj) {
    if (!log.isConnected || finished) return;
    if (obj.log) {
        pending.push(String(obj.log).slice(0, 4096));
        if (pending.length > MAX_LINES) pending = pending.slice(-MAX_LINES);
        if (!frame) frame = requestAnimationFrame(flush);
    }
    const percent = obj.percent;
    if (typeof percent === 'number' && Number.isFinite(percent) && percent >= 0) {
        progress.value = Math.min(100, percent);
        percentLabel.textContent = `${Math.round(progress.value)}%`;
    } else {
        progress.removeAttribute('value');
        percentLabel.textContent = '';
    }
};
window.currentPageStack.next = async function () {
    if (hasLegacyNextPatchStep) {
        await window.deltamodBackend.invokeOptional('npsCallback', [], false);
        return;
    }
    await page('main');
};
window.currentPageStack.fp = async function () {
    cancelAnimationFrame(frame);
    flush();
    finished = true;
    progress.value = 100;
    percentLabel.textContent = '100%';
    const heading = document.getElementById('patchingTXT');
    heading.textContent = t('refine_patch_complete', 'Patching complete!');
    heading.classList.add('success');
    nextButton.style.display = 'block';
};
})();
