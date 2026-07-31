const path = require('path');
const system = require('./System');
const fs = require('fs');
const os = require('os');
const console = require('./Console');
const { extractArchiveAtomic } = require('./security/ArchiveSecurity');
const { randomString, page, shopClang } = require('./Utils');
const crypto = require('crypto');
const { dialog } = require('electron');
const TOML = require('js-toml');
const { resolveWithin } = require('./security/PathSecurity');
const { downloadToFile } = require('./security/RemoteSecurity');
const { loadHashCache, hashGameFile, saveHashCache } = require('./storage/GameHashCache');

const computerName = os.hostname();

async function downloadModFromURL(url, onProgress, mID, mModel, downloadOptions = {}) {
    let downloadedBytes = 0;
    let totalBytes = 0;
    const filePath = path.join(system.getTemporary(), `${crypto.randomUUID()}.modarchive`);
    try {
        await downloadToFile(url, filePath, {
            maximumBytes: downloadOptions.maximumBytes || 2 * 1024 * 1024 * 1024,
            allowedHosts: downloadOptions.allowedHosts,
            headers: downloadOptions.headers,
            onProgress: ({ completed, total }) => {
                downloadedBytes = completed;
                totalBytes = total;
                const percentage = total > 0 ? (completed / total) * 100 : 0;
                console.log(`Download progress: ${percentage.toFixed(1)}%`);
                onProgress?.(percentage, completed, { phase: 'download', total });
            }
        });
        onProgress?.(100, downloadedBytes, { phase: 'import', total: totalBytes });
        const imported = await importMod(filePath, "donothing", mID, mModel);
        if (imported !== true) throw new Error('The downloaded mod was not imported.');
        onProgress?.(100, downloadedBytes, { phase: 'complete', total: totalBytes });
        return true;
    } finally {
        try { await fs.promises.rm(filePath, { force: true }); } catch {}
    }
}

