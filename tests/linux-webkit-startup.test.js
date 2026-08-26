const fs = require('fs');
const path = require('path');

const projectRoot = path.join(__dirname, '..');

function source(relativePath) {
    return fs.readFileSync(path.join(projectRoot, relativePath), 'utf8');
}

describe('Linux WebKitGTK startup guard', () => {
    it('configures the Linux graphics environment during Tauri setup before WebKit windows exist', () => {
        const main = source('src-tauri/src/main.rs');
        const state = source('src-tauri/src/state.rs');
        const controller = source('src-tauri/src/controller.rs');

        expect(main).toContain('.setup(|app| {');
        expect(main).toContain('state::AppState::initialize_with_app(');
        expect(state).toContain('crate::controller::install_protocols(&app)?;');
        expect(controller).toMatch(
            /#\[cfg\(target_os = "linux"\)\]\n#\[path = "linux_webkit\.rs"\]\nmod linux_webkit;/
        );

        const installProtocols = controller.slice(
            controller.indexOf('pub fn install_protocols'),
            controller.indexOf('fn is_protocol_url')
        );
        expect(installProtocols).toMatch(
            /#\[cfg\(target_os = "linux"\)\]\n\s*linux_webkit::configure\(\);/
        );
        expect(installProtocols.indexOf('linux_webkit::configure();'))
            .toBeLessThan(installProtocols.indexOf('#[cfg(target_os = "macos")]'));
    });

    it('only auto-pins Mesa for a non-NVIDIA DRM GPU with native NVIDIA userspace', () => {
        const guard = source('src-tauri/src/linux_webkit.rs');

        expect(guard).toContain('"0x8086" | "0x1002" => has_intel_or_amd = true');
        expect(guard).toContain('"0x10de" => has_nvidia = true');
        expect(guard).toContain('libnvidia-eglcore.so');
        expect(guard).toContain('libnvidia-gpucomp.so');
        expect(guard).toContain('libEGL_nvidia.so');
        expect(guard).toContain('libegl_mesa');
        expect(guard).toContain('env::set_var(EGL_VENDOR_VAR, path.as_os_str());');
        expect(guard).toContain('Deliberately do not');
        expect(guard).toContain('inspect /usr/lib32');

        const autoRisk = guard.slice(
            guard.indexOf('RequestedMode::Auto if risk_detected =>'),
            guard.indexOf('RequestedMode::Auto => {}')
        );
        expect(autoRisk).toContain('plan.pin_mesa_egl = probe.mesa_egl_vendor.clone();');
        expect(autoRisk).not.toContain('plan.force_shm = true');
        expect(autoRisk).not.toContain('plan.disable_dmabuf = true');
    });

    it('preserves explicit WebKit, EGL and GPU-routing overrides', () => {
        const guard = source('src-tauri/src/linux_webkit.rs');

        for (const variable of [
            'WEBKIT_DMABUF_RENDERER_FORCE_SHM',
            'WEBKIT_DISABLE_DMABUF_RENDERER',
            'WEBKIT_DISABLE_COMPOSITING_MODE',
            'WEBKIT_WEB_RENDER_DEVICE_FILE',
            '__EGL_VENDOR_LIBRARY_FILENAMES',
            '__EGL_VENDOR_LIBRARY_DIRS',
            '__NV_PRIME_RENDER_OFFLOAD',
            '__NV_PRIME_RENDER_OFFLOAD_PROVIDER',
            'DRI_PRIME',
            '__GLX_VENDOR_LIBRARY_NAME',
            'GBM_BACKEND'
        ]) {
            expect(guard).toContain(variable);
        }

        expect(guard).toContain('existing WebKit renderer override preserved');
        expect(guard).toContain('existing EGL vendor override preserved');
        expect(guard).toContain('explicit GPU routing override preserved');
    });

    it('provides reversible troubleshooting modes without making them the automatic default', () => {
        const guard = source('src-tauri/src/linux_webkit.rs');

        expect(guard).toContain('DELTAMOD_LINUX_WEBKIT_RENDERER');
        expect(guard).toContain('"native" | "default"');
        expect(guard).toContain('"shm" | "shared-memory" | "shared_memory"');
        expect(guard).toContain('"disable" | "disable-dmabuf" | "legacy"');
        expect(guard).toContain('let normalized = raw.unwrap_or("auto")');
    });
});
