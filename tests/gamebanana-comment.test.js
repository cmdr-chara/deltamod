// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const {
    MAX_COMMENT_LENGTH,
    normalizeCommentTarget,
    normalizeCommentText,
    createCommentRequest
} = require('../node/gamebanana/CommentRequest');

describe('GameBanana comment requests', () => {
    it('builds a validated request and preserves line breaks safely', () => {
        expect(createCommentRequest(698242, 'Hello <Kris>\nStay determined & kind.', 'Mod')).toEqual({
            url: 'https://gamebanana.com/apiv12/Mod/698242/Post/Add',
            payload: {
                _aImageFiles: [],
                _aImages: [],
                _aMentionedMemberRowIds: [],
                _sText: '<p>Hello &lt;Kris&gt;<br>Stay determined &amp; kind.</p>'
            }
        });
    });

    it('rejects missing or oversized comments', () => {
        expect(() => normalizeCommentText('   ')).toThrow('Enter a comment');
        expect(() => normalizeCommentText('x'.repeat(MAX_COMMENT_LENGTH + 1))).toThrow('cannot exceed');
    });

    it('rejects unsafe submission targets', () => {
        expect(() => normalizeCommentTarget('../1', 'Mod')).toThrow();
        expect(() => normalizeCommentTarget(1, '../Mod')).toThrow();
        expect(() => normalizeCommentTarget(0, 'Mod')).toThrow();
    });
});

describe('GameBanana comment responses', () => {
    const { readFileSync } = require('node:fs');
    const { runInNewContext } = require('node:vm');
    const page = readFileSync(require('node:path').join(__dirname, '../web/views/gamebanana-leave-comment/index.js'), 'utf8');
    const source = page.slice(page.indexOf('async function readGameBananaJson('), page.indexOf('function commentErrorMessage('));
    const read = runInNewContext(source + '; readGameBananaJson');
    const response = text => ({ text: async () => text });

    it('parses clean JSON', async () => {
        expect(await read(response('{"_aRecords":[1]}'))).toEqual({ _aRecords: [1] });
    });
    it('skips PHP warnings that GameBanana prints before the body', async () => {
        const body = '\n<b>Warning</b>: Undefined array key "images" in Cacher.php on line 87\n{\n    "_aRecords": [{"_idRow": 5}]\n}';
        expect(await read(response(body))).toEqual({ _aRecords: [{ _idRow: 5 }] });
    });
    it('reports a response with no JSON body', async () => {
        await expect(read(response('Warning: database offline'))).rejects.toThrow('unreadable');
    });
});
