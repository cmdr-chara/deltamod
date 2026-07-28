const APPLICATION_SCHEME = 'deltamod-community';

function parseLaunch(value) {
    if (typeof value !== 'string' || !value.startsWith(`${APPLICATION_SCHEME}://`)) return null;
    const raw = value.slice(`${APPLICATION_SCHEME}://`.length).replace(/^\/+|\/+$/g, '');
    const parts = raw.split('/');
    return {
        command: String(parts.shift() || '').toLowerCase(),
        arguments: parts
    };
}

module.exports = {
    APPLICATION_SCHEME,
    parseLaunch
};
