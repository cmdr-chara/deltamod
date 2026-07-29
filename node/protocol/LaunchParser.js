const APPLICATION_SCHEME = 'deltamod-community';

function parseLaunch(value) {
    if (typeof value !== 'string' || !value.startsWith(`${APPLICATION_SCHEME}://`)) return null;
    const raw = value.slice(`${APPLICATION_SCHEME}://`.length);
    const hashIndex = raw.indexOf('#');
    const withoutHash = hashIndex === -1 ? raw : raw.slice(0, hashIndex);
    const queryIndex = withoutHash.indexOf('?');
    const route = (queryIndex === -1 ? withoutHash : withoutHash.slice(0, queryIndex))
        .replace(/^\/+|\/+$/g, '');
    const parts = route ? route.split('/') : [];
    const launch = {
        command: String(parts.shift() || '').toLowerCase(),
        arguments: parts
    };
    if (queryIndex !== -1) {
        const query = new URLSearchParams(withoutHash.slice(queryIndex + 1));
        launch.parameters = {};
        for (const [name, parameterValue] of query) {
            if (Object.hasOwn(launch.parameters, name)) {
                throw new Error(`Protocol parameter "${name}" was provided more than once.`);
            }
            launch.parameters[name] = parameterValue;
        }
    }
    return launch;
}

module.exports = {
    APPLICATION_SCHEME,
    parseLaunch
};
