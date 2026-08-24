# Community original-work evidence

This document records a **conservative, evidence-backed subset** of files first introduced as new files in Deltamod Community history. It is intended to make authorship and licence provenance easier to verify if code is reused elsewhere.

It is not a claim that every unlisted Community change lacks copyright, and it does not claim ownership of inherited upstream code or third-party material.

## Evidence standard

A file is registered here only when:

1. the file appears as **added** (`A`) in the recorded Community commit;
2. the recorded commit is part of this repository's history;
3. the file carries an in-file `SPDX-FileCopyrightText` notice;
4. the file carries an in-file `SPDX-License-Identifier: EUPL-1.2` notice; and
5. the repository provenance check verifies the record against Git history.

The machine-readable source of truth is [`provenance/community-original-work.json`](./provenance/community-original-work.json).

## Registered files

| File | First recorded Community commit | Date | Evidence |
| --- | --- | --- | --- |
| `scripts/build-installer.js` | [`552b90adc50757317bf4c8e86b02b7fb8f0fde56`](https://github.com/cmdr-chara/deltamod/commit/552b90adc50757317bf4c8e86b02b7fb8f0fde56) | 2026-08-14 | Added as a new file |
| `web/installer/index.js` | [`552b90adc50757317bf4c8e86b02b7fb8f0fde56`](https://github.com/cmdr-chara/deltamod/commit/552b90adc50757317bf4c8e86b02b7fb8f0fde56) | 2026-08-14 | Added as a new file |
| `web/installer/index.html` | [`552b90adc50757317bf4c8e86b02b7fb8f0fde56`](https://github.com/cmdr-chara/deltamod/commit/552b90adc50757317bf4c8e86b02b7fb8f0fde56) | 2026-08-14 | Added as a new file |
| `scripts/acquire-undertale-mod-tool.js` | [`3e936db6887d55fa8f27e4ad90745d2fae965c3a`](https://github.com/cmdr-chara/deltamod/commit/3e936db6887d55fa8f27e4ad90745d2fae965c3a) | 2026-08-08 | Added as a new file |
| `scripts/lib/undertale-mod-tool-provenance.js` | [`3e936db6887d55fa8f27e4ad90745d2fae965c3a`](https://github.com/cmdr-chara/deltamod/commit/3e936db6887d55fa8f27e4ad90745d2fae965c3a) | 2026-08-08 | Added as a new file |
| `scripts/smoke-game-patching-csx.js` | [`3e936db6887d55fa8f27e4ad90745d2fae965c3a`](https://github.com/cmdr-chara/deltamod/commit/3e936db6887d55fa8f27e4ad90745d2fae965c3a) | 2026-08-08 | Added as a new file |
| `scripts/verify-undertale-mod-tool.js` | [`3e936db6887d55fa8f27e4ad90745d2fae965c3a`](https://github.com/cmdr-chara/deltamod/commit/3e936db6887d55fa8f27e4ad90745d2fae965c3a) | 2026-08-08 | Added as a new file |
| `tests/undertale-mod-tool-provenance.test.js` | [`3e936db6887d55fa8f27e4ad90745d2fae965c3a`](https://github.com/cmdr-chara/deltamod/commit/3e936db6887d55fa8f27e4ad90745d2fae965c3a) | 2026-08-08 | Added as a new file |

## What the record proves

The record is designed to establish that the listed file path was introduced as a new file at the stated commit and that the current file carries a specific copyright/licence notice. It does **not**, by itself, prove that every line was independently invented, resolve every possible copyright question, or replace legal analysis of a dispute.

Git history, commit metadata, review records, contemporaneous releases, and the actual file contents should be preserved together as evidence.

## Adding another file

When a new Community-original file is created:

1. add the appropriate SPDX copyright and EUPL identifier at the top of the file;
2. merge the file through normal review so its first commit remains traceable;
3. add its path, first commit, and date to the JSON registry;
4. add the same record to this document; and
5. run `node scripts/verify-community-provenance.js` from a full Git clone.

Do not register inherited or mixed-history files merely because Community later changed them. For those files, use Git history and precise contribution-level evidence instead of a whole-file ownership claim.
