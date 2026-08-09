const fs = require('node:fs');
const path = require('node:path');

const root = path.join(__dirname, '..');
const read = relativePath => fs.readFileSync(path.join(root, relativePath), 'utf8');

describe('Tauri renderer capability gates', () => {
    it('disables beta-facing unsupported installation, account, tool, and download controls', () => {
        const options = read('web/views/options/index.js');
        const installations = read('web/views/installmanager/index.js');
        const shop = read('web/views/gamebanana-browse/index.js');
        const collections = read('web/views/collections/index.js');

        for (const channel of [
            'removeSteamIntegration', 'rebootDev', 'installDeltamodCLI',
            'openFlagDatabase', 'cmode-on', 'loginGamebanana'
        ]) {
            expect(options).toContain(`isCommandAvailable('${channel}')`);
        }
        expect(installations).toContain("isCommandAvailable('createInstallLink')");
        expect(installations).toContain("isCommandAvailable('undertaleModTool:openInstallation')");
        expect(shop).toContain("isCommandAvailable('modSources:downloadNexus')");
        expect(collections).toContain("isCommandAvailable('gamebanana_downloadAllInCollection')");
    });

    it('uses optional invocation for unsupported startup and event-driven commands', () => {
        const app = read('web/index.js');
        const deleteAll = read('web/views/deleteall/index.js');
        const patching = read('web/views/patching/index.js');

        for (const channel of [
            'isCMode', 'shouldGoIM', 'executeArgumentCmd', 'start-update', 'ignore-update',
            'getGamebananaUserinfo', 'cmode-on', 'cmode-off'
        ]) {
            expect(app).toMatch(new RegExp(`invokeOptional\\(\\s*['\"]${channel}`));
        }
        expect(deleteAll).toContain("invokeOptional('initialize'");
        expect(deleteAll).toContain("isCommandAvailable('initialize')");
        expect(patching).toContain("invokeOptional('npsCallback'");
        expect(patching).toContain("isCommandAvailable('npsCallback')");
    });
});