async function importMod(filePath, nextPage = "main", mID = null, mModel = null) {
    var clangit = true;

    console.log("Importing mod (gb info)", mID, mModel, "from file:", filePath);
    // create unique mod folder
    const modPath = path.join(system.getPacketDatabase(), "Mod_" + randomString(32));
    try {
        await extractArchiveAtomic(filePath, modPath);
        // I (mc) believe that we shouldn't delete a user's files if we did not create/download them ourselves
        // I (techy) agree with mc
        // fs.unlinkSync (filePath); // delete the zip file after extraction, I (Zork) commented this out temporarily to keep the zip file for debugging.

        // Flatten if extracted into a single subfolder
        const contents = fs.readdirSync(modPath);
        if (contents.length === 1) {
            const singleItem = path.join(modPath, contents[0]);
            const stats = fs.statSync(singleItem);
            if (stats.isDirectory()) {
                const tempDir = path.join(system.getPacketDatabase(), "Mod_" + randomString(32));
                fs.renameSync(singleItem, tempDir);
                fs.rmdirSync(modPath);
                fs.renameSync(tempDir, modPath);
            }
        }

        // Legacy support: rename _deltamodInfo.json to meta.json if needed
        if (fs.existsSync(path.join(modPath, '_deltamodInfo.json'))) {
            fs.copyFileSync(path.join(modPath, '_deltamodInfo.json'), path.join(modPath, 'meta.json'));
            fs.unlinkSync(path.join(modPath, '_deltamodInfo.json'));
        }
        if (fs.existsSync(path.join(modPath, '_icon.png'))) {
            fs.copyFileSync(path.join(modPath, '_icon.png'), path.join(modPath, 'icon.png'));
            fs.unlinkSync(path.join(modPath, '_icon.png'));
        }

        if (fs.existsSync(path.join(modPath, 'meta.json')) && !fs.existsSync(path.join(modPath, 'meta.toml'))) {
            console.log("Converting meta.json to meta.toml for mod at:", modPath);
            var jsonModInfo = safeReadJSON(path.join(modPath, 'meta.json'));

            // some toml converting things (move color from metadata to root)
            var metaColor = jsonModInfo?.metadata?.color;
            if (metaColor) {
                delete jsonModInfo.metadata.color;
                jsonModInfo.color = metaColor;
            }
            
            var toml = TOML.dump(jsonModInfo);
            fs.writeFileSync(path.join(modPath, 'meta.toml'), toml, 'utf8');
            fs.unlinkSync(path.join(modPath, 'meta.json')); // delete the old JSON manifest
        }

        // Check manifest anywhere in the tree (now usually at root after flatten)
        const manifestPath = findFirstByName(modPath, 'meta.toml') || path.join(modPath, 'meta.toml');
        if (!fs.existsSync(manifestPath)) {
            fs.rmSync(modPath, { recursive: true, force: true });
            throw new Error('Mod TOML manifest not found. Please ensure the mod is properly packaged.');
        }

        var modInfo = safeReadTOML(manifestPath);
        if (!modInfo || !modInfo.metadata) {
            fs.rmSync(modPath, { recursive: true, force: true });
            throw new Error('Invalid mod manifest. Please ensure meta.toml is correctly formatted.');
        }

        var moddingXMLPath = path.join(modPath, 'modding.xml');
        console.log("Checking for modding.xml at:", moddingXMLPath);
        if (!fs.existsSync(moddingXMLPath)) {
            throw new Error('Modding XML file not found. Please ensure modding.xml is included in the mod package.');
        }

        modInfo.metadata.packageID = validatePID(modInfo.metadata.packageID);
        fs.writeFileSync(path.join(modPath, 'meta.toml'), TOML.dump(modInfo), 'utf8');

        if (mID && typeof mID === 'object') {
            const source = mID;
            if (!/^(gamebanana|nexus|moddb)$/.test(String(source.provider || ''))) {
                throw new Error('Invalid mod source metadata.');
            }
            modInfo.metadata.source_provider = String(source.provider);
            modInfo.metadata.source_id = String(source.id || '').slice(0, 100);
            if (source.fileId != null) modInfo.metadata.source_file_id = String(source.fileId).slice(0, 100);
            if (typeof source.url === 'string' && source.url.startsWith('https://')) {
                modInfo.metadata.source_url = source.url.slice(0, 1000);
            }
            fs.writeFileSync(path.join(modPath, 'meta.toml'), TOML.dump(modInfo), 'utf8');
        }
        else if (mID && mModel) {
            modInfo.metadata.gamebanana_id = mID;
            modInfo.metadata.gamebanana_model = mModel;
            fs.writeFileSync(path.join(modPath, 'meta.toml'), TOML.dump(modInfo), 'utf8');
        }

        if (modInfo.metadata.demoMod !== undefined) {
            modInfo.metadata.game = (modInfo.metadata.demoMod ? "toby.deltarune.demo" : "toby.deltarune");
            delete modInfo.metadata.demoMod;
            fs.writeFileSync(path.join(modPath, 'meta.toml'), TOML.dump(modInfo), 'utf8');
        }
        else if (modInfo.metadata.demoMod === undefined && modInfo.metadata.game === undefined) {
            fs.rmSync(modPath, { recursive: true, force: true });
            throw new Error('Mod TOML manifest is missing required field `game` (no demoMod to determine game).');
        }


        if (modInfo.metadata.packageID?.toString().trim() && modInfo.metadata.packageID.toString().trim() != "und.und.und") {
            if (fs.existsSync(path.join(system.getPacketDatabase(), modInfo.metadata.packageID)) && modInfo.metadata.packageID != "und.und.und") {
                clangit = false;
                var existingModInfo = safeReadTOML(path.join(system.getPacketDatabase(), modInfo.metadata.packageID, 'meta.toml'));
                var oldVersion = existingModInfo?.metadata?.version || "Unknown";
                var newVersion = modInfo.metadata.version || "Unknown";
                
                var response = dialog.showMessageBoxSync({
                    type: 'error',
                    title: 'Import Failed',
                    message: `The mod "${modInfo.metadata.name}" is already present in your mods.\n\nPresent version: ${oldVersion}\nTo be imported version: ${newVersion}\n\nHow would you like to proceed?`,
                    buttons: ['Delete old version', 'Keep old version', 'Cancel import'],
                    defaultId: 0,
                    cancelId: 2,
                });

                if (response == 0) {
                    fs.rmSync(path.join(system.getPacketDatabase(), modInfo.metadata.packageID), { recursive: true, force: true });
                } else if (response == 1) {
                    fs.rmSync(modPath, { recursive: true, force: true });
                     if (nextPage && nextPage !== "donothing") page(nextPage);
                    return false;
                } else {
                    fs.rmSync(modPath, { recursive: true, force: true });
                    if (nextPage && nextPage !== "donothing") page(nextPage);
                    return false;
                }
            }
            fs.renameSync(modPath, path.join(system.getPacketDatabase(), modInfo.metadata.packageID));
        }


        /*await dialog.showMessageBox(win, {
            type: 'info',
            title: 'Import Successful',
            message: 'Mod imported successfully.',
            buttons: ['OK']
        });*/

        if (nextPage && nextPage !== "donothing") page(nextPage);

        if (clangit) {
            shopClang();
        }
        return true;

        // Simple way to refresh the list
        // app.relaunch(properRelaunch());
        // app.exit();
        // process.exit();
    } catch (err) {
        console.error('Error importing mod:', err);
        dialog.showErrorBox('Import failed', String(err) + "\nThe mod was not imported.");
        // clean up
        try {
            fs.rmSync(modPath, { recursive: true, force: true });
        }
        catch (_) {
            console.warn('Failed to clean up mod folder after failed import:', modPath);
        }
        return false;
    }
}

