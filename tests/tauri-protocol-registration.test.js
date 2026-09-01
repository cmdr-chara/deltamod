const fs = require('node:fs');
const path = require('node:path');
const root = path.join(__dirname, '..');

describe('packaged Tauri protocol registration', () => {
    it('claims only the Community scheme and registers single-instance first', () => {
        const config = JSON.parse(fs.readFileSync(
            path.join(root, 'src-tauri', 'tauri.conf.json'),
            'utf8'
        ));
        expect(config.plugins['deep-link'].desktop.schemes)
            .toEqual(['deltamod-community']);

        const main = fs.readFileSync(path.join(root, 'src-tauri', 'src', 'main.rs'), 'utf8');
        const single = main.indexOf('.plugin(tauri_plugin_single_instance::init');
        const deepLink = main.indexOf('.plugin(tauri_plugin_deep_link::init())');
        expect(single).toBeGreaterThan(0);
        expect(deepLink).toBeGreaterThan(single);
        expect(main).toContain('controller::protocol_second_instance(app, argv)');
    });

    it('drops the executable argument before the existing strict handoff boundary', () => {
        const controller = fs.readFileSync(
            path.join(root, 'src-tauri', 'src', 'controller.rs'),
            'utf8'
        );
        expect(controller).toContain('args.into_iter().skip(1).map(OsString::from)');
        expect(controller).toContain('match classify_handoff_argument(arg)');
    });
});
