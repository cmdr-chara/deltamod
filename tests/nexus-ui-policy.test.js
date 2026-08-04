// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-08-04.
// Licensed under the EUPL 1.2.

const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const read = relativePath => fs.readFileSync(path.join(root, relativePath), 'utf8');

describe('Nexus renderer policy', () => {
    it('exposes SSO only and never renders a pasted credential control', () => {
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
        expect(ipc).toMatch(/getNexusAuthMethod\(\) !== 'sso'[\s\S]*?clearNexusCredentialFiles\(\)[\s\S]*?return null/);
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

    it('documents the SSO-only, bounded, quota-aware catalogue contract', () => {
        const readme = read('README.md');
        expect(readme).toMatch(/single-sign-on only/i);
        expect(readme).toMatch(/bounded result page/i);
        expect(readme).toMatch(/quota.*Retry-After/i);
        expect(readme).not.toMatch(/personal\s+api\s+key/i);
    });
});
