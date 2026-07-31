// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const cheerio = require('cheerio');
const convert = require('xml-js');
const { z } = require('zod');
const { fetchWithValidatedRedirects, validateRemoteUrl } = require('./security/RemoteSecurity');

const ProviderId = z.enum(['gamebanana', 'nexus', 'moddb']);
const BrowseRequest = z.object({
    provider: ProviderId,
    query: z.string().trim().max(120).optional().default(''),
    sort: z.enum(['latest_added', 'latest_updated', 'trending']).optional().default('latest_added')
});

const MODDB_FEED_HOST = 'rss.moddb.com';
const NEXUS_API_HOST = 'api.nexusmods.com';
const MAX_CATALOG_BYTES = 4 * 1024 * 1024;
const NEXUS_PAGE_SIZE = 50;
const NEXUS_CATALOG_QUERY = `
    query BrowseMods($filter: ModsFilter, $sort: [ModsSort!], $offset: Int, $count: Int) {
        mods(filter: $filter, sort: $sort, offset: $offset, count: $count) {
            totalCount
            nodes {
                modId
                name
                summary
                author
                updatedAt
                pictureUrl
                adultContent
                downloads
                endorsements
            }
        }
    }
`;

function hostMatches(hostname, approved) {
    const host = String(hostname || '').toLowerCase().replace(/\.$/, '');
    return approved.some(item => host === item || host.endsWith(`.${item}`));
}

const isModDbFeedHost = hostname =>
    String(hostname || '').toLowerCase().replace(/\.$/, '') === MODDB_FEED_HOST;
const isModDbPublicHost = hostname => hostMatches(hostname, ['moddb.com']);
const isModDbImageHost = hostname => hostMatches(hostname, ['media.moddb.com', 'static.moddb.com']);
const isNexusApiHost = hostname =>
    String(hostname || '').toLowerCase().replace(/\.$/, '') === NEXUS_API_HOST;
const isNexusPublicHost = hostname => hostMatches(hostname, ['nexusmods.com']);
const isNexusDownloadHost = hostname => hostMatches(hostname, [
    'nexusmods.com',
    'nexus-cdn.com'
]);

function safeHttpsUrl(value, allowedHosts) {
    if (!value) return null;
    try {
        return validateRemoteUrl(String(value), { allowedHosts }).toString();
    } catch {
        return null;
    }
}

async function readLimitedText(response, maximumBytes = MAX_CATALOG_BYTES) {
    const advertised = Number(response.headers.get('content-length')) || 0;
    if (advertised > maximumBytes) {
        const error = new Error('The remote catalogue is larger than the safety limit.');
        error.code = 'REMOTE_CATALOG_TOO_LARGE';
        throw error;
    }
    const text = await response.text();
    if (Buffer.byteLength(text, 'utf8') > maximumBytes) {
        const error = new Error('The remote catalogue exceeded the safety limit.');
        error.code = 'REMOTE_CATALOG_TOO_LARGE';
        throw error;
    }
    return text;
}

function xmlText(node) {
    if (node == null) return '';
    if (typeof node === 'string') return node;
    return String(node._cdata ?? node._text ?? '');
}

function stripMarkup(value) {
    const $ = cheerio.load(String(value || ''));
    $('img,script,style').remove();
    return $.root().text().replace(/\s+/g, ' ').trim();
}

function normalizeModDbFeed(xml, query = '') {
    const parsed = convert.xml2js(xml, { compact: true, trim: true });
    const rawItems = parsed?.rss?.channel?.item;
    const items = rawItems == null ? [] : (Array.isArray(rawItems) ? rawItems : [rawItems]);
    const needle = String(query || '').trim().toLocaleLowerCase();

    return items.map(item => {
        const guid = xmlText(item.guid);
        const sourceUrl = safeHttpsUrl(xmlText(item.link), isModDbPublicHost);
        const imageUrl = safeHttpsUrl(
            item.enclosure?._attributes?.url
                || item['media:content']?.['media:thumbnail']?._attributes?.url,
            isModDbImageHost
        );
        const title = xmlText(item.title) || 'Untitled ModDB download';
        const summary = stripMarkup(
            xmlText(item['media:content']?.['media:description'])
                || xmlText(item.description)
        );
        return {
            provider: 'moddb',
            id: (guid.match(/\d+/) || [guid || sourceUrl])[0],
            title,
            summary,
            author: 'ModDB contributor',
            updatedAt: xmlText(item.pubDate),
            imageUrl,
            sourceUrl,
            contentRating: 'unknown',
            installMode: 'manual',
            actionLabel: 'Open download page'
        };
    }).filter(item => {
        if (!item.sourceUrl) return false;
        if (!needle) return true;
        return `${item.title} ${item.summary}`.toLocaleLowerCase().includes(needle);
    });
}

