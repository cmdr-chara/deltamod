# Source Provenance

This document records the source and licensing provenance of Deltamod Community. It is an evidence record for maintainers and contributors; it does not replace [`LICENSE.txt`](./LICENSE.txt) and is not legal advice.

## Project origin

Deltamod Community is an independent modified fork of the upstream [`deltamodders/deltamod`](https://github.com/deltamodders/deltamod) project.

The software in this repository remains distributed under the **European Union Public Licence v1.2 (EUPL-1.2)** except where a separately identified third-party component or asset carries its own applicable terms. The authoritative licence text for this repository is [`LICENSE.txt`](./LICENSE.txt).

Git commit history is the authoritative record of individual modifications, authorship metadata, and modification dates.

## Preserved upstream licensing boundary

On 2026-08-22 at 18:56:22 UTC, upstream commit [`93d58d66dd8b67b1931e61a49b443a09ed16c8e8`](https://github.com/deltamodders/deltamod/commit/93d58d66dd8b67b1931e61a49b443a09ed16c8e8) added a separate **All Rights Reserved** notice to `node/Accounts/Itch.js` and added an Itch.io-specific licensing statement to the upstream README.

The direct parent of that commit is:

`5d60c62814b28f87abb5b3eb7a309161e488c313`

At [`5d60c62814b28f87abb5b3eb7a309161e488c313`](https://github.com/deltamodders/deltamod/commit/5d60c62814b28f87abb5b3eb7a309161e488c313), immediately before the restrictive notice was added:

- the upstream README stated that the software was licensed under EUPL-1.2;
- `node/Accounts/Itch.js` existed without the later All Rights Reserved header.

Deltamod Community records `5d60c62814b28f87abb5b3eb7a309161e488c313` as a **historical upstream provenance boundary** for licensing review. This marker does not, by itself, make a legal conclusion about any particular file or later upstream contribution.

## Upstream import policy

For upstream changes after the provenance boundary above:

1. licensing and provenance must be reviewed before import;
2. code carrying terms incompatible with this repository's distribution obligations must not be copied into the Community codebase without appropriate permission;
3. where a useful feature is contested or separately restricted, maintainers should prefer an independently written implementation based on public specifications, public APIs, and independently gathered requirements;
4. imported code must retain notices required by its applicable licence;
5. the exact upstream commit(s) used should remain traceable in Git history or review records.

This policy is intended to keep future upstream synchronization auditable rather than treating all later upstream commits as automatically importable.

## Community modifications

Community-specific development is recorded in this repository's Git history. Major changes should remain attributable to their commits and pull requests rather than being represented as upstream work.

When code is independently implemented in Community, contributors should avoid copying implementation details from sources carrying incompatible or unclear terms. Public protocols, API documentation, observed interoperable behavior, and independently developed tests should be preferred as implementation references.

## Releases and traceability

Community release tags and CI metadata should identify the exact source revision used to produce a release. Release artifacts, checksums, and build attestations should be retained where the release workflow provides them.

A release binary should therefore be traceable back to:

- a specific Community tag or commit;
- the corresponding repository licence and notices;
- the relevant third-party dependency and asset notices;
- the CI/build record used to produce it.

## Third-party material

Dependencies, APIs, artwork, trademarks, service names, and other third-party material may have terms independent of the EUPL-1.2 software licence. Their inclusion or mention does not imply endorsement, affiliation, or a transfer of trademark rights.

See [`NOTICE.md`](./NOTICE.md) for the repository-level attribution and independence notice.
