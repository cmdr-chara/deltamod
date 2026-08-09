const fs = require('fs');
const path = require('path');
const os = require('os');
const crypto = require('crypto');
const { spawn, spawnSync } = require('child_process');
const TOML = require('js-toml');
const XML = require('xml-js');
const console = require('./Console');
const { resolveWithin } = require('./security/PathSecurity');
const { validatePatchPlanNative } = require('./security/NativePatchPlanValidation');
const { invoke: invokePatchTransaction, invokeSync: invokePatchTransactionSync } = require('./security/NativePatchTransaction');
const { writeJsonAtomicSync, readJsonSync } = require('./storage/AtomicStore');

const PATCHER_LAYOUTS = Object.freeze({
    'win32-x64': ['win-x64', 'G3MTool.exe'],
    'linux-x64': ['linux-x64', 'G3MTool'],
    'darwin-x64': ['mac-x64', 'G3MTool'],
    'darwin-arm64': ['mac-arm64', 'G3MTool']
});
const JOURNAL_NAME = '.deltamod-community-patch-journal.json';
const BACKUP_DIRECTORY_NAME = '.deltamod-community-patch-backups';
const SUPPORTED_PATCH_TYPES = new Set(['override', 'copy', 'xdelta', 'g3mpatch', 'csx']);
const GAME_DATA_FILE_NAMES = new Set(['data.win', 'game.ios', 'game.unx', 'game.droid']);
const MAX_PLAN_PATCHES = 10_000;
const MAX_PLAN_STRING_BYTES = 32_768;
const UNDERTALE_MOD_CLI_LAYOUTS = Object.freeze({
    'win32-x64': ['win-x64', 'UndertaleModCli.exe'],
    'linux-x64': ['linux-x64', 'UndertaleModCli'],
    'darwin-x64': ['mac-x64', 'UndertaleModCli']
});
const activePatchers = new Set();

function patcherPathFor(platform = process.platform, arch = process.arch) {
    const layout = PATCHER_LAYOUTS[`${platform}-${arch}`];
    if (!layout) {
        throw new Error(`G3MTool is not packaged for ${platform}-${arch}.`);
    }
    return path.join(__dirname, '..', 'tools', 'g3mtool', ...layout);
}

function undertaleModCliPathFor(platform = process.platform, arch = process.arch) {
    const layout = UNDERTALE_MOD_CLI_LAYOUTS[`${platform}-${arch}`];
    if (!layout) {
        throw new Error(`UndertaleModCli is not packaged for ${platform}-${arch}.`);
    }
    return path.join(__dirname, '..', 'tools', 'undertale-mod-tool', ...layout);
}

function assertCsxPlatformSupported(plan, platform = process.platform, arch = process.arch) {
    if (plan.scripts.length > 0) undertaleModCliPathFor(platform, arch);
}

function assertCsxRuntimeAvailable(plan, options = {}) {
    if (plan.scripts.length === 0) return;
    assertCsxPlatformSupported(plan, options.platform || process.platform, options.arch || process.arch);
    const executable = options.undertaleModCliPath || undertaleModCliPathFor(
        options.platform || process.platform,
        options.arch || process.arch
    );
    if (!fs.existsSync(executable)) throw new Error('UndertaleModCli is missing from the tools directory.');
    assertPatchFile(executable, 'UndertaleModCli executable');
}

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