function removeModSafe(modid) {
    let modPath;
    try {
        modPath = resolveModFolder(modid, true);
    } catch (error) {
        console.warn(`Refusing unsafe mod removal request: ${error.message}`);
        return false;
    }

    // make sure that what we're deleting is actually a mod and not a random folder
    if (fs.existsSync(path.join(modPath, "__deltaID.json")) && fs.existsSync(modPath)) {
        console.log("Deleting mod", modPath);
        fs.rmSync(modPath, { recursive: true });
    } else {
        console.warn("Error: Mod", modPath, "doesn't seem to be a valid mod with a __deltaID.json.");
        return false;
    }

    page("");
    return true;
}

// [ADDED] depth-first search for a file by name anywhere under root
function findFirstByName(root, fileName) {
    const needle = String(fileName).toLowerCase();
    const stack = [root];
    while (stack.length) {
        const dir = stack.pop();
        let ents;
        try { ents = fs.readdirSync(dir, { withFileTypes: true }); } catch { continue; }
        for (const e of ents) {
            const full = path.join(dir, e.name);
            if (e.isFile() && e.name.toLowerCase() === needle) return full;
            if (e.isDirectory()) stack.push(full);
        }
    }
    return null;
}

function safeReadJSON(p) {
    if (!p) return null;
    try { return JSON.parse(fs.readFileSync(p, 'utf8')); } catch { return null; }
}

function safeReadTOML(p) {
    if (!p) return null;
    try { return TOML.load(fs.readFileSync(p, 'utf8')); } catch { return null; }
}

function validatePID(pid) {
    console.log("Validating packageID:", pid);
    if (typeof pid !== 'string') return "und.und.und";
    const normalized = pid.trim().toLowerCase();
    if (
        normalized.length > 191
        || !/^[a-z0-9][a-z0-9_-]{0,62}(?:\.[a-z0-9][a-z0-9_-]{0,62}){2}$/.test(normalized)
    ) {
        return "und.und.und";
    }
    return normalized;
}

