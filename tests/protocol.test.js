// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { parseLaunch } = require('../node/protocol/LaunchParser');

describe('Community protocol parsing', () => {
    it('parses a Community GameBanana URL', () => {
        expect(parseLaunch('deltamod-community://gb/Mod/123/https://gamebanana.com/mmdl/456')).toEqual({
            command: 'gb',
            arguments: ['Mod', '123', 'https:', '', 'gamebanana.com', 'mmdl', '456']
        });
    });

    it('does not claim the official Deltamod protocol', () => {
        expect(parseLaunch('deltamod://launch/0')).toBe(null);
    });

    it('parses a percent-encoded local archive path', () => {
        expect(parseLaunch('deltamod-community://import?path=C%3A%5CUsers%5CChara%5CMy%20Mod.modarchive')).toEqual({
            command: 'import',
            arguments: [],
            parameters: {
                path: 'C:\\Users\\Chara\\My Mod.modarchive'
            }
        });
    });

    it('rejects duplicate protocol parameters', () => {
        expect(() => parseLaunch('deltamod-community://import?path=one.zip&path=two.zip')).toThrow(
            'provided more than once'
        );
    });
});
