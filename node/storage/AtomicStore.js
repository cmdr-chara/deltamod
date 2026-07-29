// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

function temporaryName(filePath) {
    return `${filePath}.${process.pid}.${crypto.randomBytes(6).toString('hex')}.tmp`;
}

function writeFileAtomicSync(filePath, contents, options = {}) {
    const parent = path.dirname(filePath);
    fs.mkdirSync(parent, { recursive: true });

    const tempPath = temporaryName(filePath);
    const backupPath = `${filePath}.backup`;
    let descriptor;

    try {
        descriptor = fs.openSync(tempPath, 'wx', 0o600);
        fs.writeFileSync(descriptor, contents, options.encoding || 'utf8');
        fs.fsyncSync(descriptor);
        fs.closeSync(descriptor);
        descriptor = null;

        if (options.backup !== false && fs.existsSync(filePath)) {
            fs.copyFileSync(filePath, backupPath);
        }

        fs.renameSync(tempPath, filePath);
        return filePath;
    } catch (error) {
        if (descriptor !== undefined && descriptor !== null) {
            try { fs.closeSync(descriptor); } catch {}
        }
        try { fs.rmSync(tempPath, { force: true }); } catch {}
        throw error;
    }
}

function writeJsonAtomicSync(filePath, value, options = {}) {
    const serialized = `${JSON.stringify(value, null, options.spaces ?? 2)}\n`;
    return writeFileAtomicSync(filePath, serialized, options);
}

function readJsonSync(filePath, fallback = null) {
    try {
        return JSON.parse(fs.readFileSync(filePath, 'utf8').split('##')[0]);
    } catch (error) {
        if (arguments.length >= 2) return fallback;
        error.code = error.code || 'INVALID_JSON_STORE';
        throw error;
    }
}

module.exports = {
    writeFileAtomicSync,
    writeJsonAtomicSync,
    readJsonSync
};
