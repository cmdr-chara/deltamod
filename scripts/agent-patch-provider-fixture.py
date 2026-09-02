from pathlib import Path

path = Path("tests/e2e/community-smoke.spec.js")
text = path.read_text(encoding="utf-8")
start_marker = "        await window.evaluate(() => {\n            window.__deltamodOriginalRendererFetch = window.fetch;\n"
end_marker = "\n        for (const [route, selector] of ["
start = text.find(start_marker)
if start < 0:
    raise SystemExit("renderer fetch fixture start marker not found")
end = text.find(end_marker, start)
if end < 0:
    raise SystemExit("renderer fetch fixture end marker not found")
if text.find(start_marker, start + 1) >= 0:
    raise SystemExit("renderer fetch fixture start marker is not unique")

replacement = r'''        await application.evaluate(({ ipcMain }) => {
            const makeGameBananaMod = ({ id, name, description, authorId, authorName, imageCount }) => ({
                _idRow: id,
                _sModelName: 'Mod',
                _sName: name,
                _sDescription: description,
                _sProfileUrl: `https://gamebanana.com/mods/${id}`,
                _bHasFiles: true,
                _bHasContentRatings: false,
                _tsDateAdded: 1751328000 + (id === 41 ? 1 : 0),
                _tsDateModified: 1751328000 + (id === 41 ? 1 : 0),
                _aSubmitter: {
                    _idRow: authorId,
                    _sName: authorName,
                    _sProfileUrl: `https://gamebanana.com/members/${authorId}`,
                    _sAvatarUrl: './img/mod-placeholder.png'
                },
                _aPreviewMedia: {
                    _aImages: Array.from({ length: imageCount }, () => ({
                        _sBaseUrl: './img',
                        _sFile: 'mod-placeholder.png',
                        _sFile100: 'mod-placeholder.png',
                        _sFile220: 'mod-placeholder.png',
                        _sFile530: 'mod-placeholder.png'
                    }))
                }
            });
            const regularMod = makeGameBananaMod({
                id: 41,
                name: 'Regular test mod',
                description: 'A regular GameBanana card.',
                authorId: 8,
                authorName: 'Regular author',
                imageCount: 1
            });
            const galleryMod = makeGameBananaMod({
                id: 42,
                name: 'Gallery test mod',
                description: 'A GameBanana card using the shared shop layout.',
                authorId: 7,
                authorName: 'Test author',
                imageCount: 2
            });

            ipcMain.removeHandler('modSources:browse');
            ipcMain.handle('modSources:browse', (_event, args) => {
                const request = args?.[0] || {};
                if (request.provider !== 'gamebanana') {
                    return {
                        ok: false,
                        error: {
                            code: 'TEST_UNEXPECTED_PROVIDER',
                            message: `Unexpected provider in E2E fixture: ${request.provider || 'unknown'}`
                        }
                    };
                }

                const url = String(request.url || '');
                let payload;
                if (url.includes('/Subfeed')) {
                    payload = {
                        _aMetadata: { _bIsComplete: true },
                        _aRecords: url.includes('_nPage=2') ? [galleryMod] : [regularMod]
                    };
                } else if (url.includes('/TopSubs')) {
                    payload = [{ ...galleryMod, _sPeriod: 'alltime' }];
                } else {
                    return {
                        ok: false,
                        error: {
                            code: 'TEST_UNEXPECTED_GAMEBANANA_URL',
                            message: `Unexpected GameBanana URL in E2E fixture: ${url}`
                        }
                    };
                }

                return {
                    ok: true,
                    result: {
                        provider: 'gamebanana',
                        payload,
                        cached: false,
                        stale: false
                    }
                };
            });
        });
'''

path.write_text(text[:start] + replacement + text[end:], encoding="utf-8")
