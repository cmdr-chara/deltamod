// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-30.
// Licensed under the EUPL 1.2.

function normalizeProgressFraction(value) {
    const fraction = Number(value);
    if (!Number.isFinite(fraction)) return 0;
    return Math.min(1, Math.max(0, fraction));
}

module.exports = { normalizeProgressFraction };
