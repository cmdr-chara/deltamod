const console = require('./Console.js');
const fs = require('fs');

function createJunction(target, path) {
    console.log(`Creating junction from ${path} to ${target}`);
    try {
        fs.symlinkSync(target, path, "junction");
        return `Successfully created junction from ${path} to ${target}`
    } catch (err) {
        return err.toString();
    }
}

function deleteJunction(path) {
    console.log(`Deleting junction at ${path}`);
    try {
        fs.unlinkSync(path)
        return `Successfully deleted junction at ${path}`
    } catch (err) {
        return err.toString();
    }
}

function isJunction(path) {
    try {
        return fs.lstatSync(path).isSymbolicLink();
    } catch {
        return false;
    }
}

module.exports = {
    createJunction,
    deleteJunction,
    isJunction
};
