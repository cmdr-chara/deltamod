// Shared command-line logging. Native UI logging is owned by the Tauri runtime.
const write = (level, args) => {
    process.stdout.write(`[${level}] ${args.join(' ')}\n`);
};

module.exports = {
    log: (...args) => write('LOG', args),
    warn: (...args) => write('WARN', args),
    error: (...args) => write('ERROR', args),
    info: (...args) => write('INFO', args),
    debug: (...args) => write('DEBUG', args),
    clear: () => process.stdout.write('\x1b[2J\x1b[H'),
};
