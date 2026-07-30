// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const ThemeSprites = require('../web/modules/theme-sprites');
const fs = require('fs');
const path = require('path');

function relativeLuminance([red, green, blue]) {
    const channels = [red, green, blue].map(channel => {
        const normalized = channel / 255;
        return normalized <= 0.03928
            ? normalized / 12.92
            : ((normalized + 0.055) / 1.055) ** 2.4;
    });
    return (0.2126 * channels[0]) + (0.7152 * channels[1]) + (0.0722 * channels[2]);
}

describe('theme sprite recoloring', () => {
    it('parses supported theme colors and rejects invalid channels', () => {
        expect(ThemeSprites.parseThemeColor('rgb(20, 62, 128)')).toEqual([20, 62, 128]);
        expect(ThemeSprites.parseThemeColor('#003CFF')).toEqual([0, 60, 255]);
        expect(ThemeSprites.parseThemeColor('rgb(300, 62, 128)')).toBeNull();
        expect(ThemeSprites.parseThemeColor('#143e8')).toBeNull();
    });

    it('recolors the soul while preserving the gear palette', () => {
        const source = new Uint8ClampedArray([
            15, 183, 214, 255,
            116, 122, 160, 255
        ]);
        const output = ThemeSprites.recolorPixels(source, [20, 62, 128], 'soul');

        expect([...output.slice(0, 4)]).toEqual([0, 60, 255, 255]);
        expect([...output.slice(4, 8)]).toEqual([116, 122, 160, 255]);
    });

    it('snaps theme colors to the exact Undertale SOUL palette', () => {
        expect(ThemeSprites.canonicalSoulColor([205, 68, 81])).toEqual([255, 0, 0]);
        expect(ThemeSprites.canonicalSoulColor([145, 75, 0])).toEqual([252, 166, 0]);
        expect(ThemeSprites.canonicalSoulColor([6, 108, 59])).toEqual([0, 192, 0]);
        expect(ThemeSprites.canonicalSoulColor([0, 98, 137])).toEqual([66, 252, 255]);
        expect(ThemeSprites.canonicalSoulColor([20, 62, 128])).toEqual([0, 60, 255]);
        expect(ThemeSprites.canonicalSoulColor([140, 40, 140])).toEqual([213, 53, 217]);
    });

    it('keeps readable UI accents separate from canonical SOUL colors', () => {
        const themeDirectory = path.join(__dirname, '..', 'web', 'themes', 'data');
        const canonicalSoulColors = new Set(
            ThemeSprites.SOUL_COLORS.map(color => color.rgb.join(','))
        );

        for (const filename of fs.readdirSync(themeDirectory).filter(name => name.endsWith('.theme.json'))) {
            const theme = JSON.parse(fs.readFileSync(path.join(themeDirectory, filename), 'utf8'));
            const interfaceColor = ThemeSprites.parseThemeColor(theme.color);
            const soulColor = ThemeSprites.parseThemeColor(theme.soulColor);

            expect(interfaceColor, `${filename} has a valid UI color`).not.toBeNull();
            expect(soulColor, `${filename} has a valid SOUL color`).not.toBeNull();
            expect(canonicalSoulColors.has(soulColor.join(',')), `${filename} uses a canonical SOUL color`).toBe(true);

            const contrastWithWhite = 1.05 / (relativeLuminance(interfaceColor) + 0.05);
            expect(contrastWithWhite, `${filename} keeps white labels readable`).toBeGreaterThanOrEqual(4.5);
        }
    });

    it('recolors only the mapped accent and preserves secondary colors', () => {
        const source = new Uint8ClampedArray([
            97, 78, 107, 255,
            255, 201, 14, 255,
            255, 84, 86, 0
        ]);
        const output = ThemeSprites.recolorPixels(
            source,
            [20, 62, 128],
            'accent',
            'options.png'
        );

        expect([...output.slice(0, 3)]).toEqual([0, 60, 255]);
        expect([...output.slice(4, 7)]).toEqual([255, 201, 14]);
        expect([...output.slice(8, 12)]).toEqual([255, 84, 86, 0]);
    });

    it('keeps collection blocks distinct with neighboring theme hues', () => {
        const source = new Uint8ClampedArray([
            34, 54, 167, 255,
            65, 140, 26, 255,
            140, 26, 128, 255
        ]);
        const output = ThemeSprites.recolorPixels(
            source,
            [6, 108, 59],
            'accent',
            'collections.png'
        );
        const colors = [
            [...output.slice(0, 3)].join(','),
            [...output.slice(4, 7)].join(','),
            [...output.slice(8, 11)].join(',')
        ];

        expect(new Set(colors).size).toBe(3);
    });

    it('gives the mod bookmark a contrasting neighboring hue', () => {
        const source = new Uint8ClampedArray([
            255, 84, 86, 255,
            48, 169, 142, 255
        ]);
        const output = ThemeSprites.recolorPixels(
            source,
            [20, 190, 210],
            'accent',
            'allmods.png'
        );

        expect([...output.slice(0, 3)]).not.toEqual([...output.slice(4, 7)]);
        expect([...output.slice(4, 7)]).not.toEqual([48, 169, 142]);
    });

    it('coordinates the install speech graphic without flattening its details', () => {
        const source = new Uint8ClampedArray([
            18, 99, 188, 255,
            172, 50, 50, 255,
            248, 231, 255, 255
        ]);
        const output = ThemeSprites.recolorPixels(
            source,
            [210, 45, 70],
            'accent',
            'installmanager.png'
        );

        expect([...output.slice(0, 3)]).not.toEqual([...output.slice(4, 7)]);
        expect([...output.slice(8, 11)]).toEqual([248, 231, 255]);
    });
});
