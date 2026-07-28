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
});
