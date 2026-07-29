const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const MAXIMUM_LOCAL_ARCHIVE_BYTES = 2 * 1024 * 1024 * 1024;
const ALLOWED_EXTENSIONS = new Set(['.modarchive', '.zip']);

async function assertNoLinkedParents(filePath) {
    const resolved = path.resolve(filePath);
    const parsed = path.parse(resolved);
    const relativeParts = resolved.slice(parsed.root.length).split(path.sep).filter(Boolean);
    let current = parsed.root;
    for (const part of relativeParts.slice(0, -1)) {
        current = path.join(current, part);
        const stat = await fs.promises.lstat(current);
        if (!stat.isDirectory() || stat.isSymbolicLink()) {
            throw new Error('The archive path contains a linked or invalid parent directory.');
        }
    }
}

async function validateLocalArchive(filePath) {
    if (typeof filePath !== 'string' || !filePath.trim() || filePath.includes('\0')) {
        throw new Error('The local mod archive path is invalid.');
    }
    if (filePath.length > 32_767 || !path.isAbsolute(filePath)) {
        throw new Error('The local mod archive path must be absolute.');
    }
    if (process.platform === 'win32' && /^\\\\(?:\\?\\|\.\\|[^\\])/u.test(filePath)) {
        throw new Error('UNC and Windows device paths are not accepted for local mod imports.');
    }

    const resolved = path.resolve(filePath);
    if (!ALLOWED_EXTENSIONS.has(path.extname(resolved).toLowerCase())) {
        throw new Error('Only .modarchive and .zip files can be imported.');
    }
    await assertNoLinkedParents(resolved);
    const stat = await fs.promises.lstat(resolved);
    if (!stat.isFile() || stat.isSymbolicLink() || stat.nlink > 1) {
        throw new Error('The local mod archive must be a regular, non-linked file.');
    }
    if (stat.size <= 0 || stat.size > MAXIMUM_LOCAL_ARCHIVE_BYTES) {
        throw new Error('The local mod archive is empty or exceeds the 2 GiB limit.');
    }
    const realPath = await fs.promises.realpath(resolved);
    const realStat = await fs.promises.lstat(realPath);
    if (!realStat.isFile() || realStat.isSymbolicLink() || realStat.nlink > 1) {
        throw new Error('The resolved local mod archive is not a regular, non-linked file.');
    }
    return { path: realPath, stat: realStat };
}

async function stageLocalArchive(sourcePath, temporaryRoot) {
    const validated = await validateLocalArchive(sourcePath);
    await fs.promises.mkdir(temporaryRoot, { recursive: true });
    const destination = path.join(temporaryRoot, `${crypto.randomUUID()}.modarchive`);
    let sourceHandle;
    let destinationHandle;

    try {
        sourceHandle = await fs.promises.open(validated.path, 'r');
        const before = await sourceHandle.stat();
        if (
            !before.isFile()
            || before.nlink > 1
            || before.size !== validated.stat.size
            || before.size > MAXIMUM_LOCAL_ARCHIVE_BYTES
        ) {
            throw new Error('The local mod archive changed before staging.');
        }

        destinationHandle = await fs.promises.open(destination, 'wx');
        const buffer = Buffer.allocUnsafe(1024 * 1024);
        let offset = 0;
        while (offset < before.size) {
            const length = Math.min(buffer.length, before.size - offset);
            const { bytesRead } = await sourceHandle.read(buffer, 0, length, offset);
            if (bytesRead <= 0) throw new Error('The local mod archive ended unexpectedly while staging.');
            let written = 0;
            while (written < bytesRead) {
                const result = await destinationHandle.write(buffer, written, bytesRead - written, offset + written);
                if (result.bytesWritten <= 0) throw new Error('The staged archive could not be written completely.');
                written += result.bytesWritten;
            }
            offset += bytesRead;
        }
        await destinationHandle.sync();

        const after = await sourceHandle.stat();
        const staged = await destinationHandle.stat();
        if (
            after.size !== before.size
            || after.mtimeMs !== before.mtimeMs
            || staged.size !== before.size
        ) {
            throw new Error('The local mod archive changed while it was being staged.');
        }
        return destination;
    } catch (error) {
        await fs.promises.rm(destination, { force: true });
        throw error;
    } finally {
        await destinationHandle?.close().catch(() => {});
        await sourceHandle?.close().catch(() => {});
    }
}

module.exports = {
    ALLOWED_EXTENSIONS,
    MAXIMUM_LOCAL_ARCHIVE_BYTES,
    stageLocalArchive,
    validateLocalArchive
};
