const { describe, expect, it } = globalThis;
const {
    isApprovedGameBananaHost,
    validateRemoteUrl
} = require('../node/security/RemoteSecurity');

describe('GameBanana remote URL validation', () => {
    it.each([
        'https://gamebanana.com/mmdl/123',
        'https://images.gamebanana.com/example.png',
        'https://files.gamebanana.com/archive.zip'
    ])('accepts approved URL %s', value => {
        expect(validateRemoteUrl(value).protocol).toBe('https:');
    });

    it.each([
        'http://gamebanana.com/mmdl/123',
        'https://gamebanana.com.evil.example/archive.zip',
        'https://127.0.0.1/archive.zip',
        'file:///etc/passwd'
    ])('rejects unsafe URL %s', value => {
        expect(() => validateRemoteUrl(value)).toThrow();
    });

    it('does not approve suffix lookalikes', () => {
        expect(isApprovedGameBananaHost('notgamebanana.com')).toBe(false);
    });
});
