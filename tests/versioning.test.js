const { isNewerVersion } = require('../node/updates/Versioning');

it('offers only a genuinely newer semantic version', () => {
    expect(isNewerVersion('2.0.3', '2.0.2')).toBe(true);
    expect(isNewerVersion('2.0.2', '2.0.2')).toBe(false);
    expect(isNewerVersion('2.0.1', '2.0.2')).toBe(false);
    expect(isNewerVersion('not-a-version', '2.0.2')).toBe(false);
});

it('keeps prereleases out of the stable channel', () => {
    expect(isNewerVersion('2.1.0-beta.1', '2.0.2')).toBe(false);
    expect(isNewerVersion('2.1.0-beta.1', '2.0.2', { allowPrerelease: true })).toBe(true);
});
