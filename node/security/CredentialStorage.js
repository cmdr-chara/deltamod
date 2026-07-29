// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

function getCredentialStorageStatus(safeStorage, platform = process.platform) {
    if (!safeStorage?.isEncryptionAvailable?.()) {
        return {
            available: false,
            backend: null,
            reason: 'Encrypted credential storage is unavailable on this system.'
        };
    }

    if (platform !== 'linux') {
        return {
            available: true,
            backend: platform,
            reason: null
        };
    }

    const backend = typeof safeStorage.getSelectedStorageBackend === 'function'
        ? safeStorage.getSelectedStorageBackend()
        : 'unknown';
    if (backend === 'basic_text') {
        return {
            available: false,
            backend,
            reason: 'The Linux desktop keyring is unavailable, so Electron would store credentials with weak basic-text protection.'
        };
    }
    if (backend === 'unknown') {
        return {
            available: false,
            backend,
            reason: 'The Linux credential-storage backend could not be verified.'
        };
    }

    return {
        available: true,
        backend,
        reason: null
    };
}

function requireSecureCredentialStorage(safeStorage, platform = process.platform) {
    const status = getCredentialStorageStatus(safeStorage, platform);
    if (!status.available) {
        const error = new Error(status.reason);
        error.code = 'SECURE_STORAGE_UNAVAILABLE';
        error.backend = status.backend;
        throw error;
    }
    return status;
}

module.exports = {
    getCredentialStorageStatus,
    requireSecureCredentialStorage
};
