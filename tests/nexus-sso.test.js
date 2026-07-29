// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { EventEmitter } = require('events');
const { describe, expect, it, vi } = globalThis;
const {
    NexusSsoClient,
    parseNexusSsoAppId,
    parseNexusSsoMessage
} = require('../node/NexusSso');

class FakeWebSocket extends EventEmitter {
    static OPEN = 1;
    static instances = [];

    constructor(url) {
        super();
        this.url = url;
        this.readyState = 0;
        this.sent = [];
        this.pings = 0;
        FakeWebSocket.instances.push(this);
    }

    open() {
        this.readyState = FakeWebSocket.OPEN;
        this.emit('open');
    }

    send(value) {
        this.sent.push(value);
    }

    ping() {
        this.pings += 1;
    }

    close() {
        this.readyState = 3;
    }

    terminate() {
        this.readyState = 3;
    }
}

describe('Nexus Mods SSO validation', () => {
    it('accepts a registration slug without accepting arbitrary strings', () => {
        expect(parseNexusSsoAppId('deltamod-community')).toBe('deltamod-community');
        expect(parseNexusSsoAppId('')).toBeNull();
        expect(parseNexusSsoAppId('bad slug/with path')).toBeNull();
    });

    it('accepts both documented plain keys and structured compatibility responses', () => {
        const key = 'A-secure-looking-api-key-123456';
        expect(parseNexusSsoMessage(key)).toBe(key);
        expect(parseNexusSsoMessage(JSON.stringify({
            success: true,
            data: { api_key: key }
        }))).toBe(key);
        expect(() => parseNexusSsoMessage('short')).toThrow(/valid API credential/);
        expect(() => parseNexusSsoMessage(JSON.stringify({
            success: false,
            error: 'Denied'
        }))).toThrow(/Denied/);
    });
});

describe('Nexus Mods SSO session', () => {
    it('opens the authorization page and resolves only the received credential', async () => {
        FakeWebSocket.instances = [];
        const openExternal = vi.fn().mockResolvedValue();
        const client = new NexusSsoClient({
            appId: 'deltamod-community',
            openExternal,
            WebSocketImpl: FakeWebSocket,
            randomUUID: () => '4c694264-1fdb-48c6-a5a0-8edd9e53c7a6',
            timeoutMs: 1000
        });

        const result = client.start();
        const socket = FakeWebSocket.instances[0];
        socket.open();
        await vi.waitFor(() => expect(openExternal).toHaveBeenCalledOnce());
        expect(socket.url).toBe('wss://sso.nexusmods.com');
        expect(JSON.parse(socket.sent[0])).toEqual({
            id: '4c694264-1fdb-48c6-a5a0-8edd9e53c7a6',
            appid: 'deltamod-community'
        });
        expect(openExternal).toHaveBeenCalledWith(
            'https://www.nexusmods.com/sso?id=4c694264-1fdb-48c6-a5a0-8edd9e53c7a6'
        );

        socket.emit('message', Buffer.from('A-secure-looking-api-key-123456'));
        await expect(result).resolves.toBe('A-secure-looking-api-key-123456');
        expect(client.pending).toBe(false);
    });

    it('stays disabled without a Nexus-issued slug and supports cancellation', async () => {
        const unavailable = new NexusSsoClient({
            appId: '',
            openExternal: vi.fn(),
            WebSocketImpl: FakeWebSocket
        });
        await expect(unavailable.start()).rejects.toMatchObject({ code: 'NEXUS_SSO_NOT_REGISTERED' });

        FakeWebSocket.instances = [];
        const client = new NexusSsoClient({
            appId: 'deltamod-community',
            openExternal: vi.fn().mockResolvedValue(),
            WebSocketImpl: FakeWebSocket,
            timeoutMs: 1000
        });
        const pending = client.start();
        expect(client.cancel()).toBe(true);
        await expect(pending).rejects.toMatchObject({ code: 'NEXUS_SSO_CANCELLED' });
        expect(client.cancel()).toBe(false);
    });
});
