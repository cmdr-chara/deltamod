const semver = require('semver');

function isNewerVersion(candidate, current, options = {}) {
    const candidateVersion = semver.valid(candidate);
    const currentVersion = semver.valid(current);
    if (!candidateVersion || !currentVersion) return false;
    if (!options.allowPrerelease && semver.prerelease(candidateVersion)) return false;
    return semver.gt(candidateVersion, currentVersion);
}

module.exports = { isNewerVersion };
