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
