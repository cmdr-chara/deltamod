const ZOOM_FACTORS = Object.freeze([
    0.75,
    0.8,
    0.9,
    1,
    1.1,
    1.25,
    1.5,
    1.75,
    2
]);

function getZoomCommand(input) {
    if (
        !input
        || input.type !== 'keyDown'
        || (!input.control && !input.meta)
        || input.alt
    ) {
        return null;
    }

    if (input.key === '+' || input.key === '=' || input.code === 'NumpadAdd') {
        return 'in';
    }
    if (input.key === '-' || input.code === 'NumpadSubtract') {
        return 'out';
    }
    if (input.key === '0' || input.code === 'Numpad0') {
        return 'reset';
    }
    return null;
}

function getNextZoomFactor(currentFactor, direction) {
    if (direction === 'reset') return 1;

    const current = Number.isFinite(currentFactor) ? currentFactor : 1;
    const epsilon = 0.001;

    if (direction === 'in') {
        return ZOOM_FACTORS.find(factor => factor > current + epsilon)
            ?? ZOOM_FACTORS.at(-1);
    }

    if (direction === 'out') {
        return [...ZOOM_FACTORS].reverse().find(factor => factor < current - epsilon)
            ?? ZOOM_FACTORS[0];
    }

    return current;
}

function registerWindowZoomShortcuts(window) {
    let zoomFactor = window.webContents.getZoomFactor();

    window.webContents.on('before-input-event', (event, input) => {
        const command = getZoomCommand(input);
        if (!command) return;

        event.preventDefault();
        zoomFactor = getNextZoomFactor(zoomFactor, command);
        window.webContents.setZoomFactor(zoomFactor);
    });

    window.webContents.on('did-finish-load', () => {
        window.webContents.setZoomFactor(zoomFactor);
    });
}

module.exports = {
    ZOOM_FACTORS,
    getZoomCommand,
    getNextZoomFactor,
    registerWindowZoomShortcuts
};
