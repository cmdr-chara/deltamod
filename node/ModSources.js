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
// Nexus expects interactive clients to request only the page the user asked
// for.  Keep the bound explicit even if the API omits totalCount or changes
// its pagination behaviour in a future response.
const NEXUS_MAX_CATALOG_PAGES = 1;
const NEXUS_MAX_CATALOG_ITEMS = NEXUS_PAGE_SIZE * NEXUS_MAX_CATALOG_PAGES;
const NEXUS_CATALOG_CACHE_TTL_MS = 60 * 1000;
const NEXUS_CATALOG_CACHE_MAX_ENTRIES = 64;
const NEXUS_MIN_REQUEST_INTERVAL_MS = 1000;
const NEXUS_RATE_LIMIT_FALLBACK_MS = 60 * 1000;

const nexusCatalogCache = new Map();
const nexusCatalogInFlight = new Map();
let nexusRequestQueue = Promise.resolve();
let nexusLastRequestAt = 0;
let nexusQuotaPauseUntil = 0;

class NexusRateLimitError extends Error {
    constructor(message, { status = 429, retryAfterMs = null, retryAt = null, quota } = {}) {
        super(message);
        this.name = 'NexusRateLimitError';
        this.code = 'NEXUS_RATE_LIMITED';
        this.status = Number(status) || 429;
        this.retryAfterMs = Number.isFinite(Number(retryAfterMs))
            ? Math.max(0, Math.round(Number(retryAfterMs)))
            : null;
        this.retryAt = retryAt || (this.retryAfterMs == null
            ? null
            : new Date(Date.now() + this.retryAfterMs).toISOString());
        this.quota = quota || emptyNexusQuota();
    }
}

function emptyNexusQuota() {
    return {
        daily: { limit: null, remaining: null, resetAt: null },
        hourly: { limit: null, remaining: null, resetAt: null }
    };
}

function parseHeaderNumber(headers, name) {
    const value = headers?.get?.(name);
    if (value == null || String(value).trim() === '') return null;
    const parsed = Number(value);
    return Number.isFinite(parsed) ? parsed : null;
}

function parseHeaderResetAt(headers, name) {
    const value = headers?.get?.(name);
    if (value == null || String(value).trim() === '') return null;
    const raw = String(value).trim();
    const numeric = Number(raw);
    const timestamp = Number.isFinite(numeric)
        ? (numeric < 1e12 ? numeric * 1000 : numeric)
        : Date.parse(raw);
    return Number.isFinite(timestamp) ? new Date(timestamp).toISOString() : null;
}

function parseNexusQuotaHeaders(headers) {
    const quota = emptyNexusQuota();
    for (const period of ['daily', 'hourly']) {
        quota[period] = {
            limit: parseHeaderNumber(headers, `x-rl-${period}-limit`),
            remaining: parseHeaderNumber(headers, `x-rl-${period}-remaining`),
            resetAt: parseHeaderResetAt(headers, `x-rl-${period}-reset`)
        };
    }
    return quota;
}

function parseRetryAfter(value, now = Date.now()) {
    if (value == null || String(value).trim() === '') return null;
    const raw = String(value).trim();
    const seconds = Number(raw);
    if (Number.isFinite(seconds) && seconds >= 0) return Math.round(seconds * 1000);
    const timestamp = Date.parse(raw);
    return Number.isFinite(timestamp) ? Math.max(0, timestamp - now) : null;
}

function quotaResetDelayMs(quota, now = Date.now()) {
    const resetTimes = ['daily', 'hourly']
        .map(period => quota?.[period]?.resetAt)
        .filter(Boolean)
        .map(value => Date.parse(value))
        .filter(timestamp => Number.isFinite(timestamp) && timestamp > now);
    if (!resetTimes.length) return null;
    return Math.min(...resetTimes) - now;
}

function getNexusRateMetadata(response, now = Date.now()) {
    const quota = parseNexusQuotaHeaders(response?.headers);
    const retryAfterMs = parseRetryAfter(response?.headers?.get?.('retry-after'), now)
        ?? quotaResetDelayMs(quota, now);
    const retryAt = retryAfterMs == null
        ? null
        : new Date(now + retryAfterMs).toISOString();
    return { quota, retryAfterMs, retryAt };
}

function noteNexusQuota(quota, now = Date.now()) {
    const resetDelay = quotaResetDelayMs(quota, now);
    const exhausted = ['daily', 'hourly'].some(period =>
        quota?.[period]?.remaining != null && quota[period].remaining <= 0
    );
    if (exhausted && resetDelay != null) {
        nexusQuotaPauseUntil = Math.max(nexusQuotaPauseUntil, now + resetDelay);
    }
}

function delay(ms) {
    if (!(ms > 0)) return Promise.resolve();
    return new Promise(resolve => setTimeout(resolve, ms));
}

// Serialize requests through one small client-side gate.  This prevents a
// burst of searches/download metadata calls from competing for the same API
// quota while still allowing the caller to observe the real response/error.
function scheduleNexusRequest(task) {
    const operation = nexusRequestQueue.then(async () => {
        const now = Date.now();
        const waitUntil = Math.max(
            nexusLastRequestAt + NEXUS_MIN_REQUEST_INTERVAL_MS,
            nexusQuotaPauseUntil
        );
        await delay(waitUntil - now);
        try {
            return await task();
        } finally {
            nexusLastRequestAt = Date.now();
        }
    });
    nexusRequestQueue = operation.catch(() => undefined);
    return operation;
}

