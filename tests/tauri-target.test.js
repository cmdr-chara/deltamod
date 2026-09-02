const { hostTarget, resolveTauriTarget } = require('../scripts/lib/tauri-target');

describe('Tauri host target selection', () => {
    it.each([
        ['win32', 'x64', 'x86_64-pc-windows-msvc'],
        ['linux', 'x64', 'x86_64-unknown-linux-gnu'],
        ['darwin', 'x64', 'x86_64-apple-darwin'],
        ['darwin', 'arm64', 'aarch64-apple-darwin']
    ])('maps %s/%s to %s', (platform, arch, expected) => {
        expect(hostTarget(platform, arch)).toBe(expected);
        expect(resolveTauriTarget(undefined, {}, platform, arch)).toBe(expected);
    });

    it('prefers explicit arguments and release environment targets', () => {
        expect(resolveTauriTarget('x86_64-apple-darwin', {}, 'win32', 'x64'))
            .toBe('x86_64-apple-darwin');
        expect(resolveTauriTarget(undefined, { TAURI_BUILD_TARGET: 'aarch64-apple-darwin' }, 'win32', 'x64'))
            .toBe('aarch64-apple-darwin');
        expect(resolveTauriTarget(undefined, { RUST_TARGET: 'x86_64-unknown-linux-gnu' }, 'win32', 'x64'))
            .toBe('x86_64-unknown-linux-gnu');
    });

    it('fails closed on unsupported hosts and explicit targets', () => {
        expect(() => resolveTauriTarget(undefined, {}, 'linux', 'arm64')).toThrow(/Target must be/);
        expect(() => resolveTauriTarget('wasm32-unknown-unknown', {}, 'win32', 'x64')).toThrow(/Target must be/);
    });
});
