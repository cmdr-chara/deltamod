// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');

const tag = process.argv[2];
const input = path.resolve(process.argv[3] || 'CHANGELOG.txt');
const output = path.resolve(process.argv[4] || 'release-notes.md');

if (!/^community-v\d+\.\d+\.\d+$/.test(tag || '')) {
    throw new Error(`Expected stable tag community-v<semver>; received ${tag || '<empty>'}`);
}

const version = tag.slice('community-v'.length);
const text = fs.readFileSync(input, 'utf8');
const header = `# Deltamod Community ${version}`;
const start = text.indexOf(header);
if (start < 0) throw new Error(`CHANGELOG.txt has no section for ${version}`);

const tail = text.slice(start);
const next = tail.slice(header.length).search(/\n# Deltamod Community \d+\.\d+\.\d+\n/);
const section = next < 0 ? tail : tail.slice(0, header.length + next);

const rawLines = section.trim().split(/\r?\n/);
const lines = [];
for (const line of rawLines) {
    if (/^\s+\S/.test(line) && lines.at(-1)?.startsWith('- ')) {
        lines[lines.length - 1] += ` ${line.trim()}`;
    } else {
        lines.push(line);
    }
}
const body = [];
for (const line of lines.slice(1)) {
    if (/^Released \d{4}-\d{2}-\d{2}\.$/.test(line)) continue;
    if (line === '## Changes') {
        body.push('## What changed');
        continue;
    }
    const bullet = /^- (?:(?:feat|fix|docs|ci|build|chore|refactor|perf|test|release)(?:\(([^)]+)\))?: )?(.+?)(?: \(`([0-9a-f]{7,40})`\)\.)?$/.exec(line);
    if (bullet) {
        const scope = bullet[1];
        let subject = bullet[2].trim().replace(/[.]$/, '');
        subject = subject.charAt(0).toUpperCase() + subject.slice(1);
        body.push(scope ? `- **${scope}** — ${subject}.` : `- ${subject}.`);
        continue;
    }
    body.push(line);
}

const notes = [
    `# Deltamod Community ${version}`,
    '',
    ...body,
    '',
    `Technical source history remains available in \`CHANGELOG.txt\` on the \`${tag}\` tag.`,
    ''
].join('\n');

fs.writeFileSync(output, notes, 'utf8');
console.log(`Rendered release notes for ${tag} to ${output}`);
