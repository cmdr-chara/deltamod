// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-08-12.
// Licensed under the EUPL 1.2.

const crypto = require('crypto');
const http = require('http');

const AUTHORIZATION_ENDPOINT = 'https://users.nexusmods.com/oauth/authorize';
const TOKEN_ENDPOINT = 'https://users.nexusmods.com/oauth/token';
const CALLBACK_HOST = '127.0.0.1';
const CALLBACK_PORT = 52817;
const CALLBACK_PATH = '/callback';
const REDIRECT_URI = `http://${CALLBACK_HOST}:${CALLBACK_PORT}${CALLBACK_PATH}`;
const CLIENT_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._~-]{0,199}$/;
const OAUTH_CODE_PATTERN = /^[\x21-\x7e]{8,4096}$/;
const OAUTH_TOKEN_PATTERN = /^[\x21-\x7e]{20,8192}$/;
const MAX_TOKEN_RESPONSE_BYTES = 32 * 1024;

function nexusOAuthError(code, message, metadata = {}) {
    const error = new Error(message);
    error.code = code;
    Object.assign(error, metadata);
    return error;
}

function parseNexusOAuthClientId(value) {
    const clientId = String(value || '').trim();
    return CLIENT_ID_PATTERN.test(clientId) ? clientId : null;
}

function parseNexusOAuthScope(value) {
    const scope = String(value ?? '').trim();
    if (!scope) return '';
    if (scope.length > 256 || !/^[A-Za-z0-9._:-]+(?: [A-Za-z0-9._:-]+)*$/.test(scope)) {
        return null;
    }
    return scope;
}

function base64Url(value) {
    return Buffer.from(value).toString('base64url');
}

function createPkcePair(randomBytes = crypto.randomBytes) {
    const verifier = base64Url(randomBytes(48));
    return {
        verifier,
        challenge: base64Url(crypto.createHash('sha256').update(verifier, 'ascii').digest())
    };
}

function isSafeOAuthToken(value) {
    return OAUTH_TOKEN_PATTERN.test(String(value || ''));
}

function parseTokenPayload(payload, { now = Date.now(), fallbackRefreshToken = null } = {}) {
    if (!payload || typeof payload !== 'object' || Array.isArray(payload)) {
        throw nexusOAuthError(
            'NEXUS_OAUTH_INVALID_TOKEN_RESPONSE',
            'Nexus Mods returned an invalid OAuth token response.'
        );
    }

    const accessToken = String(payload.access_token || '').trim();
    const refreshToken = String(payload.refresh_token || fallbackRefreshToken || '').trim();
    const tokenType = String(payload.token_type || '').trim();
    const expiresIn = Number(payload.expires_in);
    const scope = parseNexusOAuthScope(payload.scope ?? '');

    if (!isSafeOAuthToken(accessToken)
        || !isSafeOAuthToken(refreshToken)
        || tokenType.toLowerCase() !== 'bearer'
        || !Number.isFinite(expiresIn)
        || expiresIn <= 0
        || expiresIn > 365 * 24 * 60 * 60
        || scope === null) {
        throw nexusOAuthError(
            'NEXUS_OAUTH_INVALID_TOKEN_RESPONSE',
            'Nexus Mods returned an incomplete or invalid OAuth token response.'
        );
    }

    const issuedAt = Math.round(Number(now));
    return {
        schemaVersion: 1,
        accessToken,
        refreshToken,
        tokenType: 'Bearer',
        issuedAt,
        expiresAt: issuedAt + Math.round(expiresIn * 1000),
        scope
    };
}

function parseStoredNexusOAuthTokens(value) {
    let payload = value;
    if (typeof value === 'string') {
        if (!value || value.length > 24 * 1024) {
            throw nexusOAuthError(
                'NEXUS_CREDENTIAL_INVALID',
                'The saved Nexus Mods authorization is invalid.'
            );
        }
        try {
            payload = JSON.parse(value);
        } catch {
            throw nexusOAuthError(
                'NEXUS_CREDENTIAL_INVALID',
                'The saved Nexus Mods authorization is invalid.'
            );
        }
    }

    const scope = parseNexusOAuthScope(payload?.scope ?? '');
    if (payload?.schemaVersion !== 1
        || !isSafeOAuthToken(payload?.accessToken)
        || !isSafeOAuthToken(payload?.refreshToken)
        || payload?.tokenType !== 'Bearer'
        || !Number.isFinite(Number(payload?.issuedAt))
        || !Number.isFinite(Number(payload?.expiresAt))
        || Number(payload.expiresAt) <= Number(payload.issuedAt)
        || scope === null) {
        throw nexusOAuthError(
            'NEXUS_CREDENTIAL_INVALID',
            'The saved Nexus Mods authorization is invalid.'
        );
    }

    return {
        schemaVersion: 1,
        accessToken: String(payload.accessToken),
        refreshToken: String(payload.refreshToken),
        tokenType: 'Bearer',
        issuedAt: Math.round(Number(payload.issuedAt)),
        expiresAt: Math.round(Number(payload.expiresAt)),
        scope
    };
}