function normalizeNexusMods(records, domain, query = '') {
    const needle = String(query || '').trim().toLocaleLowerCase();
    return (Array.isArray(records) ? records : []).map(mod => {
        const modId = Number(mod.mod_id ?? mod.modId);
        return {
            provider: 'nexus',
            id: String(modId),
            title: String(mod.name || `Nexus mod ${modId}`),
            summary: stripMarkup(mod.summary || mod.description || ''),
            author: String(mod.author || mod.uploaded_by || 'Nexus Mods contributor'),
            updatedAt: mod.updated_time || mod.updatedAt || (
                Number.isFinite(Number(mod.updated_timestamp))
                    ? new Date(Number(mod.updated_timestamp) * 1000).toISOString()
                    : ''
            ),
            imageUrl: safeHttpsUrl(mod.picture_url || mod.pictureUrl, isNexusPublicHost),
            sourceUrl: `https://www.nexusmods.com/${encodeURIComponent(domain)}/mods/${modId}`,
            contentRating: (mod.contains_adult_content ?? mod.adultContent) ? 'adult' : 'general',
            downloads: Math.max(0, Number(mod.downloads) || 0),
            endorsements: Math.max(0, Number(mod.endorsements) || 0),
            featured: String(domain).toLowerCase() === 'deltarune' && modId === 23,
            installMode: 'nexus',
            actionLabel: 'Download'
        };
    }).filter(item => {
        if (!/^\d+$/.test(item.id) || Number(item.id) <= 0) return false;
        if (!needle) return true;
        return `${item.title} ${item.summary} ${item.author}`.toLocaleLowerCase().includes(needle);
    });
}

function buildNexusSearchVariables(domain, query = '', sort = 'latest_added', offset = 0) {
    const secondarySort = {
        latest_added: 'createdAt',
        latest_updated: 'updatedAt',
        trending: 'endorsements'
    }[sort] || 'createdAt';
    const normalizedQuery = String(query || '').trim();
    const filter = {
        op: 'AND',
        gameDomainName: [{ value: domain, op: 'EQUALS' }]
    };
    if (normalizedQuery) {
        filter.nameStemmed = [{ value: normalizedQuery, op: 'MATCHES' }];
    }
    return {
        filter,
        sort: normalizedQuery
            ? [
                { relevance: { direction: 'DESC' } },
                { [secondarySort]: { direction: 'DESC' } }
            ]
            : sort === 'trending'
                ? [
                    { endorsements: { direction: 'DESC' } },
                    { downloads: { direction: 'DESC' } }
                ]
                : [{ [secondarySort]: { direction: 'DESC' } }],
        offset: Math.max(0, Number(offset) || 0),
        count: NEXUS_PAGE_SIZE
    };
}

async function fetchNexusModsPage({ domain, query, sort, offset }) {
    const { response } = await fetchWithValidatedRedirects(
        `https://${NEXUS_API_HOST}/v2/graphql`,
        {
            allowedHosts: isNexusApiHost,
            maximumRedirects: 0,
            method: 'POST',
            headers: {
                accept: 'application/json',
                'content-type': 'application/json',
                'application-name': 'Deltamod Community',
                'application-version': require('../package.json').version
            },
            body: JSON.stringify({
                query: NEXUS_CATALOG_QUERY,
                variables: buildNexusSearchVariables(domain, query, sort, offset)
            })
        }
    );
    if (!response.ok) {
        const error = new Error(`Nexus Mods search failed with HTTP ${response.status}.`);
        error.code = 'MOD_SOURCE_REQUEST_FAILED';
        error.status = response.status;
        throw error;
    }
    const payload = JSON.parse(await readLimitedText(response));
    const page = payload?.data?.mods;
    const records = page?.nodes;
    if (payload?.errors?.length || !Array.isArray(records)) {
        const error = new Error('Nexus Mods could not complete this catalogue search.');
        error.code = 'MOD_SOURCE_REQUEST_FAILED';
        throw error;
    }
    return {
        records,
        totalCount: Number.isInteger(page.totalCount) && page.totalCount >= 0
            ? page.totalCount
            : null
    };
}

