const {
    GAMEBANANA_LOGIN_PARTITION,
    clearGameBananaAuthentication
} = require('../node/gamebanana/LoginSession');

describe('GameBanana login session cleanup', () => {
    it('removes every local authentication state used by the embedded login', async () => {
        const clearStorageData = vi.fn().mockResolvedValue();
        const clearCache = vi.fn().mockResolvedValue();
        const fromPartition = vi.fn(() => ({ clearStorageData, clearCache }));
        const removeCredential = vi.fn();
        const clearInMemoryCache = vi.fn();

        await expect(clearGameBananaAuthentication({
            electronSession: { fromPartition },
            removeCredential,
            clearInMemoryCache
        })).resolves.toBe(true);

        expect(fromPartition).toHaveBeenCalledWith(GAMEBANANA_LOGIN_PARTITION);
        expect(clearStorageData).toHaveBeenCalledOnce();
        expect(clearCache).toHaveBeenCalledOnce();
        expect(removeCredential).toHaveBeenCalledOnce();
        expect(clearInMemoryCache).toHaveBeenCalledOnce();
    });

    it('still attempts every cleanup step and reports partial failure', async () => {
        const clearStorageData = vi.fn().mockRejectedValue(new Error('storage locked'));
        const clearCache = vi.fn().mockResolvedValue();
        const removeCredential = vi.fn();
        const clearInMemoryCache = vi.fn();

        const cleanup = clearGameBananaAuthentication({
            electronSession: {
                fromPartition: vi.fn(() => ({ clearStorageData, clearCache }))
            },
            removeCredential,
            clearInMemoryCache
        });

        await expect(cleanup).rejects.toMatchObject({
            code: 'GAMEBANANA_LOGOUT_FAILED',
            failedSteps: ['browser storage']
        });
        expect(clearCache).toHaveBeenCalledOnce();
        expect(removeCredential).toHaveBeenCalledOnce();
        expect(clearInMemoryCache).toHaveBeenCalledOnce();
    });
});
