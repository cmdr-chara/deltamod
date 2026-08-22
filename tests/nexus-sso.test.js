// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-08-12.
// Licensed under the EUPL 1.2.

const { EventEmitter } = require('events');
const { describe, expect, it, vi } = globalThis;
const {
    AUTHORIZATION_ENDPOINT,
    CALLBACK_HOST,
    CALLBACK_PORT,
    NexusOAuthClient,
    REDIRECT_URI,
    TOKEN_ENDPOINT,
    createPkcePair,
    parseNexusOAuthClientId,
    parseStoredNexusOAuthTokens,
    parseTokenPayload
} = require('../node/NexusSso');

class FakeResponse {
    constructor() {
        this.statusCode = null;
        this.headers = null;
        this.body = '';
    }

    writeHead(statusCode, headers) {
        this.statusCode = statusCode;
        this.headers = headers;
    }

    end(body) {
        this.body = String(body || '');
    }
}

class FakeServer extends EventEmitter {
    constructor(handler) {
        super();
        this.handler = handler;
        this.closed = false;
        this.port = null;
        this.host = null;
    }

    listen(port, host, callback) {
        this.port = port;
        this.host = host;
        queueMicrotask(callback);
    }

    close() {
        this.closed = true;
    }

    request(url, overrides = {}) {
        const response = new FakeResponse();
        this.handler({
            method: 'GET',
            url,
            headers: { host: `${CALLBACK_HOST}:${CALLBACK_PORT}` },
            socket: { remoteAddress: CALLBACK_HOST },
            ...overrides
        }, response);
        return response;
    }
}

const deterministicRandomBytes = size => Buffer.alloc(size, 0x2a);
const accessToken = `access.${'a'.repeat(40)}.token`;
const refreshToken = `refresh.${'b'.repeat(40)}.token`;

function createFakeClient(overrides = {}) {
    const servers = [];
    const client = new NexusOAuthClient({
        clientId: 'deltamod-community',
        openExternal: vi.fn().mockResolvedValue(),
        createServer: handler => {
            const server = new FakeServer(handler);
            servers.push(server);
            return server;
        },
        fetchImpl: vi.fn().mockResolvedValue(new Response(JSON.stringify({
            access_token: accessToken,
            refresh_token: refreshToken,
            token_type: 'Bearer',
            expires_in: 3600,
            scope: ''
        }), {
            status: 200,
            headers: { 'content-type': 'application/json' }
        })),
        randomBytes: deterministicRandomBytes,
        now: () => 1_700_000_000_000,
        timeoutMs: 1000,
        ...overrides
    });
    return { client, servers };
}

describe('Nexus Mods OAuth validation', () => {
    it('accepts a public client ID without accepting paths or whitespace', () => {
        expect(parseNexusOAuthClientId('deltamod-community')).toBe('deltamod-community');
        expect(parseNexusOAuthClientId('')).toBeNull();
        expect(parseNexusOAuthClientId('bad client/with path')).toBeNull();
    });

    it('creates a deterministic S256 PKCE pair and validates stored token bundles', () => {
        const pair = createPkcePair(deterministicRandomBytes);
        expect(pair.verifier).toMatch(/^[A-Za-z0-9_-]{43,128}$/);
        expect(pair.challenge).toMatch(/^[A-Za-z0-9_-]{43}$/);
        expect(pair.challenge).not.toBe(pair.verifier);

        const tokens = parseTokenPayload({
            access_token: accessToken,
            refresh_token: refreshToken,
            token_type: 'bearer',
            expires_in: 60,
            scope: 'public'
        }, { now: 1000 });
        expect(parseStoredNexusOAuthTokens(JSON.stringify(tokens))).toEqual(tokens);
        expect(() => parseStoredNexusOAuthTokens('A-personal-api-key-that-is-not-oauth'))
            .toThrow(/authorization is invalid/);
    });
});

