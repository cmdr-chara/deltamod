// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const {
    isApprovedGameBananaUrl,
    isAuthenticatedUiConfig,
    serializeGameBananaCookies
} = require('../node/gamebanana/LoginValidation');

describe('GameBanana login validation', () => {
    it('accepts only HTTPS GameBanana navigation', () => {
        expect(isApprovedGameBananaUrl('https://gamebanana.com/members/account')).toBe(true);
        expect(isApprovedGameBananaUrl('https://api.gamebanana.com/path')).toBe(true);
        expect(isApprovedGameBananaUrl('http://gamebanana.com/')).toBe(false);
        expect(isApprovedGameBananaUrl('https://gamebanana.com.evil.example/')).toBe(false);
    });

    it('serializes only GameBanana cookies without logging metadata', () => {
        expect(serializeGameBananaCookies([
            { domain: '.gamebanana.com', name: 'session', value: 'secret' },
            { domain: 'cdn.gamebanana.com', name: 'preference', value: 'dark' },
            { domain: 'example.com', name: 'foreign', value: 'ignore' },
            { domain: '.gamebanana.com', name: '', value: 'ignore' }
        ])).toBe('session=secret; preference=dark');
    });

    it('requires a positive member ID before accepting a login', () => {
        expect(isAuthenticatedUiConfig({ _idMemberRow: 42 })).toBe(true);
        expect(isAuthenticatedUiConfig({ _idMemberRow: 0 })).toBe(false);
        expect(isAuthenticatedUiConfig({})).toBe(false);
    });
});
