(() => {
const setInterval = (handler, delay, ...args) => {
    const interval = window.setInterval(handler, delay, ...args);
    window._intervals = window._intervals || [];
    window._intervals.push(interval);
    return interval;
};
window.currentPageStack.onDLP = function (perc) {
    document.getElementById("up").value = perc;
}
})();
