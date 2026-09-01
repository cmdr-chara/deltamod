const path = require('node:path');

const { describe, expect, it } = globalThis;
const {
    REQUIRED_EVENT_PRODUCERS,
    assertParity,
    buildParity,
    classifyReachableEvents,
    readRustSources
} = require('../scripts/tauri-parity/lib/parity');

const root = path.join(__dirname, '..');
const paths = {
    preloadPath: path.join(root, 'web', 'preload.js'),
    rustPath: path.join(root, 'src-tauri', 'src', 'main.rs'),
    rustSourceRoot: path.join(root, 'src-tauri', 'src')
};

describe('Tauri renderer event production classification', () => {
    it('does not treat comments, cfg(test), or unreachable functions as production evidence', () => {
        const fixture = String.raw`
            fn main() {
                // app.emit("leave-controller-mode", ());
                wired();
            }
            fn wired() {}
            #[cfg(test)]
            mod tests {
                fn test_only() {
                    app.emit("protocol-download-progress", ());
                }
            }
            fn inert() {
                app.emit("leave-controller-mode", ());
                app.emit("protocol-download-progress", ());
            }
        `;
        const evidence = classifyReachableEvents([{ file: 'fixture.rs', source: fixture }]);
        expect(evidence.has('leave-controller-mode')).toBe(false);
        expect(evidence.has('protocol-download-progress')).toBe(false);
    });

    it('requires the reachable producers in controller.rs and import_download.rs', () => {
        const report = buildParity(paths);
        expect(report.ok).toBe(true);
        expect(report.gaps.missingEventProducers).toEqual([]);
        for (const [event, expectedFile] of Object.entries(REQUIRED_EVENT_PRODUCERS)) {
            const producer = report.rust.eventProducers.find(item => item.event === event);
            expect(producer).toMatchObject({ event, expectedFile, present: true });
            expect(producer.producers).toContainEqual(expect.objectContaining({ file: expectedFile }));
        }
    });

    it.each(Object.entries(REQUIRED_EVENT_PRODUCERS))(
        'fails parity when the %s producer is deleted',
        (event, expectedFile) => {
            const sources = readRustSources(paths.rustSourceRoot, root);
            let deleted = false;
            const mutated = sources.map(file => {
                if (file.file !== expectedFile) return file;
                const literal = `"${event}"`;
                expect(file.source).toContain(literal);
                deleted = true;
                return { ...file, source: file.source.replace(literal, `"deleted-${event}"`) };
            });
            expect(deleted).toBe(true);

            const report = buildParity({ ...paths, rustSources: mutated });
            expect(report.ok).toBe(false);
            expect(report.gaps.missingEventProducers).toContainEqual(
                expect.objectContaining({ event, expectedFile, present: false })
            );
            expect(() => assertParity(report)).toThrow(`missing Rust event producers: ${event}`);
        }
    );
});
