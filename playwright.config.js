// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { defineConfig } = require('@playwright/test');

module.exports = defineConfig({
    testDir: './tests/e2e',
    timeout: 60_000,
    workers: 1,
    retries: 0,
    use: {
        trace: 'retain-on-failure',
        screenshot: 'only-on-failure'
    }
});
