// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2

export const GPUIX_VERSION = '2.0.18';
export const GPUIX_UPDATE_ENDPOINT = 'https://github.com/cmdr-chara/deltamod/releases/latest/download/latest-gpuix.json';
export const GPUIX_UPDATE_PUBLIC_KEY = 'dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDg3OUYyODI4Nzc5NjMwNwpSV1FIWTNtSGd2SjVDTWU5SlF3RTJoR0x6N2VkVGxCWUJCNlZVaVZxZXRocjlBVGpVVzBEaVc1NQo=';

const UNVERIFIED = Object.freeze({ enabled: false, format: null, reason: 'GPUIX automatic updates are disabled until signed package and rollback validation is complete.' });
export function updateCapability(manifest, platform = process.platform, arch = process.arch) {
  const format = { 'win32-x64': 'nsis', 'darwin-arm64': 'app' }[`${platform}-${arch}`];
  // No switch in preferences or environment can turn a development/unsigned
  // package into a release. Linux DEB is never an in-place updater target. The
  // AppImage install/restart contract still needs a separately validated gate.
  if (!manifest || !format || manifest.target !== `${platform}-${arch}` || manifest.appVersion !== GPUIX_VERSION
    || manifest.releaseChannel !== 'stable-gpuix' || manifest.updaterRehearsal !== 'passed') return UNVERIFIED;
  return Object.freeze({ enabled: true, format, reason: '' });
}

async function nativeCheck(version, options) {
  const { checkUpdate } = await import('@gpuix/native');
  return checkUpdate(version, options);
}

export class UpdateRuntime {
  constructor(checkFn = nativeCheck, capability = UNVERIFIED) {
    this.checkFn = checkFn;
    this.capability = Object.freeze({ ...capability });
    this.listeners = new Set();
    this.disposed = false;
    this.state = { status: 'idle', update: null, error: '' };
    this.subscribe = listener => { this.listeners.add(listener); return () => this.listeners.delete(listener); };
    this.getSnapshot = () => this.state;
  }
  update(patch) {
    if (this.disposed) return;
    this.state = { ...this.state, ...patch };
    this.listeners.forEach(listener => listener());
  }
  async check() {
    if (this.disposed || this.state.status === 'checking' || this.state.status === 'installing') return false;
    if (this.capability.enabled !== true) {
      this.update({ status: 'error', error: this.capability.reason || UNVERIFIED.reason, update: null });
      return false;
    }
    this.update({ status: 'checking', error: '', update: null });
    try {
      const update = await this.checkFn(GPUIX_VERSION, {
        endpoints: [GPUIX_UPDATE_ENDPOINT],
        pubkey: GPUIX_UPDATE_PUBLIC_KEY,
        timeoutMs: 30000,
        installMode: 'passive',
      });
      if (this.disposed) return false;
      if (update && (update.format !== this.capability.format || typeof update.downloadAndInstall !== 'function')) {
        throw new Error('The update package format does not match this installation.');
      }
      this.update(update ? { status: 'available', update } : { status: 'current', update: null });
      return true;
    } catch (error) {
      this.update({ status: 'error', update: null, error: error instanceof Error ? error.message : String(error) });
      return false;
    }
  }
  async install() {
    const update = this.state.update;
    if (this.disposed || this.capability.enabled !== true || this.state.status !== 'available' || !update
      || update.format !== this.capability.format) return false;
    this.update({ status: 'installing', error: '' });
    try {
      // GPUIX performs signature verification. A manifest claim or SHA256 file
      // is not substituted for the embedded publisher public key.
      await update.downloadAndInstall();
      if (this.disposed) return false;
      this.update({ status: 'installed', update: null });
      return true;
    } catch (error) {
      this.update({ status: 'error', error: error instanceof Error ? error.message : String(error) });
      return false;
    }
  }
  dispose() {
    this.disposed = true;
    this.listeners.clear();
    this.state = { status: 'idle', update: null, error: '' };
  }
}