async function nexusFetch(url, options = {}) {
    return scheduleNexusRequest(async () => {
        const result = await fetchWithValidatedRedirects(url, options);
        const metadata = getNexusRateMetadata(result.response);
        noteNexusQuota(metadata.quota);
        if (result.response.status === 429) {
            const retryAfterMs = metadata.retryAfterMs ?? NEXUS_RATE_LIMIT_FALLBACK_MS;
            const retryAt = metadata.retryAt
                ?? new Date(Date.now() + retryAfterMs).toISOString();
            nexusQuotaPauseUntil = Math.max(nexusQuotaPauseUntil, Date.now() + retryAfterMs);
            throw new NexusRateLimitError(
                'Nexus Mods rate limit reached. Please wait before trying again.',
                {
                    status: result.response.status,
                    quota: metadata.quota,
                    retryAfterMs,
                    retryAt
                }
            );
        }
        return { ...result, ...metadata };
    });
}

function clearNexusRequestPolicyState() {
    nexusCatalogCache.clear();
    nexusCatalogInFlight.clear();
    nexusRequestQueue = Promise.resolve();
    nexusQuotaPauseUntil = 0;
    nexusLastRequestAt = 0;
}

function cacheNexusCatalog(cacheKey, value) {
    if (!nexusCatalogCache.has(cacheKey)
        && nexusCatalogCache.size >= NEXUS_CATALOG_CACHE_MAX_ENTRIES) {
        const oldestKey = nexusCatalogCache.keys().next().value;
        if (oldestKey !== undefined) nexusCatalogCache.delete(oldestKey);
    }
    nexusCatalogCache.set(cacheKey, {
        value,
        expiresAt: Date.now() + NEXUS_CATALOG_CACHE_TTL_MS
    });
}
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
    const { response, quota, retryAfterMs, retryAt } = await nexusFetch(
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
        error.retryAfterMs = retryAfterMs;
        error.retryAt = retryAt;
        error.quota = quota;
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

async function fetchBoundedNexusMods({ domain, query, sort }) {
    // Deliberately request one server page only.  The UI can issue another
    // bounded query when the user changes the search/sort; it must never turn
    // one interaction into an unbounded catalogue crawl.
    const page = await fetchNexusModsPage({ domain, query, sort, offset: 0 });
    const records = page.records.slice(0, NEXUS_MAX_CATALOG_ITEMS);
    const totalCount = page.totalCount;
    return {
        records,
        totalCount,
        hasMore: totalCount == null
            ? page.records.length >= NEXUS_PAGE_SIZE
            : totalCount > records.length
    };
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
        const error = new Error('Nexus Mods single sign-on is required.');
        error.code = 'NEXUS_SSO_REQUIRED';
        throw error;
    }
    const url = new URL(pathname, `https://${NEXUS_API_HOST}/v1/`);
    const { response, quota, retryAfterMs, retryAt } = await nexusFetch(url, {
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
        error.retryAfterMs = retryAfterMs;
        error.retryAt = retryAt;
        error.quota = quota;
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
    const normalizedDomain = String(domain || '').trim();
    if (!/^[a-z0-9][a-z0-9-]{0,79}$/i.test(normalizedDomain)) {
        const error = new Error('This game does not have a valid Nexus Mods source mapping.');
        error.code = 'MOD_SOURCE_UNAVAILABLE';
        throw error;
    }
    const normalizedQuery = String(query || '').trim();
    const normalizedSort = ['latest_added', 'latest_updated', 'trending'].includes(sort)
        ? sort
        : 'latest_added';
    const cacheKey = [
        normalizedDomain.toLocaleLowerCase(),
        normalizedQuery.toLocaleLowerCase(),
        normalizedSort
    ].join('\u0000');

    const cached = nexusCatalogCache.get(cacheKey);
    if (cached?.expiresAt > Date.now()) return cloneNexusCatalog(cached.value);
    if (cached) nexusCatalogCache.delete(cacheKey);

    const existing = nexusCatalogInFlight.get(cacheKey);
    if (existing) return existing.then(cloneNexusCatalog);

    const pending = fetchBoundedNexusMods({
        domain: normalizedDomain,
        query: normalizedQuery,
        sort: normalizedSort
    }).then(({ records, totalCount, hasMore }) => {
        const items = normalizeNexusMods(records, normalizedDomain, normalizedQuery);
        items.sort((left, right) => Number(right.featured) - Number(left.featured));
        return {
            provider: 'nexus',
            catalogScope: 'page',
            hasMore,
            totalCount,
            attribution: 'Metadata and popularity counts provided by Nexus Mods',
            items
        };
    }).then(result => {
        cacheNexusCatalog(cacheKey, result);
        return result;
    }).finally(() => {
        // Failed promises are never retained as an in-flight or completed
        // cache entry; a later user action can retry normally.
        if (nexusCatalogInFlight.get(cacheKey) === pending) {
            nexusCatalogInFlight.delete(cacheKey);
        }
    });

    nexusCatalogInFlight.set(cacheKey, pending);
    return pending.then(cloneNexusCatalog);
}

function cloneNexusCatalog(value) {
    return {
        ...value,
        items: Array.isArray(value?.items)
            ? value.items.map(item => ({ ...item }))
            : []
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
    NexusRateLimitError,
    buildModDbCatalogUrl,
    buildNexusSearchVariables,
    browseModDb,
    browseNexus,
    clearNexusRequestPolicyState,
    getAvailableProviders,
    getNexusPrimaryDownload,
    isModDbPublicHost,
    isNexusDownloadHost,
    isNexusPublicHost,
    normalizeModDbFeed,
    normalizeNexusMods,
    parseNexusQuotaHeaders,
    parseRetryAfter,
    safeHttpsUrl,
    nexusRequest,
    validateNexusApiKey
};
