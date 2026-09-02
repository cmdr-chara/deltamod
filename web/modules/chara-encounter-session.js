// Copyright © 2026 cmdr-chara. Licensed under the EUPL 1.2.

(function exposeCharaEncounterSession(root, factory) {
    const api = factory(root);
    if (typeof module === 'object' && module.exports) module.exports = api;
    if (root && !root.DeltamodCharaEncounterSession) {
        root.DeltamodCharaEncounterSession = api;
    }
})(typeof globalThis === 'object' ? globalThis : this, root => {
    function handOffFocusToReplacement(element) {
        const document = element?.ownerDocument;
        const elementId = element?.id;
        const MutationObserverClass = root?.MutationObserver;
        const setTimeoutFn = root?.setTimeout?.bind(root);
        const clearTimeoutFn = root?.clearTimeout?.bind(root);
        const observationRoot = document?.documentElement || document?.body;
        if (
            !document
            || !elementId
            || !observationRoot
            || typeof MutationObserverClass !== 'function'
            || typeof setTimeoutFn !== 'function'
        ) {
            return;
        }

        let observer = null;
        let timeout = null;
        let settled = false;
        const finish = () => {
            if (settled) return;
            settled = true;
            observer?.disconnect();
            if (timeout !== null && typeof clearTimeoutFn === 'function') {
                clearTimeoutFn(timeout);
            }
        };
        const restoreReplacement = () => {
            if (settled || element.isConnected) return false;
            const replacement = document.getElementById(elementId);
            if (!replacement || replacement === element) return false;
            replacement.focus?.();
            finish();
            return true;
        };

        observer = new MutationObserverClass(() => restoreReplacement());
        observer.observe(observationRoot, { childList: true, subtree: true });
        timeout = setTimeoutFn(finish, 1500);
        restoreReplacement();
    }

    function createSessionGate() {
        let activeToken = null;
        let focusOrigin = null;

        return Object.freeze({
            begin() {
                if (activeToken !== null) return null;
                activeToken = Symbol('chara-encounter');
                const activeElement = root?.document?.activeElement;
                focusOrigin = activeElement && activeElement !== root?.document?.body
                    ? activeElement
                    : null;
                return activeToken;
            },
            isCurrent(token) {
                return token !== null && token === activeToken;
            },
            cancel(token) {
                if (token === null || token !== activeToken) return false;
                const previousFocusOrigin = focusOrigin;
                activeToken = null;
                focusOrigin = null;
                if (previousFocusOrigin?.id) {
                    handOffFocusToReplacement(previousFocusOrigin);
                }
                return true;
            }
        });
    }

    return Object.freeze({ createSessionGate });
});
