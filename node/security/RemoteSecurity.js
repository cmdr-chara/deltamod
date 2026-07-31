// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const { Readable, Transform } = require('stream');
const { pipeline } = require('stream/promises');

const GAMEBANANA_HOSTS = new Set([
    'gamebanana.com',
    'images.gamebanana.com'
]);

function isApprovedGameBananaHost(hostname) {
    const normalized = String(hostname || '').toLowerCase().replace(/\.$/, '');
    return GAMEBANANA_HOSTS.has(normalized) || normalized.endsWith('.gamebanana.com');
}

function validateRemoteUrl(value, options = {}) {
    let parsed;
    try {
        parsed = value instanceof URL ? value : new URL(value);
    } catch {
        const error = new Error('The supplied download URL is invalid.');
        error.code = 'INVALID_REMOTE_URL';
        throw error;
    }

    if (parsed.protocol !== 'https:') {
        const error = new Error('Only HTTPS downloads are allowed.');
        error.code = 'INSECURE_REMOTE_URL';
        throw error;
    }

    const hostAllowed = options.allowedHosts
        ? options.allowedHosts(parsed.hostname)
        : isApprovedGameBananaHost(parsed.hostname);
    if (!hostAllowed) {
        const error = new Error(`Downloads from ${parsed.hostname} are not allowed.`);
        error.code = 'UNAPPROVED_REMOTE_HOST';
        throw error;
    }

    if (parsed.username || parsed.password) {
        const error = new Error('Download URLs cannot contain credentials.');
        error.code = 'REMOTE_URL_CREDENTIALS';
        throw error;
    }

    return parsed;
}

async function fetchWithValidatedRedirects(value, options = {}) {
    let current = validateRemoteUrl(value, options);
    const maximumRedirects = options.maximumRedirects ?? 5;

    for (let redirect = 0; redirect <= maximumRedirects; redirect++) {
        const response = await fetch(current, {
            method: options.method || 'GET',
            headers: options.headers,
            body: options.body,
            redirect: 'manual',
            signal: options.signal
        });

        if (![301, 302, 303, 307, 308].includes(response.status)) return { response, url: current };

        const location = response.headers.get('location');
        if (!location || redirect === maximumRedirects) {
            const error = new Error('The download exceeded the redirect limit.');
            error.code = 'REMOTE_REDIRECT_LIMIT';
            throw error;
        }
        current = validateRemoteUrl(new URL(location, current), options);
    }

    throw new Error('Unreachable redirect state.');
}

async function downloadToFile(value, destination, options = {}) {
    const { response, url } = await fetchWithValidatedRedirects(value, options);
    if (!response.ok || !response.body) {
        const error = new Error(`Download failed with HTTP ${response.status}.`);
        error.code = 'REMOTE_DOWNLOAD_FAILED';
        error.status = response.status;
        throw error;
    }

    const maximumBytes = options.maximumBytes ?? 2 * 1024 * 1024 * 1024;
    const advertisedBytes = Number(response.headers.get('content-length')) || 0;
    if (advertisedBytes > maximumBytes) {
        const error = new Error(`Download is larger than the ${maximumBytes} byte limit.`);
        error.code = 'REMOTE_DOWNLOAD_TOO_LARGE';
        throw error;
    }

    await fs.promises.mkdir(require('path').dirname(destination), { recursive: true });
    let received = 0;
    const limiter = new Transform({
        transform(chunk, _encoding, callback) {
            received += chunk.length;
            if (received > maximumBytes) {
                const error = new Error(`Download exceeded the ${maximumBytes} byte limit.`);
                error.code = 'REMOTE_DOWNLOAD_TOO_LARGE';
                callback(error);
                return;
            }
            options.onProgress?.({
                completed: received,
                total: advertisedBytes,
                currentItem: url.toString()
            });
            callback(null, chunk);
        }
    });

    try {
        await pipeline(
            Readable.fromWeb(response.body),
            limiter,
            fs.createWriteStream(destination, { flags: 'wx', mode: 0o600 })
        );
    } catch (error) {
        try { await fs.promises.rm(destination, { force: true }); } catch {}
        throw error;
    }

    return {
        destination,
        bytes: received,
        contentType: response.headers.get('content-type') || '',
        finalUrl: url.toString()
    };
}

module.exports = {
    GAMEBANANA_HOSTS,
    isApprovedGameBananaHost,
    validateRemoteUrl,
    fetchWithValidatedRedirects,
    downloadToFile
};
