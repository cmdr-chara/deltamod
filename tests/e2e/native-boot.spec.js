// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
const { test, expect } = require('@playwright/test');
const fs = require('node:fs');
const path = require('node:path');

test('native boot preserves progress, theme cues, media ownership and cleanup', async ({ page }) => {
    const root = path.resolve(__dirname, '../..');
    const runtime = fs.readFileSync(path.join(root, 'web/modules/boot-native.mjs'), 'utf8')
        .replace('export default function ', 'function ');
    const checks = fs.readFileSync(path.join(__dirname, 'fixtures/native-boot-checks.mjs'), 'utf8')
        .replace('export function ', 'function ');
    await page.setContent('<!doctype html><html><body></body></html>');
    await page.addScriptTag({ content: `${runtime}\n${checks}\nwindow.bootResult = runNativeBootChecks(installNativeBoot, document);` });
    const result = await page.evaluate(() => window.bootResult);
    expect(result.checks).toHaveLength(7);
    expect(result.passed, JSON.stringify(result.checks, null, 2)).toBe(true);
});
