// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

function isApprovedGameBananaUrl(candidate) {
    try {
        const parsed = new URL(candidate);
        return parsed.protocol === 'https:'
            && (parsed.hostname === 'gamebanana.com' || parsed.hostname.endsWith('.gamebanana.com'));
    } catch {
        return false;
    }
}

function serializeGameBananaCookies(cookies) {
    return (Array.isArray(cookies) ? cookies : [])
        .filter(cookie => {
            const domain = String(cookie?.domain || '').replace(/^\./, '').toLowerCase();
            return cookie?.name
                && typeof cookie.value === 'string'
                && (domain === 'gamebanana.com' || domain.endsWith('.gamebanana.com'));
        })
        .map(cookie => `${cookie.name}=${cookie.value}`)
        .join('; ');
}

function isAuthenticatedUiConfig(config) {
    return Number(config?._idMemberRow) > 0;
}

module.exports = {
    isApprovedGameBananaUrl,
    isAuthenticatedUiConfig,
    serializeGameBananaCookies
};