async function fetchAllNexusMods({ domain, query, sort }) {
    const records = [];
    const seenModIds = new Set();
    let offset = 0;
    let totalCount = null;

    do {
        const page = await fetchNexusModsPage({ domain, query, sort, offset });
        totalCount = page.totalCount ?? totalCount;
        for (const record of page.records) {
            const modId = String(record?.modId ?? record?.mod_id ?? '');
            if (!seenModIds.has(modId)) {
                seenModIds.add(modId);
                records.push(record);
            }
        }
        offset += page.records.length;
        if (page.records.length === 0) break;
    } while (totalCount === null
        ? offset % NEXUS_PAGE_SIZE === 0
        : offset < totalCount);

    return records;
}

function buildModDbCatalogUrl(slug) {
    const normalizedSlug = String(slug || '');
    if (!/^[a-z0-9][a-z0-9-]{0,79}$/i.test(normalizedSlug)) {
        const error = new Error('This game does not have a valid ModDB source mapping.');
        error.code = 'MOD_SOURCE_UNAVAILABLE';
        throw error;
    }
    return `https://www.moddb.com/games/${encodeURIComponent(normalizedSlug)}/downloads`;
}

async function browseModDb({ slug, query = '' }) {
    const catalogUrl = buildModDbCatalogUrl(slug);
    const url = `https://${MODDB_FEED_HOST}/games/${slug}/downloads/feed/rss.xml`;
    const { response } = await fetchWithValidatedRedirects(url, {
        allowedHosts: isModDbFeedHost,
        headers: { 'User-Agent': 'Deltamod-Community/2 ModDB-RSS' }
    });
    if (!response.ok) {
        const error = new Error(`ModDB catalogue request failed with HTTP ${response.status}.`);
        error.code = 'MOD_SOURCE_REQUEST_FAILED';
        throw error;
    }
    return {
        provider: 'moddb',
        catalogScope: 'recent',
        catalogUrl,
        attribution: 'Recent downloads supplied by the ModDB RSS feed. This is not the complete ModDB catalogue.',
        items: normalizeModDbFeed(await readLimitedText(response), query)
    };
}

async function nexusRequest(pathname, apiKey) {
    const key = String(apiKey || '').trim();
    if (!/^[A-Za-z0-9+/=_-]{20,200}$/.test(key)) {
        const error = new Error('A valid personal Nexus Mods API key is required.');
        error.code = 'NEXUS_API_KEY_REQUIRED';
        throw error;
    }
    const url = new URL(pathname, `https://${NEXUS_API_HOST}/v1/`);
    const { response } = await fetchWithValidatedRedirects(url, {
        allowedHosts: isNexusApiHost,
        headers: {
            apikey: key,
            accept: 'application/json',
            'application-name': 'Deltamod Community',
            'application-version': require('../package.json').version
        }
    });
    if (!response.ok) {
        const error = new Error(
            response.status === 401 || response.status === 403
                ? 'Nexus Mods rejected the API key or this operation.'
                : `Nexus Mods request failed with HTTP ${response.status}.`
        );
        error.code = response.status === 401 || response.status === 403
            ? 'NEXUS_AUTH_FAILED'
            : 'MOD_SOURCE_REQUEST_FAILED';
        error.status = response.status;
        throw error;
    }
    return JSON.parse(await readLimitedText(response));
}

async function validateNexusApiKey(apiKey) {
    const user = await nexusRequest('users/validate.json', apiKey);
    return {
        valid: true,
        name: String(user.name || 'Nexus Mods user'),
        userId: Number(user.user_id) || null,
        premium: Boolean(user.is_premium),
        supporter: Boolean(user.is_supporter)
    };
}