function resolveModFolder(modid, mustExist = false) {
    if (typeof modid !== 'string' || path.basename(modid) !== modid || !modid.trim()) {
        const error = new Error('Invalid mod folder identifier.');
        error.code = 'INVALID_MOD_ID';
        throw error;
    }
    const resolved = resolveWithin(system.getPacketDatabase(), modid, { mustExist });
    if (mustExist && fs.lstatSync(resolved).isSymbolicLink()) {
        const error = new Error('Linked mod folders are not allowed.');
        error.code = 'MOD_LINK_BLOCKED';
        throw error;
    }
    return resolved;
}

function howmany() {
    return fs.readdirSync(system.getPacketDatabase()).length;
}

function modList() {
    var mods = fs.readdirSync(system.getPacketDatabase());
    var modList = [];
    var errors = [];
    var uniqueIdSet = new Set(); // actually use it
    const gameRoot = system.getSystemFolder('deltaruneInstall');
    const hashCachePath = system.getSystemFile('_game-hashes.json', false);
    const hashCache = loadHashCache(hashCachePath);
    let hashCacheDirty = false;

    var failureReason = "";

    for (var mod of mods) {
        try {
            failureReason = "Unknown. Contact a developer!";
            var modPath = resolveModFolder(mod, true);
            
            if (fs.existsSync(path.join(modPath, '_deltamodInfo.json'))) {
                fs.copyFileSync(path.join(modPath, '_deltamodInfo.json'), path.join(modPath, 'meta.json'));
                fs.unlinkSync(path.join(modPath, '_deltamodInfo.json'));
            }
            if (fs.existsSync(path.join(modPath, '_icon.png'))) {
                fs.copyFileSync(path.join(modPath, '_icon.png'), path.join(modPath, 'icon.png'));
                fs.unlinkSync(path.join(modPath, '_icon.png'));
            };

            // Zork's Patch: Find manifest anywhere in the mod folder, not only at root (safe)
            const jsonManifestPath =
                findFirstByName(modPath, 'meta.json') ||
                path.join(modPath, 'meta.json');

            const tomlManifestPath =
                findFirstByName(modPath, 'meta.toml') ||
                path.join(modPath, 'meta.toml');


            if (fs.existsSync(jsonManifestPath) && !fs.existsSync(tomlManifestPath)) {
                console.log("Converting meta.json to meta.toml for mod:", mod);
                var jsonModInfo = safeReadJSON(jsonManifestPath);

                // some toml converting things (move color from metadata to root)
                var metaColor = jsonModInfo?.metadata?.color;
                if (metaColor) {
                    delete jsonModInfo.metadata.color;
                    jsonModInfo.color = metaColor;
                }

                var toml = TOML.dump(jsonModInfo);
                fs.writeFileSync(tomlManifestPath, toml, 'utf8');

                fs.unlinkSync(jsonManifestPath); // delete the old JSON manifest
            }

            var modInfo = safeReadTOML(tomlManifestPath) || null;
            if (!modInfo || !modInfo.metadata) {
                failureReason = "Failure reading meta.toml.";
                throw new Error('Failure reading meta.toml.');
            }
            var meta = modInfo.metadata || {};
            meta.isIncompatible = false;

            var moddingXMLPath = path.join(modPath, 'modding.xml');
            console.log("Checking for modding.xml at:", moddingXMLPath);
            if (!fs.existsSync(moddingXMLPath)) {
                failureReason = "Modding XML file not found. Please ensure modding.xml is included in the mod package.";
                throw new Error('Modding XML file not found. Please ensure modding.xml is included in the mod package.');
            }


            if (meta.packageID && meta.packageID.toString().trim().toLowerCase() === "..") {
                meta.packageID = "und.und.und"; // prevent directory traversal
                modInfo.metadata.packageID = "und.und.und"; // prevent directory traversal
            }

            if (meta.packageID && meta.packageID.toString().trim().split('.').length === 3) {
                console.log('detected valid pid for mod', mod, ':', meta.packageID);
                meta.packageID = validatePID(meta.packageID);

                if (modPath !== path.join(system.getPacketDatabase(), meta.packageID) && meta.packageID != "und.und.und") {
                    console.log('upgrading modstore to have folder named by packageID for mod', mod);
                    fs.renameSync(modPath, path.join(system.getPacketDatabase(), meta.packageID));
                    modPath = path.join(system.getPacketDatabase(), meta.packageID);
                }
            }

            try {
                if (meta.demoMod !== undefined) {
                    console.log("Upgrading demoMod field to game field for mod:", mod);
                    meta.game = (meta.demoMod ? "toby.deltarune.demo" : "toby.deltarune");
                    delete meta.demoMod;
                    fs.writeFileSync(path.join(modPath, 'meta.toml'), TOML.dump(modInfo), 'utf8');
                }
            }
            catch {
                console.log("Failed to upgrade demoMod field for mod:", mod);
            }

            try {
                meta.packageID = validatePID(meta.packageID) || "und.und.und";
            }
            catch {
                meta.packageID = "und.und.und";
            }
            const pid = meta.packageID;

            if (require('./KeyValue').readUniqueFlag('HASHCHECKS')) {
                modInfo.neededFiles?.forEach(file => {
                    try {
                        if (!file || typeof file.file !== 'string' || !/^[a-f0-9]{64}$/i.test(String(file.checksum || ''))) {
                            throw new Error('Invalid neededFiles entry.');
                        }
                        const result = hashGameFile(gameRoot, file.file, hashCache);
                        hashCacheDirty ||= result.updated;
                        const fileContentsHash = result.sha256;

                        console.log('CHECK FILES! ' + file.checksum + ' VS ' + fileContentsHash);
                        if (file.checksum.toLowerCase() !== fileContentsHash.toLowerCase()) {
                            meta._incompatibleHASH = true;
                            meta._hashDifferentFiles = meta._hashDifferentFiles || [];
                            meta._hashDifferentFiles.push(file.file);
                        }
                    }
                    catch {
                        meta._incompatibleHASH = true;
                    }
                }); // future use
            }

            const idPath = findFirstByName(modPath, '__deltaID.json') || path.join(modPath, '__deltaID.json');
            failureReason = "Failed to read __deltaID JSON.";
            let deltamodExclusive = safeReadJSON(idPath);

            failureReason = "Failed to generate an UINTID for the mod.";

            if (meta.game == 'toby.deltarune.demo' && meta.isForLTS == true) {
                meta.game = 'toby.deltarune.demolts';
                var modInfoCP = modInfo; // avoid mutating the original modInfo in case of errors
                modInfoCP.metadata.game = 'toby.deltarune.demolts';
                delete modInfoCP.metadata.isForLTS;
                delete modInfoCP.metadata.isIncompatible;
                delete modInfoCP.metadata._incompatibleHASH;
                fs.writeFileSync(manifestPath, JSON.stringify(modInfoCP, null, 2), 'utf8');
            }


            if (meta.game == 'toby.deltarune.demo' && meta.isForLTS == undefined && !fs.existsSync(path.join(modPath, '_democheck'))) {
                var modXML = fs.readFileSync(path.join(modPath, 'modding.xml'), 'utf8');
                if (modXML.includes('chapter1_windows') || modXML.includes('chapter2_windows')) {
                    meta.game = 'toby.deltarune.demolts';
                    modInfo.metadata.game = 'toby.deltarune.demolts';
                    fs.writeFileSync(manifestPath, JSON.stringify(modInfo, null, 2), 'utf8');
                }
                else {
                    meta.game = 'toby.deltarune.demo';
                    modInfo.metadata.game = 'toby.deltarune.demo';

                    var modInfoCP = modInfo; // avoid mutating the original modInfo in case of errors
                    delete modInfoCP.metadata.isForLTS;
                    delete modInfoCP.metadata.isIncompatible;
                    delete modInfoCP.metadata._incompatibleHASH;
                    fs.writeFileSync(manifestPath, JSON.stringify(modInfoCP, null, 2), 'utf8');

                    fs.writeFileSync(path.join(modPath, '_democheck'), "", 'utf8');
                }
            }

            try {
                if (deltamodExclusive.new == null) {
                    deltamodExclusive.new = false; // backfill old mods
                    fs.writeFileSync(idPath, JSON.stringify(deltamodExclusive, null, 2), 'utf8');
                }

                if (deltamodExclusive.uniqueId.split('_')[3] !== require('../package.json').version) {
                    // if the version is different, regenerate the uniqueId
                    console.log('mod version mismatch, regenerating uniqueId for mod:', mod);
                    deltamodExclusive = null; // force regeneration below
                }
            }
            catch {
                console.log('deltamodExclusive uniqueId version parse failed, regenerating uniqueId for mod:', mod);
            }

            if (!deltamodExclusive || !deltamodExclusive.uniqueId) {
                console.log('generating unique uid for mod:', mod);
                deltamodExclusive = {
                    uniqueId: system.generateUniqueId(),
                    validFor: computerName,
                    new: true
                };
                try {
                    fs.writeFileSync(idPath, JSON.stringify(deltamodExclusive, null, 2), 'utf8');
                } catch (_) {}
            }

            // de-dupe in memory so list has unique rows (don’t rewrite disk)
            let uid = deltamodExclusive.uniqueId;

            if (uniqueIdSet.has(uid)) uid = `${uid}#${mod}`;
            uniqueIdSet.add(uid);

            // sanity for required fields
            if (
                !meta ||
                typeof meta.name !== 'string' ||
                typeof meta.description !== 'string' ||
                typeof meta.game === 'undefined'
            ) {
                failureReason = "meta.toml is missing required fields `name`, `description` or `game`.";
                throw new Error(`Missing required fields in meta.toml for mod: ${mod}`);
            }

            if (fs.readdirSync(modPath).filter(x => x.endsWith('.js')).length !== 0
            || fs.readdirSync(modPath).filter(x => x.endsWith('.ts')).length !== 0
            || fs.readdirSync(modPath).filter(x => x.endsWith('.exe')).length !== 0) {
                failureReason = "This mod contains potentially malicious content. (EXE_DETECT)";
                throw new Error(`This mod contains potentially malicious content. (EXE_DETECT)`);
            }

            [meta.name, meta.description, meta.author].forEach(field => {
                if (
                    (typeof field === 'string' && /<\/?[^>]+>/.test(field))
                ) {
                    failureReason = "This mod contains potentially malicious content. (HTML_DETECT)";
                    throw new Error('This mod contains potentially malicious content. (HTML_DETECT)');
                }
            });

            var modSize = 0;
            function calculateFolderSize(folderPath) {
                const items = fs.readdirSync(folderPath);
                for (const item of items) {
                    const itemPath = path.join(folderPath, item);
                    const stats = fs.statSync(itemPath);
                    if (stats.isFile()) {
                        modSize += stats.size;
                    } else if (stats.isDirectory()) {
                        calculateFolderSize(itemPath);
                    }
                }
            }
            calculateFolderSize(modPath);
            // convert bytes to megabytes, round to 2 decimals; non-zero values are at least 0.01 MB
            modSize = modSize === 0 ? 0 : Math.max(0.01, Math.round((modSize / (1024 * 1024)) * 100) / 100);

            var games = require('./GameDB').getGames();
            if (!games.some(g => g.id === meta.game)) {
                failureReason = `Mod targets unknown game: ${meta.game}`;
                throw new Error(`Mod targets unknown game: ${meta.game}`);
            }

            try {
                var variant = fs.readFileSync(path.join(modPath, '__variant'), 'utf8').trim();
            }
            catch {
                variant = null;
            }
            // keep your return shape; just add ids (non-breaking)
            modList.push({
                name:         meta.name || mod,
                version:      require('./Utils').validateVersioning(meta.version) || "Unknown",
                author:       meta.author || computerName,
                description:  meta.description || '',
                folder:       mod,
                size:         modSize, // New in 1.1.2
                mergeSupport: (meta.mergeSupport == undefined ? true : meta.mergeSupport), // default true
                url:          meta.url || null,
                customRGB:    meta.color || null,
                variants:     modInfo.variants || null,
                game:         meta.game || "toby.deltarune",
                dependencies: modInfo.dependencies || [],
                packageID: pid,
                gamebanana: {
                    supports: meta.gamebanana_id != null && meta.gamebanana_model != null,
                    id:       meta.gamebanana_id || null,
                    model:    meta.gamebanana_model || null,
                },
                source: meta.source_provider ? {
                    provider: meta.source_provider,
                    id: meta.source_id || null,
                    fileId: meta.source_file_id || null,
                    url: meta.source_url || null
                } : null,
                _incompatibleHASH: meta._incompatibleHASH || false,
                _hashDifferentFiles: meta._hashDifferentFiles || [],
                _selectedVariant: variant || null,
                // NEW: give the renderer stable identifiers
                new: deltamodExclusive.new || false, // Used in UI

                uniqueId: uid,
                uid:      uid,   // <- many UIs look for this name
                id:       uid,

                // TODO I don't know what the default values for these fields should be.
                // I'm just adding them to satisfy the typechecker.
                // 
                // GHINORHINO NOTE:
                // These are dynamically set one level above this function, before sending them off to the renderer,
                // compatibility checks are performed in Runner.js
                isIncompatible: false,
                incompatibilityReason: "",
            });
        }
        catch (e) {
            console.error(`Error reading mod info for ${mod}:`, e, ' ' + e.stack);
            errors.push({ mod, reason: failureReason });
        }
    };

    /*
    // Zork's Patch: give the “No.” column a value most UIs expect, this could be used for sorting mods by priority in the future, but probably not as GM3P doesn't have that right now.
    modList.sort((a, b) => String(a.uniqueId).localeCompare(String(b.uniqueId)));
    modList.forEach((m, i) => {
        const n = i + 1;
        m.priority = n; // many UIs use this for the first column
        m.number   = n;
        m.index    = n;
        m.no       = n;
    });
    */
    // CURRENTLY DEPRECATED: priority function was planned but removed to favor GM3P integration

    if (hashCacheDirty) saveHashCache(hashCachePath, hashCache);
    return { modList, errors };
}

function getModImage(moduid) {
    var modPackets = fs.readdirSync(system.getPacketDatabase());
    for (var mod of modPackets) {
        var deltaID = safeReadJSON(path.join(system.getPacketDatabase(), mod, '__deltaID.json'));
        if (deltaID && deltaID.uniqueId === moduid) {
            try {
                const imgPath = mod + '/icon.png';
                if (fs.existsSync(path.join(system.getPacketDatabase(), imgPath))) {
                    return { exists: true, path: 'packet://' + imgPath };
                }
                return { exists: false, path: null };
            }
            catch {
                return { exists: false, path: null };
            }
        }
    }
    return { exists: false, path: null };
}
if (!fs.existsSync(system.getPacketDatabase())) {
    fs.mkdirSync(system.getPacketDatabase(), { recursive: true });
}

module.exports = {
    modList,
    importMod,
    howmany,
    downloadModFromURL,
    removeModSafe,
    getModImage,
    resolveModFolder,
    validatePID
};
