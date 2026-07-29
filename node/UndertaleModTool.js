const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { spawn } = require('child_process');

const DATA_FILE_NAMES = Object.freeze([
    'data.win',
    'game.ios',
    'game.unx',
    'game.droid'
]);

function integrationError(code, message) {
    const error = new Error(message);
    error.code = code;
    return error;
}

function validateRegularFile(filePath, {
    code,
    description,
    extensions
}) {
    if (typeof filePath !== 'string' || !path.isAbsolute(filePath)) {
        throw integrationError(code, `${description} path must be absolute.`);
    }

    const normalized = path.resolve(filePath);
    if (extensions && !extensions.includes(path.extname(normalized).toLowerCase())) {
        throw integrationError(code, `${description} must be one of: ${extensions.join(', ')}.`);
    }

    let linkStats;
    let stats;
    try {
        linkStats = fs.lstatSync(normalized);
        stats = fs.statSync(normalized);
    } catch {
        throw integrationError(code, `${description} was not found.`);
    }

    if (!stats.isFile() || !linkStats.isFile() || linkStats.isSymbolicLink() || linkStats.nlink !== 1) {
        throw integrationError(code, `${description} must be a regular, non-linked file.`);
    }

    return fs.realpathSync.native(normalized);
}

function validateExecutablePath(filePath) {
    return validateRegularFile(filePath, {
        code: 'UMT_EXECUTABLE_INVALID',
        description: 'UndertaleModTool executable',
        extensions: ['.exe']
    });
}

function validateCliExecutablePath(filePath) {
    return validateRegularFile(filePath, {
        code: 'COMMUNITY_CLI_EXECUTABLE_INVALID',
        description: 'Deltamod Community CLI executable',
        extensions: process.platform === 'win32' ? ['.exe'] : undefined
    });
}

function resolveGameDataFile(gameDirectory) {
    if (typeof gameDirectory !== 'string' || !path.isAbsolute(gameDirectory)) {
        throw integrationError('UMT_GAME_PATH_INVALID', 'The game directory path must be absolute.');
    }

    let root;
    try {
        root = fs.realpathSync.native(gameDirectory);
        if (!fs.statSync(root).isDirectory()) throw new Error('not a directory');
    } catch {
        throw integrationError('UMT_GAME_PATH_INVALID', 'The game directory is unavailable.');
    }

    for (const fileName of DATA_FILE_NAMES) {
        const candidate = path.join(root, fileName);
        if (!fs.existsSync(candidate)) continue;
        return validateRegularFile(candidate, {
            code: 'UMT_DATA_FILE_INVALID',
            description: 'GameMaker data file'
        });
    }

    throw integrationError(
        'UMT_DATA_FILE_MISSING',
        `No supported GameMaker data file was found (${DATA_FILE_NAMES.join(', ')}).`
    );
}

function validateWorkspaceRoot(workspaceRoot) {
    if (typeof workspaceRoot !== 'string' || !path.isAbsolute(workspaceRoot)) {
        throw integrationError('UMT_WORKSPACE_PATH_INVALID', 'The workspace root path must be absolute.');
    }

    fs.mkdirSync(workspaceRoot, { recursive: true });
    const root = fs.realpathSync.native(workspaceRoot);
    const stats = fs.lstatSync(root);
    if (!stats.isDirectory() || stats.isSymbolicLink()) {
        throw integrationError('UMT_WORKSPACE_PATH_INVALID', 'The workspace root must be a regular directory.');
    }
    return root;
}

function safeManifestSegment(value, fallback) {
    const normalized = String(value ?? '')
        .toLowerCase()
        .replace(/[^a-z0-9_-]+/g, '-')
        .replace(/^-+|-+$/g, '')
        .slice(0, 48);
    return normalized || fallback;
}

function validateGameId(value) {
    const gameId = String(value ?? '').trim();
    if (!/^[a-z0-9][a-z0-9_-]*(?:\.[a-z0-9][a-z0-9_-]*)+$/i.test(gameId)) {
        throw integrationError('UMT_GAME_ID_INVALID', 'The installation has no valid game identifier.');
    }
    return gameId;
}

async function hashFile(filePath) {
    const hash = crypto.createHash('sha256');
    await new Promise((resolve, reject) => {
        const input = fs.createReadStream(filePath);
        input.on('data', chunk => hash.update(chunk));
        input.once('error', reject);
        input.once('end', resolve);
    });
    return hash.digest('hex');
}

