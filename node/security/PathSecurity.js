const fs = require('fs');
const path = require('path');

class UnsafePathError extends Error {
    constructor(message, candidate) {
        super(message);
        this.name = 'UnsafePathError';
        this.code = 'UNSAFE_PATH';
        this.candidate = candidate;
    }
}

function normalizeForComparison(value) {
    const resolved = path.resolve(value);
    return process.platform === 'win32' ? resolved.toLowerCase() : resolved;
}

function decodePath(value) {
    let decoded = String(value);
    for (let i = 0; i < 3; i++) {
        const next = decodeURIComponent(decoded);
        if (next === decoded) break;
        decoded = next;
    }
    return decoded;
}

function validateRelativePath(candidate) {
    if (typeof candidate !== 'string' || candidate.length === 0) {
        throw new UnsafePathError('Path must be a non-empty string.', candidate);
    }

    let decoded;
    try {
        decoded = decodePath(candidate);
    } catch {
        throw new UnsafePathError('Path contains invalid URL encoding.', candidate);
    }

    if (decoded.includes('\0')) {
        throw new UnsafePathError('Path contains a null byte.', candidate);
    }

    if (
        path.isAbsolute(decoded)
        || /^[a-zA-Z]:/.test(decoded)
        || decoded.startsWith('\\\\')
        || decoded.startsWith('//')
        || decoded.startsWith('\\\\?\\')
        || decoded.startsWith('\\\\.\\')
    ) {
        throw new UnsafePathError('Absolute and device paths are not allowed.', candidate);
    }

    const separated = decoded.replace(/[\\/]+/g, path.sep);
    if (separated.split(path.sep).some(segment => segment === '..')) {
        throw new UnsafePathError('Path traversal is not allowed.', candidate);
    }

    const normalized = path.normalize(separated);
    return normalized;
}

function isWithin(root, target, allowRoot = false) {
    const normalizedRoot = normalizeForComparison(root);
    const normalizedTarget = normalizeForComparison(target);
    if (normalizedRoot === normalizedTarget) return allowRoot;
    const relative = path.relative(normalizedRoot, normalizedTarget);
    return relative !== ''
        && !relative.startsWith(`..${path.sep}`)
        && relative !== '..'
        && !path.isAbsolute(relative);
}

function nearestExistingAncestor(target) {
    let current = path.resolve(target);
    while (!fs.existsSync(current)) {
        const parent = path.dirname(current);
        if (parent === current) return null;
        current = parent;
    }
    return current;
}

function assertNoSymlinkEscape(root, target) {
    const existingRoot = fs.realpathSync.native(root);
    const ancestor = nearestExistingAncestor(target);
    if (!ancestor) {
        throw new UnsafePathError('No existing parent could be verified.', target);
    }
    const realAncestor = fs.realpathSync.native(ancestor);
    if (!isWithin(existingRoot, realAncestor, true)) {
        throw new UnsafePathError('Path escapes its root through a link or reparse point.', target);
    }
}

function resolveWithin(root, candidate, options = {}) {
    const normalizedRoot = path.resolve(root);
    const relative = validateRelativePath(candidate);
    const resolved = path.resolve(normalizedRoot, relative);

    if (!isWithin(normalizedRoot, resolved, options.allowRoot === true)) {
        throw new UnsafePathError('Resolved path escapes its allowed root.', candidate);
    }

    if (options.verifyLinks !== false && fs.existsSync(normalizedRoot)) {
        assertNoSymlinkEscape(normalizedRoot, resolved);
    }

    if (options.mustExist === true && !fs.existsSync(resolved)) {
        const error = new Error(`Required path does not exist: ${candidate}`);
        error.code = 'PATH_NOT_FOUND';
        throw error;
    }

    return resolved;
}

function protocolPath(url) {
    const parsed = url instanceof URL ? url : new URL(url);
    const host = decodeURIComponent(parsed.hostname);
    const pathname = decodeURIComponent(parsed.pathname).replace(/^[/\\]+/, '');
    return validateRelativePath([host, pathname].filter(Boolean).join(path.sep));
}

module.exports = {
    UnsafePathError,
    validateRelativePath,
    resolveWithin,
    isWithin,
    protocolPath
};
