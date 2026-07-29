const console = require("./Console");

function approve(domain) {
    // List of approved domains for Deltamod external resources
    var approvedDomains = [
        "fonts.googleapis.com",
        "api.github.com",
        "images.gamebanana.com",
        "avatars.githubusercontent.com",
        "fonts.gstatic.com",
        "gamebanana.com",
        "media.moddb.com",
        "static.moddb.com",
        "images.nexusmods.com",
        "staticdelivery.nexusmods.com"
    ];
    if (!approvedDomains.includes(domain)) {
        console.log("Blocked request to: " + domain);
        return false;
    }
    return true;
}

module.exports = {
    approve
};