async function createWorkspace({
    workspaceRoot,
    sourceDataFile,
    cliExecutable,
    installationIndex,
    installationName,
    gameId,
    author
}) {
    const root = validateWorkspaceRoot(workspaceRoot);
    const source = validateRegularFile(sourceDataFile, {
        code: 'UMT_DATA_FILE_INVALID',
        description: 'GameMaker data file'
    });
    const cli = validateCliExecutablePath(cliExecutable);
    const validatedGameId = validateGameId(gameId);
    const sourceStats = fs.statSync(source);
    const workspaceId = `installation-${safeManifestSegment(installationIndex, 'unknown')}-${Date.now()}-${crypto.randomUUID()}`;
    const staging = path.join(root, `${workspaceId}.staging`);
    const workspace = path.join(root, workspaceId);
    const editorDirectory = path.join(staging, 'editor');
    const exportsDirectory = path.join(staging, 'exports');
    const dataFile = path.join(editorDirectory, path.basename(source));
    const manifestFile = path.join(editorDirectory, '.deltamod-community.json');

    try {
        await fs.promises.mkdir(editorDirectory, { recursive: true });
        await fs.promises.mkdir(exportsDirectory, { recursive: true });
        await fs.promises.copyFile(source, dataFile, fs.constants.COPYFILE_EXCL);

        const copiedStats = await fs.promises.lstat(dataFile);
        if (!copiedStats.isFile() || copiedStats.isSymbolicLink() || copiedStats.nlink !== 1) {
            throw integrationError('UMT_WORKSPACE_COPY_INVALID', 'The workspace copy is not a regular file.');
        }
        if (copiedStats.size !== sourceStats.size) {
            throw integrationError('UMT_WORKSPACE_COPY_INVALID', 'The workspace copy size does not match the source.');
        }

        const [sourceSha256, copySha256] = await Promise.all([
            hashFile(source),
            hashFile(dataFile)
        ]);
        if (sourceSha256 !== copySha256) {
            throw integrationError('UMT_WORKSPACE_COPY_INVALID', 'The workspace copy hash does not match the source.');
        }

        const safeIndex = safeManifestSegment(installationIndex, 'unknown');
        const manifest = {
            schemaVersion: 1,
            workspaceId,
            dataFile: path.basename(dataFile),
            exportRoot: '..\\exports',
            cliExecutable: cli,
            source: {
                size: sourceStats.size,
                sha256: sourceSha256
            },
            package: {
                name: `UMT edits - ${String(installationName || `Installation ${installationIndex}`).trim()}`,
                packageId: `community.undertalemodtool.installation-${safeIndex}`,
                game: validatedGameId,
                author: String(author || 'Deltamod Community user').trim()
            }
        };
        const manifestTemporary = `${manifestFile}.tmp`;
        await fs.promises.writeFile(manifestTemporary, `${JSON.stringify(manifest, null, 2)}\n`, {
            encoding: 'utf8',
            flag: 'wx',
            mode: 0o600
        });
        await fs.promises.rename(manifestTemporary, manifestFile);
        await fs.promises.rename(staging, workspace);

        return {
            workspace,
            dataFile: path.join(workspace, 'editor', path.basename(dataFile)),
            manifestFile: path.join(workspace, 'editor', path.basename(manifestFile)),
            sourceSha256,
            size: sourceStats.size
        };
    } catch (error) {
        await fs.promises.rm(staging, { recursive: true, force: true }).catch(() => {});
        throw error;
    }
}

async function launchEditor(executablePath, dataFilePath, spawnImpl = spawn) {
    const executable = validateExecutablePath(executablePath);
    const dataFile = validateRegularFile(dataFilePath, {
        code: 'UMT_DATA_FILE_INVALID',
        description: 'GameMaker data file'
    });

    const child = spawnImpl(executable, ['--open', dataFile], {
        cwd: path.dirname(executable),
        detached: true,
        stdio: 'ignore',
        windowsHide: false,
        shell: false
    });

    await new Promise((resolve, reject) => {
        child.once('spawn', resolve);
        child.once('error', error => {
            error.code = error.code || 'UMT_LAUNCH_FAILED';
            reject(error);
        });
    });
    child.unref();

    return {
        launched: true,
        executable,
        dataFile
    };
}

module.exports = {
    DATA_FILE_NAMES,
    validateExecutablePath,
    validateCliExecutablePath,
    resolveGameDataFile,
    createWorkspace,
    launchEditor
};
