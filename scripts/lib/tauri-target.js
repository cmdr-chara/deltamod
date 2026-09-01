const SUPPORTED_TARGETS = new Set([
    'x86_64-pc-windows-msvc',
    'x86_64-unknown-linux-gnu',
    'x86_64-apple-darwin',
    'aarch64-apple-darwin'
]);

function hostTarget(platform = process.platform, arch = process.arch) {
    const key = `${platform}-${arch}`;
    const targets = {
        'win32-x64': 'x86_64-pc-windows-msvc',
        'linux-x64': 'x86_64-unknown-linux-gnu',
        'darwin-x64': 'x86_64-apple-darwin',
        'darwin-arm64': 'aarch64-apple-darwin'
    };
    return targets[key] || null;
}

function resolveTauriTarget(argument, env = process.env, platform = process.platform, arch = process.arch) {
    const target = argument || env.TAURI_BUILD_TARGET || env.RUST_TARGET || hostTarget(platform, arch);
    if (!SUPPORTED_TARGETS.has(target)) {
        throw new Error('Target must be x86_64-pc-windows-msvc, x86_64-unknown-linux-gnu, x86_64-apple-darwin, or aarch64-apple-darwin.');
    }
    return target;
}

module.exports = { SUPPORTED_TARGETS, hostTarget, resolveTauriTarget };