function buildPatchPlan(gamePath, modFolder, selectedIds, options = {}) {
    const gameRoot = path.resolve(gamePath);
    const modRoot = path.resolve(modFolder);
    const mods = loadSelectedMods(modRoot, selectedIds);
    const mapTarget = typeof options.mapPatchTarget === 'function'
        ? options.mapPatchTarget
        : value => value;
    const direct = [];
    const merged = new Map();
    const scripts = new Map();
    const targetOwners = new Map();
    const candidates = [];

    for (const mod of mods) {
        for (const patch of mod.patches) {
            const source = resolveWithin(mod.root, patch.patch, { mustExist: true });
            const mappedTarget = mapTarget(patch.to);
            const target = resolveWithin(gameRoot, mappedTarget);
            const candidate = {
                type: patch.type,
                patch: patch.patch,
                to: patch.to,
                mappedTarget,
                modName: mod.name,
                modId: mod.uuid,
                modRoot: mod.root
            };
            if (candidates.length >= MAX_PLAN_PATCHES || Object.values(candidate).some(value => (
                typeof value !== 'string' || !value || Buffer.byteLength(value) > MAX_PLAN_STRING_BYTES || value.includes('\0')
            ))) {
                throw new Error('Patch plan exceeds the supported protocol limits.');
            }
            candidates.push(candidate);
            const targetKey = process.platform === 'win32' ? target.toLowerCase() : target;
            assertPatchFile(source, `Patch source "${patch.patch}"`);
            if (fs.existsSync(target)) assertPatchFile(target, `Patch target "${patch.to}"`);

            if (patch.type === 'csx') {
                if (path.extname(source).toLowerCase() !== '.csx') {
                    throw new Error(`CSX patch "${patch.patch}" from "${mod.name}" must use the .csx extension.`);
                }
                if (!GAME_DATA_FILE_NAMES.has(path.basename(target).toLowerCase())) {
                    throw new Error(`CSX patch from "${mod.name}" must target a supported GameMaker data file.`);
                }
                if (!fs.existsSync(target)) {
                    throw new Error(`CSX target "${patch.to}" required by "${mod.name}" does not exist.`);
                }
                if (targetOwners.has(targetKey) || merged.has(targetKey)) {
                    throw new Error(`Patch conflict: "${patch.to}" has both CSX and non-CSX patches.`);
                }
                if (!scripts.has(targetKey)) {
                    scripts.set(targetKey, { target, relativeTarget: mappedTarget, patches: [] });
                }
                scripts.get(targetKey).patches.push({
                    ...patch,
                    source,
                    sourceHash: sha256File(source),
                    modRoot: mod.root,
                    modTreeHash: treeSha256(mod.root),
                    relativeSource: path.relative(mod.root, source),
                    modId: mod.uuid
                });
                continue;
            }

            if (patch.type === 'override' || patch.type === 'copy') {
                if (targetOwners.has(targetKey)) {
                    throw new Error(`Patch conflict: "${patch.to}" is modified by both "${targetOwners.get(targetKey)}" and "${mod.name}".`);
                }
                if (merged.has(targetKey) || scripts.has(targetKey)) {
                    throw new Error(`Patch conflict: "${patch.to}" has both direct and non-direct patches.`);
                }
                targetOwners.set(targetKey, mod.name);
                direct.push({ ...patch, source, target, mappedTarget, modId: mod.uuid });
                continue;
            }

            if (targetOwners.has(targetKey) || scripts.has(targetKey)) {
                throw new Error(`Patch conflict: "${patch.to}" has both direct and merge patches.`);
            }
            if (!fs.existsSync(target)) {
                throw new Error(`Merge target "${patch.to}" required by "${mod.name}" does not exist.`);
            }
            if (!merged.has(targetKey)) {
                merged.set(targetKey, { target, relativeTarget: mappedTarget, patches: [] });
            }
            merged.get(targetKey).patches.push({ ...patch, source, modId: mod.uuid });
        }
    }

    const plan = {
        mods,
        direct,
        merged: [...merged.values()],
        scripts: [...scripts.values()],
        operationCount: direct.length + merged.size + scripts.size
    };
    Object.defineProperty(plan, '_nativeValidationRequest', {
        value: {
            schemaVersion: 1,
            gameRoot,
            platform: process.platform === 'win32' ? 'win32' : process.platform === 'darwin' ? 'darwin' : 'linux',
            patches: candidates
        },
        enumerable: false
    });
    return plan;
}

function validatePatchPlanFallback(plan) {
    if (!plan || !Array.isArray(plan.direct) || !Array.isArray(plan.merged)
        || !Array.isArray(plan.scripts) || !Number.isSafeInteger(plan.operationCount)
        || plan.operationCount < 0) {
        throw new Error('Patch plan is invalid.');
    }
    return { operationCount: plan.operationCount };
}

async function approvePatchPlan(plan, options = {}) {
    const request = plan?._nativeValidationRequest;
    if (!request) return validatePatchPlanFallback(plan);
    const validation = (options.validatePatchPlanNative || validatePatchPlanNative)(request, {
        sidecarPath: options.patchPlanWorkerPath
    });
    if (validation === null) return validatePatchPlanFallback(plan);
    const approval = await validation;
    if (approval.operationCount !== plan.operationCount || approval.patchCount !== request.patches.length) {
        const error = new Error('Native patch-plan validation returned counts that do not match the candidate plan.');
        error.code = 'PATCH_PLAN_NATIVE_FAILED';
        throw error;
    }
    return approval;
}

