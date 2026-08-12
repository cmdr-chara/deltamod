// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const fs = require('fs');
const path = require('path');

const root = path.join(__dirname, '..', 'web', 'langs');
const languages = ['en', 'it', 'pl', 'es', 'fr', 'de', 'pt-br', 'ja'];

function loadDictionary(code) {
    const source = fs.readFileSync(path.join(root, code, 'language.json'), 'utf8');
    return JSON.parse(source.replace(/\/\*[\s\S]*?\*\//g, ''));
}

function placeholders(value) {
    return [...String(value).matchAll(/{(\d+)}/g)]
        .map(match => match[0])
        .sort();
}

describe('localization catalogs', () => {
    const english = loadDictionary('en');

    it('ships every supported locale with valid metadata and a flag', () => {
        for (const code of languages) {
            expect(() => loadDictionary(code)).not.toThrow();

            const lines = fs.readFileSync(path.join(root, code, 'metadata.txt'), 'utf8')
                .split(/\r?\n/)
                .map(line => line.trim());
            expect(lines[0]).toBeTruthy();
            expect(lines[1]).toBeTruthy();
            expect(lines[2]).toBeTruthy();

            const flag = lines[3] || 'flag.png';
            expect(flag).toMatch(/^[A-Za-z0-9._-]+$/);
            expect(fs.existsSync(path.join(root, code, flag))).toBe(true);
        }
    });

    it('keeps every supported catalog complete', () => {
        const expected = Object.keys(english).sort();
        expect(Object.keys(english).length).toBeGreaterThan(200);
        for (const code of languages.filter(code => code !== 'en')) {
            expect(Object.keys(loadDictionary(code)).sort(), code).toEqual(expected);
        }
    });

    it('preserves interpolation placeholders in every translated entry', () => {
        for (const code of languages.filter(code => code !== 'en')) {
            const dictionary = loadDictionary(code);
            for (const [key, translated] of Object.entries(dictionary)) {
                if (!(key in english)) continue;
                expect(placeholders(translated), `${code}:${key}`).toEqual(
                    placeholders(english[key])
                );
            }
        }
    });
});
