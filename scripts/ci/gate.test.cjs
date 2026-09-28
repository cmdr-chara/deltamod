// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';
const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawnSync } = require('node:child_process');
const { evaluate } = require('./gate.cjs');
const required = ['quality', 'verify'];
const outcomes = (quality, verify) => ({ quality: { result: quality }, verify: { result: verify } });

test('only two successful prerequisites pass, independent of JSON key order', () => {
    assert.equal(evaluate({ verify: { result: 'success' }, quality: { result: 'success' } }, required).passed, true);
});
for (const result of ['failure', 'cancelled', 'skipped']) {
    test(`${result} cannot turn into a passing aggregate check`, () => {
        assert.equal(evaluate(outcomes(result, 'success'), required).passed, false);
        assert.equal(evaluate(outcomes('success', result), required).passed, false);
    });
}
test('missing, empty, additional and malformed results fail closed', () => {
    for (const input of [null, [], {}, { quality: { result: 'success' } },
        { ...outcomes('success', 'success'), unexpected: { result: 'success' } },
        outcomes('success', 'pending'), outcomes('success', null)]) {
        assert.throws(() => evaluate(input, required));
    }
    for (const list of [[], ['verify', 'verify'], ['bad\nname']]) {
        assert.throws(() => evaluate(outcomes('success', 'success'), list));
    }
});
test('actual gate process preserves failure status and writes the result summary', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-ci-gate-'));
    try {
        for (const [input, expected] of [[outcomes('success', 'success'), 0], [outcomes('success', 'skipped'), 1], [null, 1]]) {
            const summary = path.join(root, `result-${expected}-${input === null}.md`);
            const result = spawnSync(process.execPath, [path.join(__dirname, 'gate.cjs'), ...required], {
                env: { ...process.env, CI_NEEDS: JSON.stringify(input), GITHUB_STEP_SUMMARY: summary },
                encoding: 'utf8', timeout: 5000, shell: false
            });
            assert.ifError(result.error);
            assert.equal(result.status, expected, result.stderr);
            if (input) assert.match(fs.readFileSync(summary, 'utf8'), /Native CI result/);
        }
    } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