function recheckPlanFiles(plan) {
    for (const patch of plan.direct) {
        assertPatchFile(patch.source, `Patch source "${patch.patch}"`);
        if (fs.existsSync(patch.target)) assertPatchFile(patch.target, `Patch target "${patch.to}"`);
    }
    for (const group of [...plan.merged, ...plan.scripts]) {
        assertPatchFile(group.target, `Patch target "${group.relativeTarget}"`);
        for (const patch of group.patches) assertPatchFile(patch.source, `Patch source "${patch.patch}"`);
    }
}

function treeSha256(directory) {
    const entries = [];
    const visit = (current, relative = '') => {
        for (const entry of fs.readdirSync(current, { withFileTypes: true }).sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)) {
            const absolute = path.join(current, entry.name);
            const childRelative = path.posix.join(relative, entry.name);
            const stat = fs.lstatSync(absolute);
            if (stat.isSymbolicLink()) throw new Error(`Script resources contain a symbolic link: ${childRelative}`);
            if (stat.isDirectory()) visit(absolute, childRelative);
            else if (stat.isFile()) entries.push(`${childRelative}\0${sha256File(absolute)}\n`);
            else throw new Error(`Script resources contain an unsupported file: ${childRelative}`);
        }
    };
    visit(directory);
    return crypto.createHash('sha256').update(entries.join('')).digest('hex');
}

function copyTreeSnapshot(source, destination) {
    fs.mkdirSync(destination, { recursive: true });
    for (const entry of fs.readdirSync(source, { withFileTypes: true })) {
        const from = path.join(source, entry.name);
        const to = path.join(destination, entry.name);
        const stat = fs.lstatSync(from);
        if (stat.isSymbolicLink()) throw new Error(`Script resources contain a symbolic link: ${entry.name}`);
        if (stat.isDirectory()) copyTreeSnapshot(from, to);
        else if (stat.isFile()) fs.copyFileSync(from, to, fs.constants.COPYFILE_EXCL);
        else throw new Error(`Script resources contain an unsupported file: ${entry.name}`);
    }
}

function sha256File(filePath) {
    return crypto.createHash('sha256').update(fs.readFileSync(filePath)).digest('hex');
}

function writeJournal(gamePath, journal) {
    writeJsonAtomicSync(path.join(gamePath, JOURNAL_NAME), journal, { backup: false });
}

