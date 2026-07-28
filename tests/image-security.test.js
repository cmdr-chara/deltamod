const fs = require('fs');
const os = require('os');
const path = require('path');
const { detectImageType } = require('../node/security/ImageSecurity');

const files = [];
afterEach(() => {
    while (files.length) fs.rmSync(files.pop(), { force: true });
});

async function detect(buffer) {
    const file = path.join(os.tmpdir(), `deltamod-image-${Date.now()}-${Math.random()}`);
    files.push(file);
    fs.writeFileSync(file, buffer);
    return detectImageType(file);
}

it('recognizes supported image signatures and rejects disguised content', async () => {
    expect(await detect(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]))).toBe('png');
    expect(await detect(Buffer.from([0xff, 0xd8, 0xff, 0xe0]))).toBe('jpeg');
    expect(await detect(Buffer.from('GIF89a'))).toBe('gif');
    expect(await detect(Buffer.from('RIFF0000WEBP'))).toBe('webp');
    expect(await detect(Buffer.from('<script>alert(1)</script>'))).toBeNull();
});
