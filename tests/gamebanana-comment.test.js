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
