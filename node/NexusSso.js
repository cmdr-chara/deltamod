// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const crypto = require('crypto');
const WebSocket = require('ws');

const SSO_ENDPOINT = 'wss://sso.nexusmods.com';
const SSO_PAGE = 'https://www.nexusmods.com/sso';
const APP_ID_PATTERN = /^[a-z0-9][a-z0-9_-]{1,79}$/i;
const API_KEY_PATTERN = /^[A-Za-z0-9+/=_-]{20,200}$/;

function nexusSsoError(code, message) {
    const error = new Error(message);
    error.code = code;
    return error;
}

function parseNexusSsoAppId(value) {
    const appId = String(value || '').trim();
    return APP_ID_PATTERN.test(appId) ? appId : null;
}

function parseNexusSsoMessage(raw) {
    const text = Buffer.isBuffer(raw) ? raw.toString('utf8') : String(raw || '');
    if (!text || text.length > 4096) {
        throw nexusSsoError('NEXUS_SSO_INVALID_RESPONSE', 'Nexus Mods returned an invalid SSO response.');
    }

    let apiKey = text.trim();
    if (apiKey.startsWith('{')) {
        let response;
        try {
            response = JSON.parse(apiKey);
        } catch {
            throw nexusSsoError('NEXUS_SSO_INVALID_RESPONSE', 'Nexus Mods returned malformed SSO data.');
        }
        if (response?.success === false) {
            throw nexusSsoError(
                'NEXUS_SSO_REJECTED',
                String(response?.error || response?.message || 'Nexus Mods rejected the sign-in request.')
            );
        }
        apiKey = String(response?.data?.api_key || response?.api_key || '').trim();
    }

    if (!API_KEY_PATTERN.test(apiKey)) {
        throw nexusSsoError('NEXUS_SSO_INVALID_RESPONSE', 'Nexus Mods did not return a valid API credential.');
    }
    return apiKey;
}

class NexusSsoClient {
    constructor({
        appId,
        openExternal,
        WebSocketImpl = WebSocket,
        randomUUID = crypto.randomUUID,
        timeoutMs = 5 * 60 * 1000,
        pingIntervalMs = 30 * 1000
    }) {
        this.appId = parseNexusSsoAppId(appId);
        this.openExternal = openExternal;
        this.WebSocketImpl = WebSocketImpl;
        this.randomUUID = randomUUID;
        this.timeoutMs = timeoutMs;
        this.pingIntervalMs = pingIntervalMs;
        this.active = null;
    }

    get available() {
        return Boolean(this.appId);
    }

    get pending() {
        return Boolean(this.active);
    }

    start() {
        if (!this.available) {
            return Promise.reject(nexusSsoError(
                'NEXUS_SSO_NOT_REGISTERED',
                'Nexus Mods SSO is waiting for the application slug issued during registration.'
            ));
        }
        if (this.active) {
            return Promise.reject(nexusSsoError(
                'NEXUS_SSO_ALREADY_PENDING',
                'A Nexus Mods sign-in is already waiting for authorization.'
            ));
        }

        const id = this.randomUUID();
        return new Promise((resolve, reject) => {
            const socket = new this.WebSocketImpl(SSO_ENDPOINT);
            let settled = false;
            let heartbeat = null;
            let timeout = null;

            const cleanup = () => {
                if (heartbeat) clearInterval(heartbeat);
                if (timeout) clearTimeout(timeout);
                if (this.active?.id === id) this.active = null;
                try {
                    if (socket.readyState === this.WebSocketImpl.OPEN) socket.close(1000, 'Authentication complete');
                    else if (typeof socket.terminate === 'function') socket.terminate();
                } catch {}
            };

            const finish = (error, apiKey) => {
                if (settled) return;
                settled = true;
                cleanup();
                if (error) reject(error);
                else resolve(apiKey);
            };

            this.active = {
                id,
                cancel: () => finish(nexusSsoError('NEXUS_SSO_CANCELLED', 'Nexus Mods sign-in was cancelled.'))
            };

            timeout = setTimeout(() => {
                finish(nexusSsoError(
                    'NEXUS_SSO_TIMEOUT',
                    'Nexus Mods sign-in timed out. Start it again when you are ready to authorize the app.'
                ));
            }, this.timeoutMs);

            socket.on('open', async () => {
                try {
                    socket.send(JSON.stringify({ id, appid: this.appId }));
                    heartbeat = setInterval(() => {
                        if (socket.readyState === this.WebSocketImpl.OPEN) {
                            try { socket.ping(); } catch {}
                        }
                    }, this.pingIntervalMs);
                    const authorizationUrl = new URL(SSO_PAGE);
                    authorizationUrl.searchParams.set('id', id);
                    await this.openExternal(authorizationUrl.toString());
                } catch (error) {
                    finish(nexusSsoError(
                        'NEXUS_SSO_BROWSER_FAILED',
                        `The Nexus Mods authorization page could not be opened: ${error.message || error}`
                    ));
                }
            });

            socket.on('message', data => {
                try {
                    finish(null, parseNexusSsoMessage(data));
                } catch (error) {
                    finish(error);
                }
            });

            socket.on('error', error => {
                finish(nexusSsoError(
                    'NEXUS_SSO_CONNECTION_FAILED',
                    `Could not connect to Nexus Mods SSO: ${error.message || error}`
                ));
            });

            socket.on('close', () => {
                if (!settled) {
                    finish(nexusSsoError(
                        'NEXUS_SSO_CONNECTION_CLOSED',
                        'Nexus Mods closed the sign-in connection before authorization completed.'
                    ));
                }
            });
        });
    }

    cancel() {
        if (!this.active) return false;
        this.active.cancel();
        return true;
    }
}

module.exports = {
    API_KEY_PATTERN,
    NexusSsoClient,
    parseNexusSsoAppId,
    parseNexusSsoMessage
};
