const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const source = fs.readFileSync(path.join(__dirname, '../web/index.js'), 'utf8');
function harness() {
    let now = 0;
    let tick;
    const context = {
        theme: { transitionEffect: 'knight', transitionCueTime: 26, mainSong: 'knight.mp3' },
        ROARING_KNIGHT_TRANSITION: { id: 'knight' },
        themeTransitionCompleted: false, themeTransitionLastAudioTime: null,
        currentAudioSource: 'credits.mp3',
        audio: { currentTime: 0, paused: true, error: null },
        document: { hidden: false, hasFocus: () => true },
        performance: { now: () => now },
        setInterval: callback => { tick = callback; return 1; },
        themeAssetUrl: (theme, folder, file) => file,
        playRoaringKnightTransition: vi.fn(),
        clearThemeTransition: vi.fn(), applyRoaringKnightPalette: vi.fn()
    };
    vm.createContext(context);
    vm.runInContext(source.slice(source.indexOf('function watchRoaringKnightCue('), source.indexOf('function replayRoaringKnightTransition(')), context);
    vm.runInContext(source.slice(source.indexOf('function synchronizeThemeTransition('), source.indexOf('function setThemeVideoAudioEnabled(')), context);
    context.watchRoaringKnightCue(context.theme);
    return { context, advance: seconds => { for (let i = 0; i < seconds * 4; i++) { now += 250; tick(); } } };
}
it.each(['credits.mp3', ''])('awakens without theme audio (%s) after foreground cue time', track => {
    const {context, advance} = harness();
    context.currentAudioSource = track;
    advance(25);
    expect(context.playRoaringKnightTransition).not.toHaveBeenCalled();
    advance(1);
    expect(context.playRoaringKnightTransition).toHaveBeenCalledOnce();
});
it('keeps the awakening synchronized to a playing soundtrack', () => {
    const {context, advance} = harness();
    context.currentAudioSource = 'knight.mp3'; context.audio.paused = false;
    context.audio.currentTime = 10;
    advance(30);
    expect(context.playRoaringKnightTransition).not.toHaveBeenCalled();
    context.audio.currentTime = 26;
    advance(.25);
    expect(context.playRoaringKnightTransition).toHaveBeenCalledOnce();
});
it('does not lose the cue when focus returns after the crossing', () => {
    const {context} = harness();
    context.currentAudioSource = 'knight.mp3'; context.audio.currentTime = 25;
    context.synchronizeThemeTransition();
    context.document.hasFocus = () => false; context.audio.currentTime = 27;
    context.synchronizeThemeTransition();
    expect(context.playRoaringKnightTransition).not.toHaveBeenCalled();
    context.document.hasFocus = () => true;
    context.synchronizeThemeTransition();
    expect(context.playRoaringKnightTransition).toHaveBeenCalledOnce();
});
it('does not spend fallback cue time while hidden', () => {
    const {context, advance} = harness();
    context.document.hidden = true; advance(60);
    expect(context.playRoaringKnightTransition).not.toHaveBeenCalled();
    context.document.hidden = false; advance(26);
    expect(context.playRoaringKnightTransition).toHaveBeenCalledOnce();
});
it('does not automatically replay a completed soundtrack cue', () => {
    const {context} = harness();
    context.currentAudioSource = 'knight.mp3'; context.audio.currentTime = 30;
    context.themeTransitionCompleted = true;
    context.synchronizeThemeTransition();
    expect(context.playRoaringKnightTransition).not.toHaveBeenCalled();
});