async function readBoundedResponseText(response, maximumBytes = MAX_TOKEN_RESPONSE_BYTES) {
    const advertisedBytes = Number(response?.headers?.get?.('content-length')) || 0;
    if (advertisedBytes > maximumBytes) {
        throw nexusOAuthError(
            'NEXUS_OAUTH_INVALID_TOKEN_RESPONSE',
            'Nexus Mods returned an oversized OAuth response.'
        );
    }

    if (response?.body?.getReader) {
        const reader = response.body.getReader();
        const chunks = [];
        let total = 0;
        while (true) {
            const { done, value } = await reader.read();
            if (done) break;
            const chunk = Buffer.from(value);
            total += chunk.length;
            if (total > maximumBytes) {
                try { await reader.cancel(); } catch {}
                throw nexusOAuthError(
                    'NEXUS_OAUTH_INVALID_TOKEN_RESPONSE',
                    'Nexus Mods returned an oversized OAuth response.'
                );
            }
            chunks.push(chunk);
        }
        return Buffer.concat(chunks).toString('utf8');
    }

    const text = await response.text();
    if (Buffer.byteLength(text, 'utf8') > maximumBytes) {
        throw nexusOAuthError(
            'NEXUS_OAUTH_INVALID_TOKEN_RESPONSE',
            'Nexus Mods returned an oversized OAuth response.'
        );
    }
    return text;
}

function stateMatches(received, expected) {
    const left = Buffer.from(String(received || ''), 'utf8');
    const right = Buffer.from(String(expected || ''), 'utf8');
    return left.length === right.length && crypto.timingSafeEqual(left, right);
}

function isLoopbackRequest(request) {
    const address = String(request?.socket?.remoteAddress || '');
    const host = String(request?.headers?.host || '').toLowerCase();
    return (address === CALLBACK_HOST || address === `::ffff:${CALLBACK_HOST}`)
        && host === `${CALLBACK_HOST}:${CALLBACK_PORT}`;
}

function sendCallbackResponse(response, statusCode, title, message) {
    const body = `<!doctype html><html lang="en"><meta charset="utf-8"><title>${title}</title>`
        + `<body><main><h1>${title}</h1><p>${message}</p></main></body></html>`;
    response.writeHead(statusCode, {
        'cache-control': 'no-store',
        'content-security-policy': "default-src 'none'; frame-ancestors 'none'; base-uri 'none'",
        'content-type': 'text/html; charset=utf-8',
        'content-length': Buffer.byteLength(body, 'utf8'),
        'referrer-policy': 'no-referrer',
        'x-content-type-options': 'nosniff',
        'x-frame-options': 'DENY'
    });
    response.end(body);
}

class NexusOAuthClient {
    constructor({
        clientId,
        scope = '',
        openExternal,
        createServer = handler => http.createServer(handler),
        fetchImpl = globalThis.fetch,
        randomBytes = crypto.randomBytes,
        now = Date.now,
        timeoutMs = 5 * 60 * 1000,
        tokenTimeoutMs = 30 * 1000
    }) {
        this.clientId = parseNexusOAuthClientId(clientId);
        this.scope = parseNexusOAuthScope(scope);
        this.openExternal = openExternal;
        this.createServer = createServer;
        this.fetchImpl = fetchImpl;
        this.randomBytes = randomBytes;
        this.now = now;
        this.timeoutMs = timeoutMs;
        this.tokenTimeoutMs = tokenTimeoutMs;
        this.active = null;
    }

    get available() {
        return Boolean(this.clientId && this.scope !== null);
    }

    get pending() {
        return Boolean(this.active);
    }

    buildAuthorizationUrl(state, challenge) {
        const url = new URL(AUTHORIZATION_ENDPOINT);
        url.searchParams.set('client_id', this.clientId);
        url.searchParams.set('redirect_uri', REDIRECT_URI);
        url.searchParams.set('response_type', 'code');
        url.searchParams.set('scope', this.scope);
        url.searchParams.set('state', state);
        url.searchParams.set('code_challenge', challenge);
        url.searchParams.set('code_challenge_method', 'S256');
        return url.toString();
    }

