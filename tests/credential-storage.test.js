// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const {
    getCredentialStorageStatus,
    requireSecureCredentialStorage
} = require('../node/security/CredentialStorage');

function storage({ available = true, backend = 'gnome_libsecret' } = {}) {
    return {
        isEncryptionAvailable: vi.fn(() => available),
        getSelectedStorageBackend: vi.fn(() => backend)
    };
}

describe('credential storage policy', () => {
    it('accepts operating-system credential storage on Windows', () => {
        expect(getCredentialStorageStatus(storage(), 'win32')).toMatchObject({
            available: true,
            backend: 'win32'
        });
    });

    it.each(['gnome_libsecret', 'kwallet', 'kwallet5', 'kwallet6'])(
        'accepts the Linux %s keyring backend',
        backend => {
            expect(getCredentialStorageStatus(storage({ backend }), 'linux')).toMatchObject({
                available: true,
                backend
            });
        }
    );

    it.each(['basic_text', 'unknown'])(
        'rejects the Linux %s backend',
        backend => {
            expect(() => requireSecureCredentialStorage(storage({ backend }), 'linux'))
                .toThrow(/Linux|credential-storage/);
        }
    );

    it('fails closed when a Linux build cannot identify its keyring backend', () => {
        expect(getCredentialStorageStatus({
            isEncryptionAvailable: () => true
        }, 'linux')).toMatchObject({
            available: false,
            backend: 'unknown'
        });
    });

    it('rejects storage when Electron reports encryption unavailable', () => {
        expect(getCredentialStorageStatus(storage({ available: false }), 'win32'))
            .toMatchObject({ available: false });
    });
});
