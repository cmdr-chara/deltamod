// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const GAMEBANANA_LOGIN_PARTITION = 'persist:gamebananaLogin';

async function clearGameBananaAuthentication({
    electronSession,
    removeCredential,
    clearInMemoryCache
}) {
    const failures = [];
    let loginSession;

    try {
        loginSession = electronSession.fromPartition(GAMEBANANA_LOGIN_PARTITION);
    } catch (cause) {
        failures.push({ step: 'browser session', cause });
    }

    const cleanupSteps = [
        ...(loginSession ? [
            {
                step: 'browser storage',
                run: () => loginSession.clearStorageData()
            },
            {
                step: 'browser cache',
                run: () => loginSession.clearCache()
            }
        ] : []),
        {
            step: 'saved credential',
            run: removeCredential
        },
        {
            step: 'in-memory cache',
            run: clearInMemoryCache
        }
    ];

    for (const cleanup of cleanupSteps) {
        try {
            await cleanup.run();
        } catch (cause) {
            failures.push({ step: cleanup.step, cause });
        }
    }

    if (failures.length > 0) {
        const error = new Error(
            `GameBanana logout could not clear: ${failures.map(failure => failure.step).join(', ')}.`
        );
        error.code = 'GAMEBANANA_LOGOUT_FAILED';
        error.failedSteps = failures.map(failure => failure.step);
        error.cause = failures[0].cause;
        throw error;
    }

    return true;
}

module.exports = {
    GAMEBANANA_LOGIN_PARTITION,
    clearGameBananaAuthentication
};
