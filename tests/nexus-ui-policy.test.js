// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-08-04.
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const read = relativePath => fs.readFileSync(path.join(root, relativePath), 'utf8');

describe('Nexus renderer policy', () => {
    it('exposes OAuth PKCE only and never renders a pasted credential control', () => {
        const ipc = read('node/IPCHandlers.js');
        const preload = read('web/preload.js');
        const types = read('web/types/preload.d.ts');
        const options = read('web/views/options/index.js');
        const sources = `${preload}\n${types}\n${options}`;

        expect(sources).not.toContain('setNexusKey');
        expect(options).not.toMatch(/personal\s+api\s+key|paste\s+api\s+key|api%20access/i);
        expect(options).toContain('startNexusSso');
        expect(options).toContain('NEXUS_SSO_NOT_REGISTERED');
        expect(options).toContain('status.ssoAvailable === true');
        expect(ipc).toMatch(/handle\('modSources:setNexusKey',[\s\S]*?throw createNexusPersonalKeyDisabledError\(\)/);
        expect(ipc).toMatch(/getNexusAuthMethod\(\) !== 'oauth-pkce'[\s\S]*?clearNexusCredentialFiles\(\)[\s\S]*?return null/);
        expect(ipc).toContain('DELTAMOD_NEXUS_OAUTH_CLIENT_ID');
        expect(ipc).not.toContain('DELTAMOD_NEXUS_SSO_APP_ID');
    });

    it('serializes external refreshes and presents typed Nexus quota waits', () => {
        const shop = read('web/views/gamebanana-browse/index.js');

        expect(shop).toContain('requestExternalSource');
        expect(shop).toContain('externalBrowseState.active');
        expect(shop).toContain("error?.code === 'NEXUS_RATE_LIMITED'");
        expect(shop).toContain('retryAfterMs');
        expect(shop).toContain('retryAt');
        expect(shop).toContain('error?.quota');
        expect(shop).toContain('Nexus Mods rate limit reached');
        expect(shop).toMatch(/if \(isNexusRateLimited\(error\)\) \{[\s\S]*?renderSourceState\([\s\S]*?formatRateLimitMessage\(error\)[\s\S]*?scheduleNexusRateLimitRetry/);
    });

    it('routes forbidden free-tier downloads to the website and archive-import fallback', () => {
        const shop = read('web/views/gamebanana-browse/index.js');

        expect(shop).toContain("Number(error?.status) === 403");
        expect(shop).toContain("error?.code === 'NEXUS_MANUAL_DOWNLOAD_REQUIRED'");
        expect(shop).toContain("{ text: 'Open Nexus Mods', resolveWith: 'open' }");
        expect(shop).toContain("{ text: 'Import archive', resolveWith: 'import' }");
        expect(shop).toContain("window.deltamodBackend.invoke('importMod', [])");
        expect(shop).toContain("phase: manual ? 'manual' : 'failed'");
        expect(shop).toContain("manual: 'Website confirmation required'");
        expect(shop).toMatch(/const authorizationRequired = \[[\s\S]*?'NEXUS_AUTH_REQUIRED'[\s\S]*?Number\(error\?\.status\) !== 403/);
    });

    it('documents the OAuth PKCE, fixed-callback, bounded, quota-aware catalogue contract', () => {
        const readme = read('README.md');
        expect(readme).toMatch(/OAuth 2\.0 Authorization Code with PKCE S256/i);
        expect(readme).toContain('http://127.0.0.1:52817/callback');
        expect(readme).toMatch(/never falls back to a dynamic port/i);
        expect(readme).toMatch(/bounded result page/i);
        expect(readme).toMatch(/quota.*Retry-After/i);
        expect(readme).not.toMatch(/personal\s+api\s+key/i);
    });
});