    async requestTokens(parameters, { signal, refreshing = false, fallbackRefreshToken = null } = {}) {
        if (typeof this.fetchImpl !== 'function') {
            throw nexusOAuthError(
                'NEXUS_OAUTH_CONNECTION_FAILED',
                'This build cannot contact the Nexus Mods authorization service.'
            );
        }

        const controller = new AbortController();
        const forwardAbort = () => controller.abort();
        signal?.addEventListener('abort', forwardAbort, { once: true });
        const timeout = setTimeout(() => controller.abort(), this.tokenTimeoutMs);
        let response;
        try {
            response = await this.fetchImpl(TOKEN_ENDPOINT, {
                method: 'POST',
                redirect: 'manual',
                cache: 'no-store',
                credentials: 'omit',
                headers: {
                    accept: 'application/json',
                    'content-type': 'application/x-www-form-urlencoded'
                },
                body: new URLSearchParams(parameters).toString(),
                signal: controller.signal
            });
        } catch (error) {
            if (signal?.aborted) {
                throw nexusOAuthError('NEXUS_SSO_CANCELLED', 'Nexus Mods sign-in was cancelled.');
            }
            if (controller.signal.aborted) {
                throw nexusOAuthError(
                    'NEXUS_OAUTH_TOKEN_TIMEOUT',
                    'Nexus Mods did not finish the token exchange in time.'
                );
            }
            throw nexusOAuthError(
                'NEXUS_OAUTH_CONNECTION_FAILED',
                'Could not contact the Nexus Mods authorization service.'
            );
        } finally {
            clearTimeout(timeout);
            signal?.removeEventListener('abort', forwardAbort);
        }

        const raw = await readBoundedResponseText(response);
        if (!response.ok) {
            const reauthRequired = refreshing && response.status >= 400 && response.status < 500;
            throw nexusOAuthError(
                reauthRequired ? 'NEXUS_OAUTH_REAUTH_REQUIRED' : 'NEXUS_OAUTH_TOKEN_FAILED',
                reauthRequired
                    ? 'The Nexus Mods authorization has expired or was revoked. Sign in again.'
                    : 'Nexus Mods rejected the OAuth token exchange.',
                { status: Number(response.status) || 0 }
            );
        }

        let payload;
        try {
            payload = JSON.parse(raw);
        } catch {
            throw nexusOAuthError(
                'NEXUS_OAUTH_INVALID_TOKEN_RESPONSE',
                'Nexus Mods returned malformed OAuth token data.'
            );
        }
        return parseTokenPayload(payload, {
            now: this.now(),
            fallbackRefreshToken
        });
    }

    exchangeAuthorizationCode(code, verifier, signal) {
        return this.requestTokens({
            grant_type: 'authorization_code',
            redirect_uri: REDIRECT_URI,
            scope: this.scope,
            client_id: this.clientId,
            code,
            code_verifier: verifier
        }, { signal });
    }

    async refresh(tokens, { signal } = {}) {
        const stored = parseStoredNexusOAuthTokens(tokens);
        return this.requestTokens({
            grant_type: 'refresh_token',
            refresh_token: stored.refreshToken,
            client_id: this.clientId
        }, {
            signal,
            refreshing: true,
            fallbackRefreshToken: stored.refreshToken
        });
    }

