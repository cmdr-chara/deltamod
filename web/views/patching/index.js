(() => {
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
window.currentPageStack = {};
const nextButton = document.getElementById('next');
const hasLegacyNextPatchStep = window.deltamodBackend.isCommandAvailable('npsCallback');
if (nextButton && !hasLegacyNextPatchStep) {
    nextButton.title = 'Return to the mod list';
}
window.currentPageStack.gpl = function (obj) {
    var message = obj.log;
    var percent = obj.percent;

    if (message != "") {
        if (document.getElementById("gpl").innerHTML.length > 10000) {
            document.getElementById("gpl").innerHTML = document.getElementById("gpl").innerHTML.slice(document.getElementById("gpl").innerHTML.length - 8000);
        }
        document.getElementById("gpl").innerHTML += message + "<br>";
        const gplElement = document.getElementById("gpl");
        gplElement.scrollTop = gplElement.scrollHeight;
        gplElement.scrollLeft = 0;
    }
}

window.currentPageStack.next = async function () {
    if (hasLegacyNextPatchStep) {
        await window.deltamodBackend.invokeOptional('npsCallback', [], false);
        return;
    }
    await page('main');
}

window.currentPageStack.fp = async function () {
    document.getElementById("patchingTXT").innerHTML = icon('check') + " " + "Patching complete!";
    document.getElementById("patchingTXT").classList.add("success");
    document.getElementById("next").style.display = "block";
}
})();
