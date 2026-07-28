const fs = require('fs');

async function detectImageType(filePath) {
    const handle = await fs.promises.open(filePath, 'r');
    try {
        const buffer = Buffer.alloc(16);
        const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
        const header = buffer.subarray(0, bytesRead);
        if (header.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))) {
            return 'png';
        }
        if (header[0] === 0xff && header[1] === 0xd8 && header[2] === 0xff) return 'jpeg';
        if (
            header.subarray(0, 6).toString('ascii') === 'GIF87a'
            || header.subarray(0, 6).toString('ascii') === 'GIF89a'
        ) {
            return 'gif';
        }
        if (
            header.subarray(0, 4).toString('ascii') === 'RIFF'
            && header.subarray(8, 12).toString('ascii') === 'WEBP'
        ) {
            return 'webp';
        }
        return null;
    } finally {
        await handle.close();
    }
}

module.exports = { detectImageType };