    start() {
        if (!this.available) {
            return Promise.reject(nexusOAuthError(
                'NEXUS_SSO_NOT_REGISTERED',
                'Nexus Mods sign-in is waiting for the OAuth client ID issued during registration.'
            ));
        }
        if (this.active) {
            return Promise.reject(nexusOAuthError(
                'NEXUS_SSO_ALREADY_PENDING',
                'A Nexus Mods sign-in is already waiting for authorization.'
            ));
        }

        const state = base64Url(this.randomBytes(32));
        const { verifier, challenge } = createPkcePair(this.randomBytes);
        const abortController = new AbortController();

        return new Promise((resolve, reject) => {
            let settled = false;
            let callbackConsumed = false;
            let timeout = null;
            let server = null;

            const cleanup = () => {
                if (timeout) clearTimeout(timeout);
                abortController.abort();
                if (this.active?.state === state) this.active = null;
                try { server?.close(); } catch {}
            };

            const finish = (error, tokens) => {
                if (settled) return;
                settled = true;
                cleanup();
                if (error) reject(error);
                else resolve(tokens);
            };

            const handleCallback = async (request, response) => {
                if (!isLoopbackRequest(request)) {
                    sendCallbackResponse(response, 403, 'Request rejected', 'This callback is only available locally.');
                    return;
                }
                if (request.method !== 'GET') {
                    sendCallbackResponse(response, 405, 'Request rejected', 'Only the OAuth callback request is accepted.');
                    return;
                }

                let callbackUrl;
                try {
                    if (!request.url || request.url.length > 8192) throw new Error('Invalid callback URL');
                    callbackUrl = new URL(request.url, REDIRECT_URI);
                } catch {
                    sendCallbackResponse(response, 400, 'Request rejected', 'The OAuth callback was malformed.');
                    return;
                }
                if (callbackUrl.pathname !== CALLBACK_PATH) {
                    sendCallbackResponse(response, 404, 'Not found', 'This local listener only accepts the OAuth callback.');
                    return;
                }

                const states = callbackUrl.searchParams.getAll('state');
                if (states.length !== 1 || !stateMatches(states[0], state)) {
                    sendCallbackResponse(response, 400, 'Request rejected', 'The OAuth callback could not be verified.');
                    return;
                }
                if (callbackConsumed) {
                    sendCallbackResponse(response, 409, 'Already received', 'Return to Deltamod Community.');
                    return;
                }

                const oauthErrors = callbackUrl.searchParams.getAll('error');
                if (oauthErrors.length > 0) {
                    callbackConsumed = true;
                    sendCallbackResponse(response, 400, 'Authorization declined', 'Return to Deltamod Community to try again.');
                    finish(nexusOAuthError(
                        'NEXUS_OAUTH_REJECTED',
                        'Nexus Mods authorization was declined or cancelled.'
                    ));
                    return;
                }

                const codes = callbackUrl.searchParams.getAll('code');
                if (codes.length !== 1 || !OAUTH_CODE_PATTERN.test(codes[0])) {
                    sendCallbackResponse(response, 400, 'Request rejected', 'Nexus Mods did not provide a valid authorization code.');
                    return;
                }

                callbackConsumed = true;
                sendCallbackResponse(
                    response,
                    200,
                    'Authorization received',
                    'You can close this tab and return to Deltamod Community.'
                );
                try { server.close(); } catch {}
                try {
                    const tokens = await this.exchangeAuthorizationCode(
                        codes[0],
                        verifier,
                        abortController.signal
                    );
                    finish(null, tokens);
                } catch (error) {
                    finish(error);
                }
            };

            try {
                server = this.createServer((request, response) => {
                    handleCallback(request, response).catch(() => {
                        try {
                            sendCallbackResponse(
                                response,
                                500,
                                'Authorization failed',
                                'Return to Deltamod Community and try again.'
                            );
                        } catch {}
                        finish(nexusOAuthError(
                            'NEXUS_OAUTH_CALLBACK_FAILED',
                            'The local Nexus Mods callback could not be processed.'
                        ));
                    });
                });
                if ('headersTimeout' in server) server.headersTimeout = 5000;
                if ('requestTimeout' in server) server.requestTimeout = 5000;
                if ('keepAliveTimeout' in server) server.keepAliveTimeout = 1000;
                if ('maxHeadersCount' in server) server.maxHeadersCount = 32;
            } catch {
                finish(nexusOAuthError(
                    'NEXUS_OAUTH_CALLBACK_UNAVAILABLE',
                    `Deltamod Community could not open the fixed callback ${REDIRECT_URI}.`
                ));
                return;
            }

            this.active = {
                state,
                cancel: () => finish(nexusOAuthError(
                    'NEXUS_SSO_CANCELLED',
                    'Nexus Mods sign-in was cancelled.'
                ))
            };

            timeout = setTimeout(() => {
                finish(nexusOAuthError(
                    'NEXUS_SSO_TIMEOUT',
                    'Nexus Mods sign-in timed out. Start it again when you are ready to authorize the app.'
                ));
            }, this.timeoutMs);

            server.once('error', error => {
                const unavailable = ['EADDRINUSE', 'EACCES'].includes(String(error?.code || ''));
                finish(nexusOAuthError(
                    unavailable ? 'NEXUS_OAUTH_CALLBACK_UNAVAILABLE' : 'NEXUS_OAUTH_CALLBACK_FAILED',
                    unavailable
                        ? `The fixed Nexus Mods callback ${REDIRECT_URI} is unavailable. Close the app using port ${CALLBACK_PORT} and try again.`
                        : 'The local Nexus Mods callback listener failed.'
                ));
            });

            try {
                server.listen(CALLBACK_PORT, CALLBACK_HOST, async () => {
                    try {
                        await this.openExternal(this.buildAuthorizationUrl(state, challenge));
                    } catch {
                        finish(nexusOAuthError(
                            'NEXUS_SSO_BROWSER_FAILED',
                            'The Nexus Mods authorization page could not be opened.'
                        ));
                    }
                });
            } catch {
                finish(nexusOAuthError(
                    'NEXUS_OAUTH_CALLBACK_UNAVAILABLE',
                    `The fixed Nexus Mods callback ${REDIRECT_URI} is unavailable.`
                ));
            }
        });
    }

    cancel() {
        if (!this.active) return false;
        this.active.cancel();
        return true;
    }
}

module.exports = {
    AUTHORIZATION_ENDPOINT,
    CALLBACK_HOST,
    CALLBACK_PATH,
    CALLBACK_PORT,
    NexusOAuthClient,
    REDIRECT_URI,
    TOKEN_ENDPOINT,
    createPkcePair,
    parseNexusOAuthClientId,
    parseStoredNexusOAuthTokens,
    parseTokenPayload
};
