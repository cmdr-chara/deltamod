const fs = require('node:fs');

const path = 'web/index.js';
let source = fs.readFileSync(path, 'utf8');

const guard = `    if (
        pageN !== 'main'
        || document.documentElement.classList.contains('deltamod-route-pending')
        || !window.deltamodBackend.isCommandAvailable('benchmark:rendererReady')
    ) return false;`;

const instrumentedGuard = `    if (
        pageN !== 'main'
        || document.documentElement.classList.contains('deltamod-route-pending')
        || !window.deltamodBackend.isCommandAvailable('benchmark:rendererReady')
    ) {
        console.error('[benchmark readiness skipped]', JSON.stringify({
            pageN,
            routePending: document.documentElement.classList.contains('deltamod-route-pending'),
            commandAvailable: window.deltamodBackend.isCommandAvailable('benchmark:rendererReady')
        }));
        return false;
    }`;

if (!source.includes(guard)) throw new Error('benchmark readiness guard not found');
source = source.replace(guard, instrumentedGuard);

const swallowed = 'signalBenchmarkReadiness().catch(() => {});';
const logged = 'signalBenchmarkReadiness().catch(error => console.error("[benchmark readiness]", error));';
if (!source.includes(swallowed)) throw new Error('benchmark route-ready hook not found');
source = source.replace(swallowed, logged);

fs.writeFileSync(path, source);
