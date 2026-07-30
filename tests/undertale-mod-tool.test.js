// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { EventEmitter } = require('events');
const {
    validateExecutablePath,
    validateCliExecutablePath,
    resolveGameDataFile,
    createWorkspace,
    launchEditor
} = require('../node/UndertaleModTool');

describe('UndertaleModTool integration', () => {
    let root;

    beforeEach(() => {
        root = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-umt-'));
    });

    afterEach(() => {
        fs.rmSync(root, { recursive: true, force: true });
    });

    it('resolves a regular GameMaker data file from an installation', () => {
        const dataFile = path.join(root, 'data.win');
        fs.writeFileSync(dataFile, 'FORM');
        expect(resolveGameDataFile(root)).toBe(fs.realpathSync.native(dataFile));
    });

    it('rejects missing data files and non-executable tool paths', () => {
        expect(() => resolveGameDataFile(root)).toThrow('No supported GameMaker data file');
        const wrongExtension = path.join(root, 'UndertaleModTool.txt');
        fs.writeFileSync(wrongExtension, 'not an executable');
        expect(() => validateExecutablePath(wrongExtension)).toThrow('must be one of');
    });

    it('rejects hardlinked executables', () => {
        const executable = path.join(root, 'UndertaleModTool.WinUI.exe');
        const linkedExecutable = path.join(root, 'UndertaleModTool-linked.exe');
        fs.writeFileSync(executable, 'test executable');
        fs.linkSync(executable, linkedExecutable);
        expect(() => validateExecutablePath(executable)).toThrow('regular, non-linked file');
    });

    it('launches with a separated --open argument and without a shell', async () => {
        const executable = path.join(root, 'UndertaleModTool.WinUI.exe');
        const dataFile = path.join(root, 'data.win');
        fs.writeFileSync(executable, 'test executable');
        fs.writeFileSync(dataFile, 'FORM');

        let invocation;
        const fakeSpawn = (command, args, options) => {
            invocation = { command, args, options };
            const child = new EventEmitter();
            child.unref = vi.fn();
            queueMicrotask(() => child.emit('spawn'));
            return child;
        };

        const result = await launchEditor(executable, dataFile, fakeSpawn);
        expect(result.launched).toBe(true);
        expect(invocation.command).toBe(fs.realpathSync.native(executable));
        expect(invocation.args).toEqual(['--open', fs.realpathSync.native(dataFile)]);
        expect(invocation.options.shell).toBe(false);
    });

    it('creates a verified isolated workspace without changing the source data', async () => {
        const sourceDirectory = path.join(root, 'game');
        const workspaceRoot = path.join(root, 'workspaces');
        const sourceDataFile = path.join(sourceDirectory, 'data.win');
        const cliExecutable = path.join(root, 'deltamod-community-cli.exe');
        fs.mkdirSync(sourceDirectory);
        fs.writeFileSync(sourceDataFile, 'FORM-community-test');
        fs.writeFileSync(cliExecutable, 'test executable');

        const result = await createWorkspace({
            workspaceRoot,
            sourceDataFile,
            cliExecutable: validateCliExecutablePath(cliExecutable),
            installationIndex: '4',
            installationName: 'Test install',
            gameId: 'toby.deltarune',
            author: 'Tester'
        });

        expect(result.dataFile).not.toBe(fs.realpathSync.native(sourceDataFile));
        expect(fs.readFileSync(result.dataFile, 'utf8')).toBe('FORM-community-test');
        expect(fs.readFileSync(sourceDataFile, 'utf8')).toBe('FORM-community-test');

        const manifest = JSON.parse(fs.readFileSync(result.manifestFile, 'utf8'));
        expect(manifest.schemaVersion).toBe(1);
        expect(manifest.dataFile).toBe('data.win');
        expect(manifest.package).toEqual(expect.objectContaining({
            name: 'UMT edits - Test install',
            packageId: 'community.undertalemodtool.installation-4',
            game: 'toby.deltarune',
            author: 'Tester'
        }));
        expect(manifest.source.sha256).toBe(result.sourceSha256);
        expect(fs.existsSync(`${result.workspace}.staging`)).toBe(false);
    });
});
