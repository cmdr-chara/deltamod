(() => {
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
window.currentPageStack = {};
const nextButton = document.getElementById('next');
if (nextButton && !window.deltamodBackend.isCommandAvailable('npsCallback')) {
    nextButton.disabled = true;
    nextButton.title = 'Continuing from this legacy patch flow is unavailable in this app build';
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

window.currentPageStack.next = function () {
    window.deltamodBackend.invokeOptional('npsCallback', [], false);
}

window.currentPageStack.fp = async function () {
    document.getElementById("patchingTXT").innerHTML = icon('check') + " " + "Patching complete!";
    document.getElementById("patchingTXT").classList.add("success");
    document.getElementById("next").style.display = "block";
}
})();
