// Copyright © 2026 cmdr-chara
// Modified for Deltamod Community on 2026-07-29.
// Licensed under the EUPL 1.2.

const semver = require('semver');

function isNewerVersion(candidate, current, options = {}) {
    const candidateVersion = semver.valid(candidate);
    const currentVersion = semver.valid(current);
    if (!candidateVersion || !currentVersion) return false;
    if (!options.allowPrerelease && semver.prerelease(candidateVersion)) return false;
    return semver.gt(candidateVersion, currentVersion);
}

module.exports = { isNewerVersion };
