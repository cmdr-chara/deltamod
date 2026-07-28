const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const TOML = require('js-toml');
const XML = require('xml-js');
const console = require('./Console');
const { resolveWithin } = require('./security/PathSecurity');
const { writeJsonAtomicSync, readJsonSync } = require('./storage/AtomicStore');

const PATCHER_PATH = process.platform === 'win32'
    ? path.join(__dirname, '..', 'tools', 'g3mtool', 'win-x64', 'G3MTool.exe')
    : path.join(__dirname, '..', 'tools', 'g3mtool', 'linux-x64', 'G3MTool');
const JOURNAL_NAME = '.deltamod-community-patch-journal.json';
const BACKUP_DIRECTORY_NAME = '.deltamod-community-patch-backups';
const SUPPORTED_PATCH_TYPES = new Set(['override', 'copy', 'xdelta', 'g3mpatch']);
const activePatchers = new Set();

function assertPatchFile(filePath, description) {
    const stat = fs.lstatSync(filePath);
    if (!stat.isFile() || stat.isSymbolicLink()) {
        throw new Error(`${description} is not a regular file.`);
    }
    if (stat.nlink > 1) {
        throw new Error(`${description} is a hardlink and cannot be patched safely.`);
    }
}

function patchElements(node, output = []) {
    if (!node || typeof node !== 'object') return output;
    if (node.type === 'element' && node.name === 'patch') output.push(node);
    for (const child of node.elements || []) patchElements(child, output);
    return output;
}

function parsePatches(xml, modName) {
    let document;
    try {
        document = XML.xml2js(xml, { compact: false, trim: true });
    } catch (error) {
        throw new Error(`Mod "${modName}" has invalid modding.xml: ${error.message}`);
    }

    return patchElements(document).map(element => {
        const attributes = element.attributes || {};
        const patch = {
            type: String(attributes.type || '').toLowerCase(),
            patch: String(attributes.patch || ''),
            to: String(attributes.to || ''),
            modName
        };
        if (!SUPPORTED_PATCH_TYPES.has(patch.type)) {
            throw new Error(`Mod "${modName}" uses unsupported patch type "${patch.type}".`);
        }
        if (!patch.patch || !patch.to) {
            throw new Error(`Mod "${modName}" has a patch without both "patch" and "to" paths.`);
        }
        return patch;
    });
}

function loadSelectedMods(modFolder, selectedIds) {
    if (!Array.isArray(selectedIds)) throw new Error('Selected mod IDs must be an array.');
    const selected = new Set(selectedIds.map(String));
    const mods = [];

    for (const folder of fs.readdirSync(modFolder)) {
        const root = resolveWithin(modFolder, folder, { mustExist: true });
        if (!fs.lstatSync(root).isDirectory()) continue;

        const identity = readJsonSync(path.join(root, '__deltaID.json'), null);
        if (!identity?.uniqueId || !selected.has(String(identity.uniqueId))) continue;

        const metadata = TOML.load(fs.readFileSync(path.join(root, 'meta.toml'), 'utf8'));
        const modName = metadata?.metadata?.name || folder;
        let manifestPath = path.join(root, 'modding.xml');
        const variantMarker = path.join(root, '__variant');
        if (fs.existsSync(variantMarker)) {
            const variant = fs.readFileSync(variantMarker, 'utf8').trim();
            manifestPath = resolveWithin(root, variant, { mustExist: true });
        }
        if (!fs.existsSync(manifestPath)) throw new Error(`Mod "${modName}" is missing its patch manifest.`);

        mods.push({
            root,
            folder,
            name: modName,
            uuid: String(identity.uniqueId),
            patches: parsePatches(fs.readFileSync(manifestPath, 'utf8'), modName)
        });
    }
    return mods;
}

