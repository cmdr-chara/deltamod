// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

function installTestIpcScope() {
    if (process.env.DELTAMOD_TEST !== '1') return;

    const { ipcMain } = require('electron');
    if (ipcMain.__deltamodTestIpcScopeInstalled) return;

    const handle = ipcMain.handle.bind(ipcMain);
    const removeHandler = ipcMain.removeHandler.bind(ipcMain);
    let productionModSourcesBrowse = null;
    let fixtureOverridePending = false;

    Object.defineProperty(ipcMain, '__deltamodTestIpcScopeInstalled', {
        value: true,
        configurable: false,
        enumerable: false,
        writable: false
    });

    ipcMain.handle = (channel, listener) => {
        if (channel !== 'modSources:browse') return handle(channel, listener);

        if (!productionModSourcesBrowse) {
            productionModSourcesBrowse = listener;
            return handle(channel, listener);
        }

        if (!fixtureOverridePending) return handle(channel, listener);
        fixtureOverridePending = false;

        return handle(channel, (event, args) => {
            const provider = args?.[0]?.provider;
            if (provider === 'gamebanana') return listener(event, args);
            return productionModSourcesBrowse(event, args);
        });
    };

    ipcMain.removeHandler = channel => {
        if (channel === 'modSources:browse' && productionModSourcesBrowse) {
            fixtureOverridePending = true;
        }
        return removeHandler(channel);
    };
}

module.exports = { installTestIpcScope };
