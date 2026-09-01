// Copyright © 2026 cmdr-chara. Licensed under the EUPL 1.2.

(function exposeCharaEncounterSession(root, factory) {
    const api = factory();
    if (typeof module === 'object' && module.exports) module.exports = api;
    if (root && !root.DeltamodCharaEncounterSession) {
        root.DeltamodCharaEncounterSession = api;
    }
})(typeof globalThis === 'object' ? globalThis : this, () => {
    function createSessionGate() {
        let activeToken = null;

        return Object.freeze({
            begin() {
                if (activeToken !== null) return null;
                activeToken = Symbol('chara-encounter');
                return activeToken;
            },
            isCurrent(token) {
                return token !== null && token === activeToken;
            },
            cancel(token) {
                if (token === null || token !== activeToken) return false;
                activeToken = null;
                return true;
            }
        });
    }

    return Object.freeze({ createSessionGate });
});
