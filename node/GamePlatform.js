// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-30.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');

function platformDefinitions(game) {
    const definitions = { ...(game?.platforms || {}) };
    if (!definitions.win32 && game?.exeName) {
        definitions.win32 = {
            executable: game.exeName,
            dataFiles: ['data.win'],
            patchLayout: 'windows-root'
        };
    }
    return definitions;
}

function candidatePlatforms(game, hostPlatform, preferredPlatform) {
    const definitions = platformDefinitions(game);
    const candidates = [];
    const isCompatible = platform => (
        platform === hostPlatform
        || (hostPlatform === 'linux' && platform === 'win32')
    );
    const add = platform => {
        if (
            definitions[platform]
            && isCompatible(platform)
            && !candidates.includes(platform)
        ) {
            candidates.push(platform);
        }
    };

    add(preferredPlatform);
    add(hostPlatform);
    if (hostPlatform === 'linux') add('win32');
    return candidates;
}

function mapPatchTarget(relativePath, definition) {
    const normalized = String(relativePath || '').replaceAll('\\', '/');
    const segments = normalized.split('/');
    if (
        !normalized
        || path.posix.isAbsolute(normalized)
        || /^[a-z]:/i.test(normalized)
        || segments.some(segment => segment === '..')
    ) {
        throw new Error('Patch target contains an unsafe platform path.');
    }
    const lower = normalized.toLowerCase();
    const dataFile = definition.dataFiles?.[0];

    if (lower === 'data.win' && dataFile) return dataFile;

    switch (definition.patchLayout) {
        case 'gamemaker-linux-assets':
        case 'gamemaker-mac-resources':
            return path.posix.join(definition.contentRoot, normalized);
        case 'deltarune-mac-resources': {
            const chapterPath = normalized.replace(
                /^chapter([1-5])_windows\//i,
                'chapter$1_mac/'
            );
            const mapped = chapterPath.replace(/(^|\/)data\.win$/i, '$1game.ios');
            return path.posix.join(definition.contentRoot, mapped);
        }
        default:
            return normalized;
    }
}

function resolveGameInstallation(game, root, options = {}) {
    if (!game || typeof root !== 'string' || !root.trim()) return null;
    const resolvedRoot = path.resolve(root);
    if (!fs.existsSync(resolvedRoot) || !fs.statSync(resolvedRoot).isDirectory()) return null;

    const definitions = platformDefinitions(game);
    const hostPlatform = options.hostPlatform || process.platform;
    for (const platform of candidatePlatforms(game, hostPlatform, options.preferredPlatform)) {
        const definition = definitions[platform];
        const required = [
            definition.executable,
            ...(definition.dataFiles || []),
            ...(definition.bundle ? [definition.bundle] : [])
        ].filter(Boolean);
        const missing = required.filter(relative => !fs.existsSync(path.join(resolvedRoot, relative)));
        if (missing.length > 0) continue;

        return {
            root: resolvedRoot,
            platform,
            native: platform === hostPlatform,
            definition,
            executablePath: path.join(resolvedRoot, definition.executable),
            bundlePath: definition.bundle ? path.join(resolvedRoot, definition.bundle) : null,
            mapPatchTarget: relative => mapPatchTarget(relative, definition)
        };
    }
    return null;
}

function createLaunchSpec(resolution, configuredLinuxLauncher) {
    if (!resolution) throw new Error('The game installation is unavailable.');

    if (resolution.platform === 'darwin') {
        return {
            command: 'open',
            args: ['-W', resolution.bundlePath],
            cwd: resolution.root
        };
    }

    if (resolution.platform === 'linux') {
        return {
            command: 'sh',
            args: [resolution.executablePath],
            cwd: resolution.root
        };
    }

    if (process.platform === 'linux') {
        if (
            configuredLinuxLauncher
            && typeof configuredLinuxLauncher === 'object'
            && typeof configuredLinuxLauncher.command === 'string'
            && configuredLinuxLauncher.command.trim()
        ) {
            return {
                command: configuredLinuxLauncher.command.trim(),
                args: Array.isArray(configuredLinuxLauncher.args)
                    ? configuredLinuxLauncher.args.map(arg => String(arg).replaceAll('{exe}', resolution.executablePath))
                    : [resolution.executablePath],
                cwd: path.dirname(resolution.executablePath)
            };
        }
        return {
            command: 'wine',
            args: [resolution.executablePath],
            cwd: path.dirname(resolution.executablePath)
        };
    }

    return {
        command: resolution.executablePath,
        args: [],
        cwd: path.dirname(resolution.executablePath)
    };
}

module.exports = {
    createLaunchSpec,
    mapPatchTarget,
    platformDefinitions,
    resolveGameInstallation
};
