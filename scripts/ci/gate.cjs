// SPDX-FileCopyrightText: 2026 cmdr-chara
// SPDX-License-Identifier: EUPL-1.2
'use strict';

const fs = require('node:fs');

/** A skipped or cancelled prerequisite is not a successful validation. */
function evaluate(needs, required) {
    if (!Array.isArray(required) || required.length === 0 ||
        required.some(name => !/^[a-zA-Z_][a-zA-Z0-9_-]*$/.test(name)) ||
        new Set(required).size !== required.length) {
        throw new Error('The CI gate requires a nonempty, unique list of job IDs.');
    }
    if (!needs || typeof needs !== 'object' || Array.isArray(needs) ||
        Object.keys(needs).sort().join(',') !== [...required].sort().join(',')) {
        throw new Error('The CI gate did not receive exactly its required job results.');
    }
    const rows = required.map(name => {
        const result = needs[name]?.result;
        if (!['success', 'failure', 'cancelled', 'skipped'].includes(result)) {
            throw new Error(`Missing or invalid outcome for ${name}.`);
        }
        return { name, result };
    });
    return { passed: rows.every(row => row.result === 'success'), rows };
}

if (require.main === module) {
    try {
        const { passed, rows } = evaluate(JSON.parse(process.env.CI_NEEDS || 'null'), process.argv.slice(2));
        const summary = '## Native CI result\n\n| Job | Result |\n| --- | --- |\n' +
            rows.map(row => `| ${row.name} | ${row.result} |`).join('\n') + '\n';
        console.log(summary);
        if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, summary);
        if (!passed) console.error('::error::At least one required native CI job did not pass.');
        process.exitCode = passed ? 0 : 1;
    } catch (error) {
        console.error(`CI gate failed: ${error.message}`);
        process.exitCode = 1;
    }
}

module.exports = { evaluate };