async function browseNexus({ domain, query = '', sort = 'latest_added' }) {
    if (!/^[a-z0-9][a-z0-9-]{0,79}$/i.test(String(domain || ''))) {
        const error = new Error('This game does not have a valid Nexus Mods source mapping.');
        error.code = 'MOD_SOURCE_UNAVAILABLE';
        throw error;
    }
    const normalizedQuery = String(query || '').trim();
    const records = await fetchAllNexusMods({
        domain: String(domain),
        query: normalizedQuery,
        sort
    });
    const items = normalizeNexusMods(records, domain, normalizedQuery);
    items.sort((left, right) => Number(right.featured) - Number(left.featured));
    return {
        provider: 'nexus',
        attribution: 'Metadata and popularity counts provided by Nexus Mods',
        items
    };
}

async function getNexusPrimaryDownload({ domain, modId, apiKey }) {
    const numericModId = Number(modId);
    if (!Number.isInteger(numericModId) || numericModId <= 0) {
        const error = new Error('Invalid Nexus Mods mod identifier.');
        error.code = 'INVALID_MOD_SOURCE_ID';
        throw error;
    }
    const fileResult = await nexusRequest(
        `games/${encodeURIComponent(domain)}/mods/${numericModId}/files.json`,
        apiKey
    );
    const files = Array.isArray(fileResult?.files) ? fileResult.files : [];
    const eligible = files.filter(file =>
        Number.isInteger(Number(file.file_id))
        && !['DELETED', 'ARCHIVED', 'OLD_VERSION'].includes(String(file.category_name || '').toUpperCase())
    );
    const selected = eligible.find(file => file.is_primary)
        || eligible.find(file => String(file.category_name || '').toUpperCase() === 'MAIN')
        || eligible[0];
    if (!selected) {
        const error = new Error('No downloadable Nexus Mods file is available for this mod.');
        error.code = 'NEXUS_FILE_UNAVAILABLE';
        throw error;
    }

    const links = await nexusRequest(
        `games/${encodeURIComponent(domain)}/mods/${numericModId}/files/${Number(selected.file_id)}/download_link.json`,
        apiKey
    );
    const link = (Array.isArray(links) ? links : []).find(item => item?.URI)?.URI;
    const downloadUrl = safeHttpsUrl(link, isNexusDownloadHost);
    if (!downloadUrl) {
        const error = new Error('Nexus Mods did not return an approved download link. Non-premium users may need to download from the website.');
        error.code = 'NEXUS_MANUAL_DOWNLOAD_REQUIRED';
        throw error;
    }
    const advertisedBytes = (Number(selected.size_kb || selected.size) || 0) * 1024;
    return {
        downloadUrl,
        fileId: Number(selected.file_id),
        fileName: String(selected.file_name || selected.name || `nexus-${numericModId}`),
        maximumBytes: advertisedBytes > 0
            ? Math.min(
                Math.max(advertisedBytes + 1024 * 1024, 16 * 1024 * 1024),
                2 * 1024 * 1024 * 1024
            )
            : 2 * 1024 * 1024 * 1024
    };
}

function getAvailableProviders(game) {
    const sources = game?.sources || {};
    return [
        { id: 'gamebanana', name: 'GameBanana', available: Boolean(game?.gamebanana?.id) },
        { id: 'nexus', name: 'Nexus Mods', available: Boolean(sources.nexus?.domain), requiresAuthentication: true },
        {
            id: 'moddb',
            name: 'ModDB (recent)',
            available: Boolean(sources.moddb?.slug),
            catalogScope: 'recent',
            installMode: 'manual'
        }
    ];
}

module.exports = {
    BrowseRequest,
    ProviderId,
    buildModDbCatalogUrl,
    buildNexusSearchVariables,
    browseModDb,
    browseNexus,
    getAvailableProviders,
    getNexusPrimaryDownload,
    isModDbPublicHost,
    isNexusDownloadHost,
    isNexusPublicHost,
    normalizeModDbFeed,
    normalizeNexusMods,
    safeHttpsUrl,
    validateNexusApiKey
};
