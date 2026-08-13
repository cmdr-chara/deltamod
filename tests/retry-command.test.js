// Copyright © 2026 cmdr-chara
// Licensed under the EUPL 1.2.

const {
    parseArguments,
    resolveCommand,
    runWithRetry,
    safeAnnotation
} = require('../scripts/retry-command');

describe('retry-command', () => {
    it('parses retry settings separately from the child command', () => {
        expect(parseArguments([
            '--attempts', '4',
            '--delay-ms', '250',
            '--label', 'Electron download',
            '--', 'npm', 'ci'
        ])).toEqual({
            attempts: 4,
            delayMs: 250,
            label: 'Electron download',
            command: 'npm',
            args: ['ci']
        });
    });

    it('rejects an absent child command and invalid options', () => {
        expect(() => parseArguments(['--attempts', '3'])).toThrow(/Usage/);
        expect(() => parseArguments(['--attempts', 'zero', '--', 'npm', 'ci'])).toThrow(/integer/);
        expect(() => parseArguments(['--unknown', 'value', '--', 'npm', 'ci'])).toThrow(/Unknown/);
    });

    it('runs npm through its JavaScript entry point on Windows', () => {
        expect(resolveCommand('npm', ['ci'], {
            platform: 'win32',
            nodeExecutable: 'C:\\Node\\node.exe',
            existsSync: () => true
        })).toEqual({
            command: 'C:\\Node\\node.exe',
            args: ['C:\\Node\\node_modules\\npm\\bin\\npm-cli.js', 'ci']
        });
    });

    it('retries failed commands with increasing delays', async () => {
        const spawn = vi.fn()
            .mockReturnValueOnce({ status: 1 })
            .mockReturnValueOnce({ status: 1 })
            .mockReturnValueOnce({ status: 0 });
        const sleep = vi.fn().mockResolvedValue();
        const logger = { warn: vi.fn(), error: vi.fn() };

        await expect(runWithRetry({
            attempts: 3,
            delayMs: 50,
            label: 'download',
            command: 'node',
            args: ['download.js']
        }, { spawn, sleep, logger, platform: 'linux' })).resolves.toBe(0);

        expect(spawn).toHaveBeenCalledTimes(3);
        expect(sleep.mock.calls).toEqual([[50], [100]]);
        expect(logger.warn).toHaveBeenCalledTimes(2);
        expect(logger.error).not.toHaveBeenCalled();
    });

    it('returns the final failure after exhausting all attempts', async () => {
        const logger = { warn: vi.fn(), error: vi.fn() };
        const spawn = vi.fn().mockReturnValue({ status: 7 });

        await expect(runWithRetry({
            attempts: 2,
            delayMs: 0,
            label: 'persistent failure',
            command: 'node',
            args: []
        }, { spawn, sleep: vi.fn().mockResolvedValue(), logger, platform: 'linux' })).resolves.toBe(7);

        expect(logger.error).toHaveBeenCalledWith('::error::persistent failure failed after 2 attempts.');
    });

    it('prevents child labels and errors from injecting workflow annotations', () => {
        expect(safeAnnotation('download\n::error::forged')).toBe('download :error:forged');
    });
});
