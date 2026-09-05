(() => {
var gbModID = window._pageArguments.id;
var gbModel = window._pageArguments.model;
var commentPage = window._pageArguments.commentPage || 1;
window._pageArguments = {};
var isSendingComment = false;
const commentsRoot = document.querySelector('.comments');
const abort = new AbortController();
let disposed = false;
window.DeltamodUI.onDispose(() => { disposed = true; abort.abort(); });
const recordsRoot = document.createElement('section');
recordsRoot.className = 'comment-thread';
recordsRoot.setAttribute('aria-busy', 'true');
commentsRoot.appendChild(recordsRoot);
async function getComments(model, id, pageNumber, count) {
    if (!/^[A-Za-z]+$/.test(String(model)) || !/^\d+$/.test(String(id))) throw new Error('Invalid comment target');
    const response = await fetch(`https://gamebanana.com/apiv11/${model}/${id}/Posts?_nPage=${pageNumber}&_nPerpage=${count}&_sSort=popular`, { signal: abort.signal });
    if (!response.ok) throw new Error(`GameBanana returned HTTP ${response.status}`);
    const result = await response.json();
    if (!Array.isArray(result?._aRecords)) throw new Error('GameBanana returned an invalid comment list.');
    return result._aRecords;
}

function commentErrorMessage(error) {
    return String(error?.message || 'GameBanana could not post the comment. Please retry.')
        .replace(/^Error invoking remote method '[^']+': Error:\s*/i, '');
}

async function send() {
    if (isSendingComment || disposed) return;

    const textarea = document.getElementById('comment');
    const sendButton = document.getElementById('sendBtn');
    const comment = textarea.value.trim();

    if (!comment) {
        await htmlAlert(
            'Comment not sent',
            'Enter a comment before sending.',
            [{ text: 'OK', resolveWith: 'ok' }],
            'error'
        );
        textarea.focus();
        return;
    }

    isSendingComment = true;
    sendButton.disabled = true;
    sendButton.setAttribute('aria-busy', 'true');
    sendButton.textContent = 'Sending...';

    try {
        const posted = await window.deltamodBackend.invoke(
            'leaveCommentGamebanana',
            [gbModID, comment, gbModel]
        );
        if (posted !== true) {
            throw new Error('GameBanana did not confirm that the comment was posted.');
        }

        if (disposed) return;
        textarea.value = '';
        await htmlAlert(
            'Comment posted',
            'Your comment was posted successfully.',
            [{ text: 'View comments', resolveWith: 'ok' }],
            'check_circle'
        );
        window._pageArguments = {
            'id': gbModID,
            'model': gbModel,
            'commentPage': commentPage
        };
        page('gamebanana-leave-comment');
    } catch (error) {
        console.error('Failed to post GameBanana comment:', error);
        await htmlAlert(
            'Comment not sent',
            commentErrorMessage(error),
            [{ text: 'Try again', resolveWith: 'retry' }],
            'error'
        );
        if (!disposed) textarea.focus();
    } finally {
        isSendingComment = false;
        if (sendButton.isConnected) {
            sendButton.disabled = false;
            sendButton.removeAttribute('aria-busy');
            sendButton.textContent = 'Send comment';
        }
    }
}

window.currentPageStack.send = send;

function renderComment(comment, parent, depth = 0) {
    const box = document.createElement('article'); box.className = 'commentBox';
    const avatar = document.createElement('img'); avatar.width = 40; avatar.height = 40;
    avatar.alt = ''; avatar.loading = 'lazy'; avatar.decoding = 'async';
    avatar.src = comment._aPoster?._sAvatarUrl || './img/mod-placeholder.png';
    avatar.onerror = () => { avatar.onerror = null; avatar.src = './img/mod-placeholder.png'; };
    const content = document.createElement('div'); content.className = 'commentArea';
    const author = document.createElement('strong'); author.textContent = comment._aPoster?._sName || 'GameBanana user';
    const copy = document.createElement('p'); copy.textContent = String(comment._sText || '').replace(/<[^>]+>/g, '');
    content.append(author, copy);
    const stamps = document.createElement('div'); stamps.className = 'stampsArea';
    for (const stamp of comment._aStamps || []) {
        const item = document.createElement('span'); item.className = 'stamp';
        item.textContent = `${stamp._sTitle} × ${stamp._nCount}`; stamps.append(item);
    }
    if (stamps.childElementCount) content.append(stamps);
    if (comment._nReplyCount > 0) {
        // Replies are fetched on demand, not recursively for every visible post.
        const expand = document.createElement('button'); expand.type = 'button'; expand.className = 'secondary-action';
        expand.textContent = `Replies (${comment._nReplyCount})`; expand.setAttribute('aria-expanded', 'false');
        const replies = document.createElement('div'); replies.className = 'comment-replies'; replies.hidden = true;
        let loaded = false;
        expand.onclick = async () => {
            if (loaded) { replies.hidden = !replies.hidden; expand.setAttribute('aria-expanded', String(!replies.hidden)); return; }
            expand.disabled = true;
            try {
                const records = await getComments('Post', comment._idRow, 1, 20);
                if (disposed) return;
                for (const record of records) renderComment(record, replies, Math.min(depth + 1, 3));
                if (!records.length) replies.textContent = 'No replies to display.';
                loaded = true; replies.hidden = false; expand.setAttribute('aria-expanded', 'true');
            } catch (error) {
                if (disposed) return;
                replies.hidden = false; window.DeltamodUI.showError(replies, error);
            } finally { expand.disabled = false; }
        };
        content.append(expand, replies);
    }
    box.append(avatar, content); parent.append(box);
}
async function loadComments() {
    recordsRoot.setAttribute('aria-busy', 'true');
    try {
        const records = await getComments(gbModel, gbModID, commentPage, 30);
        if (disposed) return;
        const fragment = document.createDocumentFragment();
        for (const comment of records) renderComment(comment, fragment);
        if (!records.length) {
            const empty = document.createElement('p'); empty.className = 'workspace-load-state';
            empty.textContent = 'No comments to display.'; fragment.append(empty);
        }
        recordsRoot.replaceChildren(fragment);
        const navigation = document.createElement('nav'); navigation.className = 'task-actions'; navigation.setAttribute('aria-label', 'Comment pages');
        const move = (label, delta) => {
            const button = document.createElement('button'); button.type = 'button'; button.className = 'secondary-action'; button.textContent = label;
            button.onclick = () => { navigation.querySelectorAll('button').forEach(item => { item.disabled = true; }); commentPage += delta; void loadComments(); };
            navigation.append(button);
        };
        if (commentPage > 1) move('Previous page', -1);
        if (records.length === 30) move('Next page', 1);
        if (navigation.childElementCount) recordsRoot.append(navigation);
    } catch (error) {
        if (!disposed) window.DeltamodUI.showError(recordsRoot, error, loadComments);
    } finally { recordsRoot.setAttribute('aria-busy', 'false'); }
}
window.deltamodBackend.invokeOptional('getGamebananaPic', [], null).then(picture => {
    if (disposed) return;
    document.getElementById('gbPic').src = picture || './img/mod-placeholder.png';
    document.getElementById('myCommentBox').style.display = picture ? 'flex' : 'none';
}).catch(() => {});
void loadComments();
})();
