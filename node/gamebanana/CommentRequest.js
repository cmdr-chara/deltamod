// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const MAX_COMMENT_LENGTH = 10000;

function commentError(code, message) {
    const error = new Error(message);
    error.code = code;
    return error;
}

function normalizeCommentTarget(id, model) {
    const numericId = Number(id);
    const normalizedModel = String(model || '').trim();

    if (!Number.isSafeInteger(numericId) || numericId <= 0) {
        throw commentError('INVALID_GAMEBANANA_ITEM_ID', 'The GameBanana submission ID is invalid.');
    }
    if (!/^[A-Za-z][A-Za-z0-9]{0,31}$/.test(normalizedModel)) {
        throw commentError('INVALID_GAMEBANANA_MODEL', 'The GameBanana submission type is invalid.');
    }

    return { id: numericId, model: normalizedModel };
}

function normalizeCommentText(value) {
    if (typeof value !== 'string') {
        throw commentError('INVALID_GAMEBANANA_COMMENT', 'Enter a comment before sending.');
    }

    const text = value.replace(/\r\n?/g, '\n').trim();
    if (!text) {
        throw commentError('EMPTY_GAMEBANANA_COMMENT', 'Enter a comment before sending.');
    }
    if (text.length > MAX_COMMENT_LENGTH) {
        throw commentError(
            'GAMEBANANA_COMMENT_TOO_LONG',
            `Comments cannot exceed ${MAX_COMMENT_LENGTH.toLocaleString('en-US')} characters.`
        );
    }
    if (/[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F]/.test(text)) {
        throw commentError('INVALID_GAMEBANANA_COMMENT', 'The comment contains unsupported control characters.');
    }

    return text;
}

function escapeHtml(value) {
    return value
        .replaceAll('&', '&amp;')
        .replaceAll('<', '&lt;')
        .replaceAll('>', '&gt;')
        .replaceAll('"', '&quot;')
        .replaceAll("'", '&#39;');
}

function createCommentRequest(id, comment, model) {
    const target = normalizeCommentTarget(id, model);
    const text = normalizeCommentText(comment);
    return {
        url: `https://gamebanana.com/apiv12/${target.model}/${target.id}/Post/Add`,
        payload: {
            _aImageFiles: [],
            _aImages: [],
            _aMentionedMemberRowIds: [],
            _sText: `<p>${escapeHtml(text).replaceAll('\n', '<br>')}</p>`
        }
    };
}

module.exports = {
    MAX_COMMENT_LENGTH,
    normalizeCommentTarget,
    normalizeCommentText,
    createCommentRequest
};
