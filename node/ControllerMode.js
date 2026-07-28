const { spawn } = require('child_process');
const path = require('path');

var proc;
var running = false;

function start() {
    if (running) return;

    running = true;
    const exepath = path.join(__dirname, '..', 'tools', 'cmodeutil.exe');
    proc = spawn(exepath, [], {
        windowsHide: true,
        shell: false,
        stdio: 'ignore'
    });
    proc.once('error', () => {
        running = false;
        proc = null;
    });
    proc.once('close', () => {
        running = false;
        proc = null;
    });
}

function stop() {
    if (!running) return;
    
    running = false;
    try {
        proc?.kill();
    }
    catch (e) {}
    proc = null;
}

module.exports = {
    start: start,
    stop: stop
};
