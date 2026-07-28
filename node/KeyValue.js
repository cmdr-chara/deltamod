const path = require('path');
const app = require('electron').app;
const fs = require('fs');
let kvs = {};
const { getSystemFile, getSystemFolder, healthCheck, getSystemFileOfIndex, getSystemFolderOfIndex } = require('./System.js');
const { get } = require('http');
const crypto = require('crypto');
const console = require('./Console.js');
const { readJsonSync, writeFileAtomicSync, writeJsonAtomicSync } = require('./storage/AtomicStore');

function hash(str) {
    return crypto.createHash('sha256').update(str).digest('hex');
}

function retrieve() {
    healthCheck();
    var pathname = getSystemFile('store.json', false);
    if (!fs.existsSync(pathname)) {
        console.log('Creating blank store');
        fs.writeFileSync(pathname, '{}');
    }
    kvs = readJsonSync(pathname, {});
    console.log('Store loaded')
    return true;
}

function kvsFlush() {
    var pathname = getSystemFile('store.json', false);
    var exportedKVS = kvs;
    
    exportedKVS.version = 'DELTAMOD_DATA_'+require('../package.json').version;
    writeJsonAtomicSync(pathname, exportedKVS);
    console.log('Store flushed.');
    return true;
}

function kvsFlushIndex(obj, index) {
    var pathname = getSystemFileOfIndex('store.json', index);
    writeJsonAtomicSync(pathname, obj);
    console.log('Store flushed for index ' + index);
    return true;
}

function writeUniqueFlag(name, val) {
    try {
        var database = getSystemFile('flagDB.config', true);
        if (!fs.existsSync(database)) {
            writeFileAtomicSync(database, defFDBMsg);
        }
        var databaseContent = fs.readFileSync(database, 'utf8');
        var lines = databaseContent.split('\n').filter(l => l.trim() != '' && !l.startsWith(name.toUpperCase() + ' = '));
        lines.push(name.toUpperCase() + ' = ' + (val ? '1' : '0'));
        writeFileAtomicSync(database, lines.join('\n'));
        return true;
    }
    catch (e) {
        console.log('Error writing unique flag: ' + e);
        return false;
    }
}

function existsUniqueFlag(name) {
    try {
        var database = getSystemFile('flagDB.config', true);
        if (!fs.existsSync(database)) {
            writeFileAtomicSync(database, "");
        }
        var databaseContent = fs.readFileSync(database, 'utf8');

        return databaseContent.split('\n').some(l => l.startsWith(name.toUpperCase() + ' = '));
    }
    catch (e) {
        console.log('Error checking unique flag existence: ' + e);
        return false;
    }
}
function readUniqueFlag(name) {
    try {
        var database = getSystemFile('flagDB.config', true);
        if (!fs.existsSync(database)) {
            writeFileAtomicSync(database, '');
        }
        var databaseContent = fs.readFileSync(database, 'utf8');

        var line = databaseContent.split('\n').find(l => l.startsWith(name.toUpperCase() + ' = '));
        if (line) {
            var value = line.split(' = ')[1].trim();
            return value.toLowerCase() == '1';
        }
        databaseContent += "\n" + name.toUpperCase() + " = 0";
        writeFileAtomicSync(database, databaseContent);
        return false;
    }
    catch (e) {
        console.log('Error reading unique flag: ' + e);
        return false;
    }
}

function kvsWipe() {
    kvs = {};
    var pathname = getSystemFile('store.json', false);
    writeJsonAtomicSync(pathname, {});
    console.log('Wiped store');
    return true;
}

function setKVS(name, value) {
    kvs[name] = value;
    kvsFlush();
}

function loadUniqueDefaults() {
    var defaults = {
        'setup': true,
        'audio': true,
        'sfx': true,
        'controller': false,
    };

    for (var key in defaults) {
        if (!existsUniqueFlag(key)) {
            console.log('Setting flag default for ' + key + ' to ' + defaults[key]);
            // set unique flags
            writeUniqueFlag(key, defaults[key]);
        }
    }
}

function setKVSOfIndex(name, value, index) {
    try {
        var odb = readJsonSync(getSystemFileOfIndex("store.json", index), {});
    }
    catch (e) {
        var odb = {};
    }
    odb[name] = value;
    kvsFlushIndex(odb, index);
}

function readKVSOfIndex(name, index, defaultTo = null) {
    var odb = readJsonSync(getSystemFileOfIndex("store.json", index), {});
    return odb[name] ?? defaultTo;
}

function upgradeStores() {
    try {
        var oldStorePath = app.getPath('userData');

        console.log('Checking for old stores to upgrade in ' + oldStorePath);

        fs.readdirSync(oldStorePath).filter(f => f.startsWith('deltamod_system-')).forEach(file => {
            if (file.endsWith('unique')) return;

            var indx = file.split('-')[1];
            console.log('Checking install index ' + indx);
            var edi = readKVSOfIndex('deltaruneEdition', indx, "none");

            console.log('Found edition ' + edi + ' in index ' + indx);
            if (edi != "rem") {
                console.log('Upgrading index ' + indx);
                var pid = "toby.deltarune.demo";
                if (readKVSOfIndex('deltaruneEdition', indx, "n") == "full") {
                    pid = "toby.deltarune";
                }
                setKVSOfIndex('gamePid', pid, indx);
                setKVSOfIndex('deltaruneEdition', "rem", indx);
                console.log('Upgraded index ' + indx + ' (Edition => GAMEID)');
                return;
            }

            function scanFolderRecursively(folder) {
                fs.readdirSync(folder).forEach(file => {
                    var fullPath = path.join(folder, file);
                    const stats = fs.lstatSync(fullPath);
                    if (stats.isSymbolicLink()) return;
                    if (stats.isDirectory()) {
                        scanFolderRecursively(fullPath);
                    }
                    else if (
                        stats.isFile()
                        && stats.nlink === 1
                        && file.endsWith('.hash')
                        && /^[a-f0-9]{64}$/i.test(fs.readFileSync(fullPath, 'utf8').trim())
                    ) {
                        console.log('Found hash file: ' + fullPath);
                        fs.rmSync(fullPath);
                    }
                });
            }

            var gpath = getSystemFolderOfIndex('', indx);
            console.log('Scanning for .hash files in ' + gpath);

            scanFolderRecursively(getSystemFolderOfIndex('', indx));

            console.log('Finished upgrading index ' + indx);
        });
    }
    catch (e) {
        console.log('No old stores found to upgrade: ' + e);
    }
}

function readKVS(name, defaultTo = null) {
    return kvs[name] ?? defaultTo;
}

module.exports = {
    hash,
    retrieve,
    kvsFlush,
    writeUniqueFlag,
    readUniqueFlag,
    kvsWipe,
    setKVS,
    setKVSOfIndex,
    readKVSOfIndex,
    readKVS,
    loadUniqueDefaults,
    upgradeStores
};
