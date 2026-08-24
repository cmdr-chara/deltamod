// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2

'use strict';

const fs = require('node:fs');
const path = require('node:path');
const { spawnSync } = require('node:child_process');

const root = path.resolve(__dirname, '..');
const registryPath = path.join(root, 'provenance', 'community-original-work.json');
const expectedCopyright = 'SPDX-FileCopyrightText: 2026 cmdr-chara';
const expectedLicense = 'SPDX-License-Identifier: EUPL-1.2';
const requireHistory = process.env.REQUIRE_PROVENANCE_HISTORY === '1';

function fail(message) {
    throw new Error(`Community provenance verification failed: ${message}`);
}

function runGit(args, allowFailure = false) {
    const result = spawnSync('git', args, {
        cwd: root,
        encoding: 'utf8',
        windowsHide: true
    });
    if (result.error) {
        if (allowFailure) return null;
        fail(`git ${args.join(' ')} could not run: ${result.error.message}`);
    }
    if (result.status !== 0) {
        if (allowFailure) return null;
        fail(`git ${args.join(' ')} exited with ${result.status}: ${String(result.stderr || '').trim()}`);
    }
    return String(result.stdout || '').trim();
}

function validateRegistry(value) {
    if (!value || value.schemaVersion !== 1 || value.license !== 'EUPL-1.2') {
        fail('registry schema or license is invalid');
    }
    if (value.copyrightHolder !== 'cmdr-chara' || value.copyrightYear !== 2026) {
        fail('registry copyright holder/year is invalid');
    }
    if (!Array.isArray(value.entries) || value.entries.length === 0) {
        fail('registry must contain at least one entry');
    }

    const seen = new Set();
    for (const entry of value.entries) {
        if (!entry || typeof entry.path !== 'string' || !entry.path) fail('entry path is missing');
        const normalized = path.posix.normalize(entry.path.replaceAll('\\', '/'));
        if (normalized !== entry.path || normalized.startsWith('../') || path.posix.isAbsolute(normalized)) {
            fail(`unsafe or non-canonical path: ${entry.path}`);
        }
        if (seen.has(entry.path)) fail(`duplicate registry path: ${entry.path}`);
        seen.add(entry.path);
        if (!/^[0-9a-f]{40}$/.test(entry.firstCommit || '')) {
            fail(`invalid first commit for ${entry.path}`);
        }
        if (!/^2026-[0-9]{2}-[0-9]{2}$/.test(entry.firstCommitDate || '')) {
            fail(`invalid first-commit date for ${entry.path}`);
        }
        if (entry.evidence !== 'added') fail(`unsupported evidence type for ${entry.path}`);
    }
    return value.entries;
}

function verifyHeader(entry) {
    const absolute = path.join(root, ...entry.path.split('/'));
    if (!fs.existsSync(absolute) || !fs.statSync(absolute).isFile()) {
        fail(`registered file is missing: ${entry.path}`);
    }
    const prefix = fs.readFileSync(absolute, 'utf8').slice(0, 4096);
    if (!prefix.includes(expectedCopyright)) {
        fail(`${entry.path} is missing ${expectedCopyright}`);
    }
    if (!prefix.includes(expectedLicense)) {
        fail(`${entry.path} is missing ${expectedLicense}`);
    }
}

function verifyHistory(entries) {
    const inside = runGit(['rev-parse', '--is-inside-work-tree'], true) === 'true';
    if (!inside) {
        if (requireHistory) fail('Git history is required but unavailable');
        console.warn('Git history unavailable; verified registry structure and SPDX headers only.');
        return;
    }

    for (const entry of entries) {
        if (!runGit(['cat-file', '-e', `${entry.firstCommit}^{commit}`], true)) {
            // cat-file -e is silent on success, so use rev-parse for a printable success value.
            if (!runGit(['rev-parse', '--verify', `${entry.firstCommit}^{commit}`], true)) {
                fail(`first commit is unavailable for ${entry.path}: ${entry.firstCommit}`);
            }
        }
        const ancestor = spawnSync('git', ['merge-base', '--is-ancestor', entry.firstCommit, 'HEAD'], {
            cwd: root,
            encoding: 'utf8',
            windowsHide: true
        });
        if (ancestor.status !== 0) fail(`${entry.firstCommit} is not an ancestor of HEAD for ${entry.path}`);

        const status = runGit(['show', '--format=', '--name-status', entry.firstCommit, '--', entry.path]);
        const added = status.split(/\r?\n/).some(line => line === `A\t${entry.path}`);
        if (!added) {
            fail(`${entry.path} was not added by recorded first commit ${entry.firstCommit}`);
        }
    }
}

const registry = JSON.parse(fs.readFileSync(registryPath, 'utf8'));
const entries = validateRegistry(registry);
for (const entry of entries) verifyHeader(entry);
verifyHistory(entries);

console.log(`Verified ${entries.length} Community original-work records and SPDX notices.`);