function buildPatchPlan(gamePath, modFolder, selectedIds) {
    const gameRoot = path.resolve(gamePath);
    const modRoot = path.resolve(modFolder);
    const mods = loadSelectedMods(modRoot, selectedIds);
    const direct = [];
    const merged = new Map();
    const targetOwners = new Map();

    for (const mod of mods) {
        for (const patch of mod.patches) {
            const source = resolveWithin(mod.root, patch.patch, { mustExist: true });
            const target = resolveWithin(gameRoot, patch.to);
            const targetKey = process.platform === 'win32' ? target.toLowerCase() : target;
            assertPatchFile(source, `Patch source "${patch.patch}"`);
            if (fs.existsSync(target)) assertPatchFile(target, `Patch target "${patch.to}"`);

            if (patch.type === 'override' || patch.type === 'copy') {
                if (targetOwners.has(targetKey)) {
                    throw new Error(`Patch conflict: "${patch.to}" is modified by both "${targetOwners.get(targetKey)}" and "${mod.name}".`);
                }
                targetOwners.set(targetKey, mod.name);
                direct.push({ ...patch, source, target, modId: mod.uuid });
                continue;
            }

            if (targetOwners.has(targetKey)) {
                throw new Error(`Patch conflict: "${patch.to}" has both direct and merge patches.`);
            }
            if (!fs.existsSync(target)) {
                throw new Error(`Merge target "${patch.to}" required by "${mod.name}" does not exist.`);
            }
            if (!merged.has(targetKey)) {
                merged.set(targetKey, { target, relativeTarget: patch.to, patches: [] });
            }
            merged.get(targetKey).patches.push({ ...patch, source, modId: mod.uuid });
        }
    }

    return {
        mods,
        direct,
        merged: [...merged.values()],
        operationCount: direct.length + merged.size
    };
}

function writeJournal(gamePath, journal) {
    writeJsonAtomicSync(path.join(gamePath, JOURNAL_NAME), journal, { backup: false });
}

function backupTarget(target, journal, gamePath) {
    const gameRoot = path.resolve(gamePath);
    const targetRelative = path.relative(gameRoot, target);
    const backupRoot = resolveWithin(gameRoot, path.join(BACKUP_DIRECTORY_NAME, journal.transactionId));
    fs.mkdirSync(path.dirname(target), { recursive: true });
    if (fs.existsSync(target)) {
        const backup = resolveWithin(backupRoot, targetRelative);
        fs.mkdirSync(path.dirname(backup), { recursive: true });
        if (fs.existsSync(backup)) throw new Error(`A transaction backup already exists for ${targetRelative}.`);
        const operation = { type: 'restore', target: targetRelative, backup: targetRelative, state: 'pending' };
        journal.operations.push(operation);
        writeJournal(gamePath, journal);
        fs.renameSync(target, backup);
        operation.state = 'applied';
    } else {
        journal.operations.push({ type: 'remove', target: targetRelative, state: 'applied' });
    }
    writeJournal(gamePath, journal);
}

async function g3mtool(callback, args, gamePath, options = {}) {
    console.log('Running G3MTool with args:', args.join(' '));
    return new Promise((resolve, reject) => {
        const child = spawn(PATCHER_PATH, args, {
            stdio: 'pipe',
            cwd: gamePath,
            windowsHide: true
        });
        activePatchers.add(child);
        let output = '';
        let settled = false;
        const timeout = setTimeout(() => {
            if (!settled) {
                child.kill();
                const error = new Error('G3MTool timed out.');
                error.code = 'PATCHER_TIMEOUT';
                reject(error);
            }
        }, options.timeoutMs || 10 * 60 * 1000);

        const finish = action => {
            if (settled) return;
            settled = true;
            clearTimeout(timeout);
            activePatchers.delete(child);
            action();
        };

        child.stdout.on('data', data => {
            output += data.toString();
            callback(`[G3MTOOL] ${data}`);
        });
        child.stderr.on('data', data => {
            output += data.toString();
            callback(`[G3MTOOL/STDERR] ${data}`);
        });
        child.on('error', error => finish(() => reject(new Error(`Could not start G3MTool: ${error.message}`))));
        child.on('close', code => finish(() => {
            if (code === 0) {
                resolve();
            } else if (output.includes('normally this indicates that the source file is incorrect')) {
                reject(new Error('An xdelta patch was applied to an incompatible source file.'));
            } else {
                reject(new Error(`G3MTool exited with code ${code}.\n${output}`));
            }
        }));
    });
}

