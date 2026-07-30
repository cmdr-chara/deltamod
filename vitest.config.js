// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { defineConfig } = require('vitest/config');

module.exports = defineConfig({
    test: {
        globals: true,
        environment: 'node',
        include: ['tests/**/*.test.js'],
        coverage: {
            reporter: ['text', 'html']
        }
    }
});