describe('Nexus Mods OAuth desktop session', () => {
    it('binds the fixed IPv4 callback before opening OAuth and exchanges the verified code', async () => {
        const { client, servers } = createFakeClient();
        const pending = client.start();

        await vi.waitFor(() => expect(client.openExternal).toHaveBeenCalledOnce());
        const server = servers[0];
        expect(server.port).toBe(CALLBACK_PORT);
        expect(server.host).toBe(CALLBACK_HOST);

        const authorizationUrl = new URL(client.openExternal.mock.calls[0][0]);
        expect(`${authorizationUrl.origin}${authorizationUrl.pathname}`).toBe(AUTHORIZATION_ENDPOINT);
        expect(authorizationUrl.searchParams.get('client_id')).toBe('deltamod-community');
        expect(authorizationUrl.searchParams.get('redirect_uri')).toBe(REDIRECT_URI);
        expect(authorizationUrl.searchParams.get('response_type')).toBe('code');
        expect(authorizationUrl.searchParams.get('code_challenge_method')).toBe('S256');
        expect(authorizationUrl.searchParams.get('code_challenge')).toMatch(/^[A-Za-z0-9_-]{43}$/);

        const state = authorizationUrl.searchParams.get('state');
        const response = server.request(`/callback?code=authorization-code-123456&state=${state}`);
        const tokens = await pending;
        expect(response.statusCode).toBe(200);
        expect(response.headers['cache-control']).toBe('no-store');
        expect(tokens).toMatchObject({
            accessToken,
            refreshToken,
            tokenType: 'Bearer',
            issuedAt: 1_700_000_000_000,
            expiresAt: 1_700_003_600_000
        });
        expect(client.pending).toBe(false);
        expect(server.closed).toBe(true);

        expect(client.fetchImpl).toHaveBeenCalledOnce();
        const [tokenUrl, request] = client.fetchImpl.mock.calls[0];
        expect(tokenUrl).toBe(TOKEN_ENDPOINT);
        expect(request.redirect).toBe('manual');
        const body = new URLSearchParams(request.body);
        expect(body.get('grant_type')).toBe('authorization_code');
        expect(body.get('redirect_uri')).toBe(REDIRECT_URI);
        expect(body.get('client_id')).toBe('deltamod-community');
        expect(body.get('code')).toBe('authorization-code-123456');
        expect(body.get('code_verifier')).toMatch(/^[A-Za-z0-9_-]{43,128}$/);
        expect(body.has('client_secret')).toBe(false);
    });

    it('ignores an unverified callback and remains cancellable', async () => {
        const { client, servers } = createFakeClient();
        const pending = client.start();
        await vi.waitFor(() => expect(client.openExternal).toHaveBeenCalledOnce());

        const response = servers[0].request('/callback?code=authorization-code-123456&state=wrong');
        expect(response.statusCode).toBe(400);
        expect(client.fetchImpl).not.toHaveBeenCalled();
        expect(client.pending).toBe(true);

        expect(client.cancel()).toBe(true);
        await expect(pending).rejects.toMatchObject({ code: 'NEXUS_SSO_CANCELLED' });
        expect(client.cancel()).toBe(false);
    });

    it('never falls back to a dynamic port when the registered callback is unavailable', async () => {
        const server = new FakeServer(() => {});
        server.listen = function listen(port, host) {
            this.port = port;
            this.host = host;
            const error = new Error('in use');
            error.code = 'EADDRINUSE';
            queueMicrotask(() => this.emit('error', error));
        };
        const client = new NexusOAuthClient({
            clientId: 'deltamod-community',
            openExternal: vi.fn(),
            createServer: () => server,
            randomBytes: deterministicRandomBytes,
            timeoutMs: 1000
        });

        await expect(client.start()).rejects.toMatchObject({
            code: 'NEXUS_OAUTH_CALLBACK_UNAVAILABLE'
        });
        expect(server.port).toBe(CALLBACK_PORT);
        expect(server.host).toBe(CALLBACK_HOST);
        expect(client.openExternal).not.toHaveBeenCalled();
    });

    it('refreshes with the registered client ID and keeps a rotated-or-existing refresh token', async () => {
        const fetchImpl = vi.fn().mockResolvedValue(new Response(JSON.stringify({
            access_token: `new.${'c'.repeat(40)}.token`,
            token_type: 'Bearer',
            expires_in: 7200,
            scope: ''
        }), { status: 200 }));
        const { client } = createFakeClient({ fetchImpl });
        const stored = parseTokenPayload({
            access_token: accessToken,
            refresh_token: refreshToken,
            token_type: 'Bearer',
            expires_in: 3600,
            scope: ''
        }, { now: 1_700_000_000_000 });

        const refreshed = await client.refresh(stored);
        expect(refreshed.refreshToken).toBe(refreshToken);
        const body = new URLSearchParams(fetchImpl.mock.calls[0][1].body);
        expect(body.get('grant_type')).toBe('refresh_token');
        expect(body.get('refresh_token')).toBe(refreshToken);
        expect(body.get('client_id')).toBe('deltamod-community');
        expect(body.has('client_secret')).toBe(false);
    });

    it('surfaces only sanitized OAuth error details when the token exchange is rejected', async () => {
        const fetchImpl = vi.fn().mockResolvedValue(new Response(JSON.stringify({
            error: 'invalid_grant',
            error_description: 'authorization code was rejected\n(and this is bounded)'
        }), { status: 400 }));
        const { client } = createFakeClient({ fetchImpl });

        await expect(client.exchangeAuthorizationCode('authorization-code-123456', 'verifier-1234567890'))
            .rejects.toMatchObject({
                code: 'NEXUS_OAUTH_TOKEN_FAILED',
                status: 400,
                message: 'Nexus Mods rejected the OAuth token exchange (HTTP 400: invalid_grant: authorization code was rejected (and this is bounded)).'
            });
    });

    it('stays disabled until Nexus issues the public OAuth client ID', async () => {
        const unavailable = new NexusOAuthClient({
            clientId: '',
            openExternal: vi.fn()
        });
        await expect(unavailable.start()).rejects.toMatchObject({ code: 'NEXUS_SSO_NOT_REGISTERED' });
    });
});
