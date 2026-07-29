// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const {
    detectOfficialProfile,
    inspectProfile,
    importProfile,
    recoverInterruptedImports,
    selectProfileAdapter
} = require('../node/ProfileMigration');

const temporaryDirectories = [];

function makeDirectory(name) {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), `${name}-`));
    temporaryDirectories.push(directory);
    return directory;
}

function createOfficialProfile(root, options = {}) {
    const profile = path.join(root, 'deltamod');
    const installation = path.join(profile, 'deltamod_system-0');
    const game = path.join(installation, 'deltaruneInstall');
    fs.mkdirSync(game, { recursive: true });
    fs.writeFileSync(path.join(game, 'data.win'), 'game-data');
    fs.writeFileSync(path.join(installation, '_cname'), 'My Install');
    const storeContents = JSON.stringify({
        version: `DELTAMOD_DATA_${options.version || '2.0.1'}`,
        gamePath: game,
        enabledMods: ['example']
    });
    fs.writeFileSync(
        path.join(installation, 'store.json'),
        options.legacyHashSuffix ? `${storeContents}##legacy-integrity-hash` : storeContents
    );
    fs.mkdirSync(path.join(profile, 'deltamod_system-unique'), { recursive: true });
    fs.writeFileSync(path.join(profile, 'deltamod_system-unique', 'language'), 'en');
    fs.mkdirSync(path.join(profile, 'pkg.db', 'Mod_example'), { recursive: true });
    fs.writeFileSync(path.join(profile, 'pkg.db', 'Mod_example', 'meta.toml'), '[metadata]\nname="Example"');
    fs.mkdirSync(path.join(profile, 'customThemes', 'data'), { recursive: true });
    fs.mkdirSync(path.join(profile, 'customThemes', 'img'), { recursive: true });
    fs.mkdirSync(path.join(profile, 'customThemes', 'mus'), { recursive: true });
    fs.writeFileSync(path.join(profile, 'customThemes', 'img', 'custom.png'), 'official-image');
    fs.writeFileSync(path.join(profile, 'customThemes', 'mus', 'custom.ogg'), 'official-music');
    fs.writeFileSync(path.join(profile, 'customThemes', 'data', 'custom.theme.json'), JSON.stringify({
        id: 'custom',
        name: 'Official custom theme',
        background: 'custom.png',
        mainSong: 'custom.ogg'
    }));
    fs.mkdirSync(path.join(profile, 'Partitions', 'deltamod', 'Cache'), { recursive: true });
    fs.writeFileSync(path.join(profile, 'Partitions', 'deltamod', 'Cache', 'ignored'), 'cache');
    return profile;
}

afterEach(() => {
    while (temporaryDirectories.length) {
        fs.rmSync(temporaryDirectories.pop(), { recursive: true, force: true });
    }
});

describe('profile discovery', () => {
    it('finds an official profile without treating the Community destination as the source', () => {
        const appData = makeDirectory('deltamod-appdata');
        const source = createOfficialProfile(appData);
        const destination = path.join(appData, 'Deltamod Community');
        const detected = detectOfficialProfile({ appData, localAppData: null, destinationRoot: destination });

        expect(detected.exists).toBe(true);
        expect(detected.sourceRoot).toBe(source);
        expect(detected.version).toBe('2.0.1');
    });

    it('selects explicit adapters for supported official layouts', () => {
        expect(selectProfileAdapter('1.7.0').id).toBe('official-1.7');
        expect(selectProfileAdapter('2.0.1').id).toBe('official-2.0');
    });

    it('detects and imports legacy stores that append an integrity suffix', async () => {
        const appData = makeDirectory('deltamod-legacy');
        const source = createOfficialProfile(appData, {
            version: '1.7.0',
            legacyHashSuffix: true
        });
        const destination = path.join(appData, 'Deltamod Community');

        const summary = await inspectProfile(source, { destinationRoot: destination });
        expect(summary).toMatchObject({ version: '1.7.0', adapter: 'official-1.7' });

        await importProfile({ sourceRoot: source, destinationRoot: destination });
        const imported = JSON.parse(fs.readFileSync(
            path.join(destination, 'deltamod_system-0', 'store.json'),
            'utf8'
        ));
        expect(imported.enabledMods).toEqual(['example']);
        expect(imported.gamePath).toBe(path.join(destination, 'deltamod_system-0', 'deltaruneInstall'));
    });
});

