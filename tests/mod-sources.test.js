// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const { describe, expect, it } = globalThis;
const {
    BrowseRequest,
    buildModDbCatalogUrl,
    getAvailableProviders,
    isNexusDownloadHost,
    normalizeModDbFeed,
    normalizeNexusMods,
    safeHttpsUrl
} = require('../node/ModSources');

describe('mod source request validation', () => {
    it('accepts the supported providers and rejects unknown ones', () => {
        expect(BrowseRequest.parse({ provider: 'moddb' }).provider).toBe('moddb');
        expect(() => BrowseRequest.parse({ provider: 'example' })).toThrow();
    });

    it('only enables providers explicitly mapped by the selected game', () => {
        const providers = getAvailableProviders({
            gamebanana: { id: 6755 },
            sources: {
                nexus: { domain: 'deltarune' },
                moddb: { slug: 'deltarune' }
            }
        });
        expect(providers.filter(provider => provider.available).map(provider => provider.id))
            .toEqual(['gamebanana', 'nexus', 'moddb']);
        expect(providers.find(provider => provider.id === 'moddb')).toMatchObject({
            name: 'ModDB (recent)',
            catalogScope: 'recent',
            installMode: 'manual'
        });
        expect(getAvailableProviders({ gamebanana: { id: 1 } }).find(provider => provider.id === 'nexus').available)
            .toBe(false);
    });
});

describe('ModDB RSS normalization', () => {
    const feed = `<?xml version="1.0"?>
        <rss version="2.0" xmlns:media="http://search.yahoo.com/mrss/">
          <channel>
            <item>
              <title>Gendered Kris</title>
              <link>https://www.moddb.com/games/deltarune/downloads/gendered-kris</link>
              <pubDate>Fri, 06 Jun 2025 06:26:11 +0000</pubDate>
              <guid isPermaLink="false">downloads291264</guid>
              <description><![CDATA[<img src="https://media.moddb.com/thumb.png"><br>This is a description.]]></description>
              <enclosure url="https://media.moddb.com/thumb.png" type="image/png" />
            </item>
          </channel>
        </rss>`;

    it('produces text-only normalized records with safe URLs', () => {
        const records = normalizeModDbFeed(feed);
        expect(records).toHaveLength(1);
        expect(records[0]).toMatchObject({
            provider: 'moddb',
            id: '291264',
            title: 'Gendered Kris',
            summary: 'This is a description.',
            installMode: 'manual'
        });
        expect(records[0].imageUrl).toBe('https://media.moddb.com/thumb.png');
    });

    it('filters locally and drops lookalike ModDB hosts', () => {
        expect(normalizeModDbFeed(feed, 'missing')).toEqual([]);
        const unsafe = feed.replace(
            'https://www.moddb.com/games/deltarune/downloads/gendered-kris',
            'https://moddb.com.evil.example/file'
        );
        expect(normalizeModDbFeed(unsafe)).toEqual([]);
    });

    it('builds a contained full-catalog URL from the mapped game slug', () => {
        expect(buildModDbCatalogUrl('deltarune'))
            .toBe('https://www.moddb.com/games/deltarune/downloads');
        expect(() => buildModDbCatalogUrl('../deltarune')).toThrow();
    });
});

describe('Nexus Mods normalization and download host containment', () => {
    it('normalizes rating metadata and filters text locally', () => {
        const records = normalizeNexusMods([{
            mod_id: 42,
            name: 'A useful patch',
            summary: '<b>Fixes</b> a thing',
            author: 'Chara',
            updated_timestamp: 1_700_000_000,
            contains_adult_content: true,
            picture_url: 'https://staticdelivery.nexusmods.com/example.png'
        }], 'deltarune', 'useful');
        expect(records).toHaveLength(1);
        expect(records[0]).toMatchObject({
            provider: 'nexus',
            id: '42',
            title: 'A useful patch',
            summary: 'Fixes a thing',
            contentRating: 'adult'
        });
        expect(records[0].sourceUrl).toBe('https://www.nexusmods.com/deltarune/mods/42');
    });

    it('allows Nexus CDNs without accepting suffix lookalikes', () => {
        expect(isNexusDownloadHost('cf-files.nexusmods.com')).toBe(true);
        expect(isNexusDownloadHost('cdn.nexus-cdn.com')).toBe(true);
        expect(isNexusDownloadHost('nexusmods.com.evil.example')).toBe(false);
        expect(safeHttpsUrl('http://staticdelivery.nexusmods.com/a.png', () => true)).toBeNull();
    });
});
