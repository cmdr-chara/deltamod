# Provider capability evidence

This document records the evidence boundary for provider adapters. It was last
reviewed on 2026-08-30. A capability remains unsupported until an official,
stable interface demonstrates it; the absence of public documentation is not
treated as permission to depend on private web endpoints.

Product decision: Game Jolt and itch.io are not Mod Shop providers. Their
adapters are retained only for configured automatic game downloads with known
build/file identifiers. The evidence below records why catalogue discovery is
not exposed in the product.

## Capability policy

- Browser pages may support `external_download`, but are not evidence for
  `direct_download`.
- Account-owned library endpoints are not evidence for public catalogue
  `search`.
- Developer/game telemetry APIs are not evidence for game or mod discovery.
- Private frontend endpoints found in application source or network traces are
  out of scope unless the provider documents them for third-party consumers.
- Authentication secrets, complete request URLs, search terms, and provider
  scopes must not appear in logs, debug output, cache keys, or user-facing raw
  errors.
- Provider failures are normalized before reaching the renderer.

## itch.io

Official evidence:

- The [server-side API](https://itch.io/docs/api/serverside) exposes the
  authenticated account's own games and seller-owned purchase/download-key
  operations. It does not document public catalogue search or arbitrary public
  downloads.
- The unauthenticated `wharf/latest` endpoint returns a latest build identifier
  only for an already-known game and channel. It is evidence for bounded version
  comparison, not discovery or artifact download.
- The [API overview](https://itch.io/docs/api/overview) documents OAuth and RSS
  feeds for public browse pages.
- The [itch app documentation](https://itch.io/docs/itch/using/downloading.html)
  says the app searches the user's local library and sends broader catalogue
  searches to the website.
- [butler](https://itch.io/docs/butler/) is a creator upload/patch tool, not a
  public consumer catalogue API.

Initial adapter boundary:

| Function | Initial support | Evidence constraint |
|---|---:|---|
| Open a known project/source page | Yes | Validated `https://itch.io` or `https://*.itch.io` URL; external handoff only |
| Read an explicitly configured public RSS feed | Yes | Bounded feed parsing; no claim of global search |
| Compare a known game/channel build | Yes | `wharf/latest`; exact identifiers required |
| List the authenticated user's own games | Optional | OAuth/API-key flow with explicit account state |
| Public catalogue search | No | No documented public catalogue API found |
| Arbitrary direct artifact download | No | No documented third-party consumer endpoint found |

## Game Jolt

Official evidence:

- Game Jolt documents a public [game browsing page](https://gamejolt.com/games)
  and recommends its desktop app for searching and installing games in
  [Downloading games](https://ssr.gamejolt.net/help-docs/Shop/download-games).
- The documented [Game API](https://ssr.gamejolt.net/help-docs/creators/game-api)
  covers trophies, scoreboards, sessions, friends, time, and game/user data
  storage. It is not a catalogue or package-download API.
- Game Jolt publishes the source for the
  [site and desktop frontend](https://github.com/gamejolt/gamejolt), but internal
  frontend routes are not a supported third-party integration contract.

Initial adapter boundary:

| Function | Initial support | Evidence constraint |
|---|---:|---|
| Open a known game/project page | Yes | Validated `https://gamejolt.com` URL; external handoff only |
| Open provider catalogue search | Yes | Browser handoff with encoded query; not native provider search |
| Import a user-selected local archive | Yes | Continues through the local-archive provider and lifecycle preflight |
| Native public catalogue API search | No | No documented third-party catalogue API found |
| Direct build/package download | No | No documented third-party consumer endpoint found |
| Use Game API credentials for discovery | No | Credentials and API purpose do not match catalogue discovery |

## Re-review triggers

Revisit this matrix when a provider publishes a new public API, OAuth scope,
download contract, or terms update. Any expanded capability needs fixtures,
normalized error coverage, cache/redaction tests, and an explicit evidence link
before the adapter advertises it.