describe('profile inspection and import', () => {
    it('imports compatible data without touching the official profile or cache', async () => {
        const appData = makeDirectory('deltamod-import');
        const source = createOfficialProfile(appData);
        const destination = path.join(appData, 'Deltamod Community');
        const sourceStore = fs.readFileSync(path.join(source, 'deltamod_system-0', 'store.json'), 'utf8');

        const summary = await inspectProfile(source, { destinationRoot: destination });
        expect(summary).toMatchObject({
            version: '2.0.1',
            adapter: 'official-2.0',
            installations: 1,
            mods: 1,
            themes: 1
        });

        const manifest = await importProfile({ sourceRoot: source, destinationRoot: destination });
        expect(manifest).toMatchObject({ installations: 1, mods: 1, themes: 1 });
        expect(manifest.hashes['deltamod_system-0']).toMatch(/^[a-f0-9]{64}$/);
        expect(manifest.importedObjects.installations[0]).toMatchObject({
            source: 'deltamod_system-0',
            destination: 'deltamod_system-0'
        });
        expect(fs.existsSync(path.join(destination, 'deltamod_system-0', 'deltaruneInstall', 'data.win'))).toBe(true);
        expect(fs.existsSync(path.join(destination, 'pkg.db', 'Mod_example', 'meta.toml'))).toBe(true);
        expect(fs.existsSync(path.join(destination, 'Partitions'))).toBe(false);
        expect(fs.readFileSync(path.join(source, 'deltamod_system-0', 'store.json'), 'utf8')).toBe(sourceStore);

        const importedStore = JSON.parse(fs.readFileSync(path.join(destination, 'deltamod_system-0', 'store.json'), 'utf8'));
        expect(importedStore.gamePath).toBe(path.join(destination, 'deltamod_system-0', 'deltaruneInstall'));
    });

    it('reuses the harmless first-launch placeholder installation', async () => {
        const appData = makeDirectory('deltamod-placeholder');
        const source = createOfficialProfile(appData);
        const destination = path.join(appData, 'Deltamod Community');
        fs.mkdirSync(path.join(destination, 'deltamod_system-0'), { recursive: true });
        fs.writeFileSync(path.join(destination, 'deltamod_system-0', 'store.json'), JSON.stringify({
            version: 'DELTAMOD_DATA_2.0.2',
            gamePid: 'toby.deltarune.demo',
            deltaruneEdition: 'rem'
        }));

        await importProfile({ sourceRoot: source, destinationRoot: destination });

        expect(fs.existsSync(path.join(destination, 'deltamod_system-0', 'deltaruneInstall', 'data.win'))).toBe(true);
        expect(fs.existsSync(path.join(destination, 'deltamod_system-1'))).toBe(false);
    });

    it('quarantines conflicting mod package IDs without altering Community files', async () => {
        const appData = makeDirectory('deltamod-conflict');
        const source = createOfficialProfile(appData);
        const destination = path.join(appData, 'Deltamod Community');
        const communityMod = path.join(destination, 'pkg.db', 'Mod_example');
        fs.mkdirSync(communityMod, { recursive: true });
        fs.writeFileSync(path.join(communityMod, 'meta.toml'), '[metadata]\nname="Community version"');

        const manifest = await importProfile({ sourceRoot: source, destinationRoot: destination });

        expect(fs.readFileSync(path.join(communityMod, 'meta.toml'), 'utf8')).toContain('Community version');
        const conflict = manifest.conflicts.find(item => item.type === 'mod-package');
        expect(conflict).toMatchObject({ resolution: 'quarantined' });
        expect(fs.existsSync(path.join(conflict.destination, 'meta.toml'))).toBe(true);
    });

    it('renames a conflicting theme as one coherent bundle', async () => {
        const appData = makeDirectory('deltamod-theme-conflict');
        const source = createOfficialProfile(appData);
        const destination = path.join(appData, 'Deltamod Community');
        fs.mkdirSync(path.join(destination, 'customThemes', 'data'), { recursive: true });
        fs.mkdirSync(path.join(destination, 'customThemes', 'img'), { recursive: true });
        fs.mkdirSync(path.join(destination, 'customThemes', 'mus'), { recursive: true });
        fs.writeFileSync(path.join(destination, 'customThemes', 'data', 'custom.theme.json'), JSON.stringify({
            id: 'custom',
            name: 'Community theme',
            background: 'custom.png',
            mainSong: 'custom.ogg'
        }));
        fs.writeFileSync(path.join(destination, 'customThemes', 'img', 'custom.png'), 'community-image');
        fs.writeFileSync(path.join(destination, 'customThemes', 'mus', 'custom.ogg'), 'community-music');

        const manifest = await importProfile({ sourceRoot: source, destinationRoot: destination });

        const conflict = manifest.conflicts.find(item => item.type === 'theme');
        expect(conflict).toMatchObject({ resolution: 'renamed' });
        const importedTheme = JSON.parse(fs.readFileSync(conflict.destination, 'utf8'));
        expect(importedTheme.id).toContain('custom_from_deltamod_');
        expect(fs.readFileSync(
            path.join(destination, 'customThemes', 'img', importedTheme.background),
            'utf8'
        )).toBe('official-image');
        expect(fs.readFileSync(
            path.join(destination, 'customThemes', 'mus', importedTheme.mainSong),
            'utf8'
        )).toBe('official-music');
        expect(fs.readFileSync(
            path.join(destination, 'customThemes', 'img', 'custom.png'),
            'utf8'
        )).toBe('community-image');
    });

    it('rolls back Community files when cancelled', async () => {
        const appData = makeDirectory('deltamod-cancel');
        const source = createOfficialProfile(appData);
        const destination = path.join(appData, 'Deltamod Community');
        const controller = new AbortController();
        controller.abort();

        await expect(importProfile({
            sourceRoot: source,
            destinationRoot: destination,
            signal: controller.signal
        })).rejects.toMatchObject({ code: 'IMPORT_CANCELLED' });

        expect(fs.existsSync(path.join(destination, 'deltamod_system-0'))).toBe(false);
    });

    it('fails preflight without creating staging when disk space is insufficient', async () => {
        const appData = makeDirectory('deltamod-low-space');
        const source = createOfficialProfile(appData);
        const destination = path.join(appData, 'Deltamod Community');

        await expect(importProfile({
            sourceRoot: source,
            destinationRoot: destination,
            availableBytes: 1
        })).rejects.toMatchObject({ code: 'INSUFFICIENT_SPACE' });
        expect(fs.existsSync(path.join(destination, 'deltamod_system-0'))).toBe(false);
        expect(fs.readdirSync(appData).some(name => name.startsWith('.deltamod-community-import-'))).toBe(false);
    });

    it('recovers an interrupted commit and restores a replaced placeholder', async () => {
        const root = makeDirectory('deltamod-recovery');
        const destination = path.join(root, 'Deltamod Community');
        const imported = path.join(destination, 'deltamod_system-0');
        const staging = path.join(root, '.deltamod-community-import-test-operation');
        const placeholderBackup = path.join(staging, 'replaced-deltamod_system-0');
        const journalRoot = path.join(destination, '.migration-transactions');
        fs.mkdirSync(imported, { recursive: true });
        fs.writeFileSync(path.join(imported, 'imported'), 'partial');
        fs.mkdirSync(placeholderBackup, { recursive: true });
        fs.writeFileSync(path.join(placeholderBackup, 'store.json'), '{}');
        fs.mkdirSync(journalRoot, { recursive: true });
        fs.writeFileSync(path.join(journalRoot, 'test-operation.json'), JSON.stringify({
            operationId: 'test-operation',
            status: 'committing',
            stagingRoot: staging,
            createdPaths: [imported]
        }));

        await recoverInterruptedImports(destination);

        expect(fs.existsSync(path.join(imported, 'imported'))).toBe(false);
        expect(fs.existsSync(path.join(imported, 'store.json'))).toBe(true);
        expect(fs.existsSync(staging)).toBe(false);
        expect(JSON.parse(fs.readFileSync(
            path.join(journalRoot, 'test-operation.json'),
            'utf8'
        )).status).toBe('rolled-back-after-restart');
    });
});