async function startGamePatch(gamePath, modFolder, mods, logCallback, progressCallback) {
    let fullLog = '';
    const log = (...args) => {
        const message = args.join(' ');
        console.log(message);
        fullLog += `${message}\n`;
        logCallback?.(message);
    };

    if (!fs.existsSync(PATCHER_PATH)) throw new Error('G3MTool is missing from the tools directory.');
    if (process.platform === 'linux') fs.chmodSync(PATCHER_PATH, 0o755);

    restore(gamePath);
    let plan;
    try {
        plan = buildPatchPlan(gamePath, modFolder, mods);
    } catch (error) {
        return { patched: false, log: error.message, fullLog };
    }

    if (plan.operationCount === 0) {
        progressCallback?.(100);
        return { patched: true, log: '', fullLog };
    }

    const journal = {
        schemaVersion: 1,
        transactionId: `${Date.now()}-${process.pid}`,
        state: 'patching',
        startedAt: new Date().toISOString(),
        operations: []
    };
    writeJournal(gamePath, journal);
    let completed = 0;
    const progress = () => progressCallback?.((completed / plan.operationCount) * 100);

    try {
        for (const patch of plan.direct) {
            log(`Applying ${patch.type} patch from "${patch.modName}" to ${patch.to}.`);
            backupTarget(patch.target, journal, gamePath);
            fs.copyFileSync(patch.source, patch.target);
            completed += 1;
            progress();
        }

        for (const group of plan.merged) {
            log(`Applying ${group.patches.length} merge patch(es) to ${group.relativeTarget}.`);
            backupTarget(group.target, journal, gamePath);
            const backup = resolveWithin(
                gamePath,
                path.join(BACKUP_DIRECTORY_NAME, journal.transactionId, group.relativeTarget),
                { mustExist: true }
            );
            if (group.patches.length > 1) {
                await g3mtool(log, [
                    'patch',
                    'merge',
                    backup,
                    ...group.patches.map(patch => patch.source),
                    '-a',
                    group.target
                ], gamePath);
            } else {
                await g3mtool(log, [
                    'patch',
                    'apply',
                    path.relative(gamePath, backup),
                    group.patches[0].source,
                    path.relative(gamePath, group.target)
                ], gamePath);
            }
            completed += 1;
            progress();
        }

        journal.state = 'patched';
        journal.completedAt = new Date().toISOString();
        writeJournal(gamePath, journal);
        return { patched: true, log: '', fullLog };
    } catch (error) {
        log(`Patching failed: ${error.message}`);
        restore(gamePath);
        return { patched: false, log: error.message, fullLog };
    }
}

function restore(gamePath) {
    if (!gamePath || !fs.existsSync(gamePath)) return;
    const gameRoot = path.resolve(gamePath);
    const journalPath = path.join(gameRoot, JOURNAL_NAME);
    if (!fs.existsSync(journalPath)) return;

    const journal = readJsonSync(journalPath, null);
    if (
        !journal
        || journal.schemaVersion !== 1
        || !/^\d+-\d+$/.test(String(journal.transactionId || ''))
        || !Array.isArray(journal.operations)
    ) {
        throw new Error('The patch recovery journal is invalid; no game files were changed.');
    }

    const backupRoot = resolveWithin(gameRoot, path.join(BACKUP_DIRECTORY_NAME, journal.transactionId));
    for (const operation of [...journal.operations].reverse()) {
        if (!operation || !['restore', 'remove'].includes(operation.type)) {
            throw new Error('The patch recovery journal contains an unknown operation.');
        }
        const target = resolveWithin(gameRoot, operation.target);
        if (operation.type === 'restore') {
            const backup = resolveWithin(backupRoot, operation.backup);
            if (fs.existsSync(backup)) {
                assertPatchFile(backup, `Transaction backup "${operation.backup}"`);
                fs.mkdirSync(path.dirname(target), { recursive: true });
                fs.rmSync(target, { force: true });
                fs.renameSync(backup, target);
            } else if (operation.state !== 'pending') {
                throw new Error(`Transaction backup is missing: ${operation.backup}`);
            }
        } else {
            fs.rmSync(target, { force: true });
        }
        journal.operations.pop();
        writeJournal(gameRoot, journal);
    }

    fs.rmSync(backupRoot, { recursive: true, force: true });
    const backupsParent = path.join(gameRoot, BACKUP_DIRECTORY_NAME);
    try {
        if (fs.readdirSync(backupsParent).length === 0) fs.rmdirSync(backupsParent);
    } catch {}
    fs.rmSync(journalPath, { force: true });
}

function stopOwnedPatchers() {
    for (const child of activePatchers) {
        try { child.kill(); } catch {}
    }
    activePatchers.clear();
}

module.exports = {
    buildPatchPlan,
    startGamePatch,
    restore,
    restoreOriginalsIfAny: restore,
    stopOwnedPatchers
};