function backupTargetFallback(target, journal, gamePath) {
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
    const patcherPath = patcherPathFor();
    console.log('Running G3MTool with args:', args.join(' '));
    return new Promise((resolve, reject) => {
        const child = spawn(patcherPath, args, {
            stdio: ['ignore', 'pipe', 'pipe'],
            cwd: gamePath,
            windowsHide: true,
            shell: false,
            detached: process.platform !== 'win32'
        });
        activePatchers.add(child);
        let output = '';
        let settled = false;
        const timeout = setTimeout(() => {
            if (!settled) {
                (options.terminateProcessTree || terminateProcessTree)(child);
                const error = new Error('G3MTool timed out.');
                error.code = 'PATCHER_TIMEOUT';
                finish(() => reject(error));
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

async function backupTarget(target, journal, gamePath, options = {}) {
    const relativeTarget = path.relative(path.resolve(gamePath), target).split(path.sep).join('/');
    const native = await invokePatchTransaction('backup', gamePath, journal, { sidecarPath: options.patchTransactionWorkerPath }, relativeTarget);
    if (native) {
        const updated = readJsonSync(path.join(gamePath, JOURNAL_NAME), null);
        if (!updated) throw new Error('Native patch transaction did not produce a journal.');
        Object.assign(journal, updated);
        return;
    }
    backupTargetFallback(target, journal, gamePath);
}

function childEnvironment() {
    return Object.fromEntries(Object.entries(process.env).filter(([name]) => (
        !/(?:TOKEN|SECRET|PASSWORD|PASSWD|API_KEY|AUTHORIZATION|COOKIE)/i.test(name)
    )));
}

function terminateProcessTree(child, platform = process.platform) {
    if (!child || child.exitCode !== null) return;
    try {
        if (platform === 'win32' && Number.isSafeInteger(child.pid)) {
            const result = spawnSync('taskkill', ['/pid', String(child.pid), '/t', '/f'], {
                stdio: 'ignore',
                windowsHide: true,
                timeout: 15_000
            });
            if (!result.error && result.status === 0) return;
        } else if (Number.isSafeInteger(child.pid)) {
            process.kill(-child.pid, 'SIGKILL');
            return;
        }
    } catch {}
    try { child.kill('SIGKILL'); } catch {}
}

async function runUndertaleModScripts(group, stagingRoot, log, options = {}) {
    const executable = options.undertaleModCliPath || undertaleModCliPathFor();
    if (!fs.existsSync(executable)) {
        throw new Error('UndertaleModCli is missing from the tools directory.');
    }
    assertPatchFile(executable, 'UndertaleModCli executable');
    if (process.platform !== 'win32') fs.chmodSync(executable, 0o755);

    const targetName = path.basename(group.target);
    const input = path.join(stagingRoot, `input-${crypto.randomUUID()}-${targetName}`);
    const output = path.join(stagingRoot, `output-${crypto.randomUUID()}-${targetName}`);
    fs.copyFileSync(group.target, input, fs.constants.COPYFILE_EXCL);
    const modSnapshots = new Map();
    const stagedScripts = group.patches.map(patch => {
        let modSnapshot = modSnapshots.get(patch.modRoot);
        if (!modSnapshot) {
            modSnapshot = path.join(stagingRoot, `mod-${modSnapshots.size}-${crypto.randomUUID()}`);
            copyTreeSnapshot(patch.modRoot, modSnapshot);
            if (treeSha256(modSnapshot) !== patch.modTreeHash) {
                throw new Error(`Mod resources for CSX patch "${patch.patch}" changed after they were approved.`);
            }
            modSnapshots.set(patch.modRoot, modSnapshot);
        }
        const staged = resolveWithin(modSnapshot, patch.relativeSource, { mustExist: true });
        if (sha256File(staged) !== patch.sourceHash) {
            throw new Error(`CSX patch "${patch.patch}" changed after it was approved.`);
        }
        return staged;
    });
    const args = [
        'load', input,
        '--verbose',
        '--output', output,
        '--scripts', ...stagedScripts
    ];

    log(`Running ${group.patches.length} UndertaleModTool script(s) for ${group.relativeTarget}.`);
    await new Promise((resolve, reject) => {
        const child = (options.spawnImpl || spawn)(executable, args, {
            stdio: ['ignore', 'pipe', 'pipe'],
            cwd: path.dirname(executable),
            windowsHide: true,
            shell: false,
            detached: process.platform !== 'win32',
            env: childEnvironment()
        });
        activePatchers.add(child);
        let outputLog = '';
        let settled = false;
        const append = (prefix, data) => {
            const text = data.toString();
            outputLog = `${outputLog}${text}`.slice(-1024 * 1024);
            log(`${prefix}${text}`);
        };
        const finish = action => {
            if (settled) return;
            settled = true;
            clearTimeout(timeout);
            activePatchers.delete(child);
            action();
        };
        const timeout = setTimeout(() => {
            if (settled) return;
            (options.terminateProcessTree || terminateProcessTree)(child);
            finish(() => reject(new Error('UndertaleModCli script execution timed out.')));
        }, options.timeoutMs || 10 * 60 * 1000);
        child.stdout.on('data', data => append('[UTMT] ', data));
        child.stderr.on('data', data => append('[UTMT/STDERR] ', data));
        child.on('error', error => finish(() => reject(new Error(`Could not start UndertaleModCli: ${error.message}`))));
        child.on('exit', code => finish(() => {
            if (code === 0) resolve();
            else reject(new Error(`UndertaleModCli exited with code ${code}.\n${outputLog}`));
        }));
    });

    if (!fs.existsSync(output)) throw new Error('UndertaleModCli did not produce a patched data file.');
    assertPatchFile(output, 'UndertaleModCli output');
    return output;
}

async function startGamePatch(gamePath, modFolder, mods, logCallback, progressCallback, options = {}) {
    let fullLog = '';
    const log = (...args) => {
        const message = args.join(' ');
        console.log(message);
        fullLog += `${message}\n`;
        logCallback?.(message);
    };

    restore(gamePath);
    let plan;
    try {
        plan = options.approvedPlan || buildPatchPlan(gamePath, modFolder, mods, options);
        await approvePatchPlan(plan, options);
        recheckPlanFiles(plan);
        assertCsxRuntimeAvailable(plan, options);
    } catch (error) {
        return { patched: false, log: error.message, fullLog };
    }

    if (plan.merged.length > 0) {
        const patcherPath = patcherPathFor();
        if (!fs.existsSync(patcherPath)) throw new Error('G3MTool is missing from the tools directory.');
        if (process.platform !== 'win32') fs.chmodSync(patcherPath, 0o755);
    }

    if (plan.operationCount === 0) {
        progressCallback?.(100);
        return { patched: true, log: '', fullLog };
    }

    const scriptStagingRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'deltamod-csx-'));
    const stagedScripts = [];
    try {
        for (const group of plan.scripts) {
            stagedScripts.push({
                group,
                output: await runUndertaleModScripts(group, scriptStagingRoot, log, options)
            });
        }
    } catch (error) {
        fs.rmSync(scriptStagingRoot, { recursive: true, force: true });
        return { patched: false, log: error.message, fullLog };
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
            assertPatchFile(patch.source, `Patch source "${patch.patch}"`);
            if (fs.existsSync(patch.target)) assertPatchFile(patch.target, `Patch target "${patch.to}"`);
            log(`Applying ${patch.type} patch from "${patch.modName}" to ${patch.to}.`);
            await backupTarget(patch.target, journal, gamePath, options);
            fs.copyFileSync(patch.source, patch.target);
            completed += 1;
            progress();
        }

        for (const group of plan.merged) {
            assertPatchFile(group.target, `Patch target "${group.relativeTarget}"`);
            for (const patch of group.patches) assertPatchFile(patch.source, `Patch source "${patch.patch}"`);
            log(`Applying ${group.patches.length} merge patch(es) to ${group.relativeTarget}.`);
            await backupTarget(group.target, journal, gamePath, options);
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
                ], gamePath, options);
            } else {
                await g3mtool(log, [
                    'patch',
                    'apply',
                    path.relative(gamePath, backup),
                    group.patches[0].source,
                    path.relative(gamePath, group.target)
                ], gamePath, options);
            }
            completed += 1;
            progress();
        }

        for (const staged of stagedScripts) {
            assertPatchFile(staged.group.target, `Patch target "${staged.group.relativeTarget}"`);
            assertPatchFile(staged.output, 'UndertaleModCli output');
            log(`Committing ${staged.group.patches.length} CSX script patch(es) to ${staged.group.relativeTarget}.`);
            await backupTarget(staged.group.target, journal, gamePath, options);
            fs.copyFileSync(staged.output, staged.group.target);
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
    } finally {
        fs.rmSync(scriptStagingRoot, { recursive: true, force: true });
    }
}

function restoreFallback(gamePath) {
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
    const targets = new Set();
    const backups = new Set();
    for (const operation of journal.operations) {
        if (!operation || !['restore', 'remove'].includes(operation.type)
            || (operation.state !== undefined && !['pending', 'applied'].includes(operation.state))
            || typeof operation.target !== 'string' || !operation.target
            || (operation.type === 'restore' && typeof operation.backup !== 'string')
            || (operation.type === 'remove' && operation.backup !== undefined)) {
            throw new Error('The patch recovery journal contains an unknown operation.');
        }
        const target = resolveWithin(gameRoot, operation.target);
        const targetKey = process.platform === 'win32' ? target.toLowerCase() : target;
        if (targets.has(targetKey)) throw new Error('The patch recovery journal contains conflicting operations.');
        targets.add(targetKey);
        if (operation.type === 'restore') {
            const backup = resolveWithin(backupRoot, operation.backup);
            const backupKey = process.platform === 'win32' ? backup.toLowerCase() : backup;
            if (backups.has(backupKey)) throw new Error('The patch recovery journal contains conflicting backups.');
            backups.add(backupKey);
        }
    }
    for (const operation of [...journal.operations].reverse()) {
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

function restore(gamePath) {
    if (!gamePath || !fs.existsSync(gamePath)) return;
    const journalPath = path.join(path.resolve(gamePath), JOURNAL_NAME);
    if (!fs.existsSync(journalPath)) return;
    const journal = readJsonSync(journalPath, null);
    const native = invokePatchTransactionSync('restore', gamePath, journal || {}, {});
    if (native) return;
    restoreFallback(gamePath);
}

function stopOwnedPatchers() {
    for (const child of activePatchers) {
        terminateProcessTree(child);
    }
    activePatchers.clear();
}

module.exports = {
    buildPatchPlan,
    approvePatchPlan,
    validatePatchPlanFallback,
    patcherPathFor,
    undertaleModCliPathFor,
    assertCsxPlatformSupported,
    assertCsxRuntimeAvailable,
    terminateProcessTree,
    startGamePatch,
    restore,
    restoreFallback,
    restoreOriginalsIfAny: restore,
    stopOwnedPatchers
};
