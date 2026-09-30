// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2

export const GPUIX_VERSION = '2.0.18';
export const GPUIX_UPDATE_ENDPOINT = 'https://github.com/cmdr-chara/deltamod/releases/latest/download/latest-gpuix.json';
export const GPUIX_UPDATE_PUBLIC_KEY = 'dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDg3OUYyODI4Nzc5NjMwNwpSV1FIWTNtSGd2SjVDTWU5SlF3RTJoR0x6N2VkVGxCWUJCNlZVaVZxZXRocjlBVGpVVzBEaVc1NQo=';

async function nativeCheck(version, options) {
  const { checkUpdate } = await import('@gpuix/native');
  return checkUpdate(version, options);
}

export class UpdateRuntime {
  constructor(checkFn = nativeCheck) {
    this.checkFn = checkFn;
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
    this.update({ status: 'checking', error: '', update: null });
    try {
      const update = await this.checkFn(GPUIX_VERSION, {
        endpoints: [GPUIX_UPDATE_ENDPOINT],
        pubkey: GPUIX_UPDATE_PUBLIC_KEY,
        timeoutMs: 30000,
        installMode: 'passive',
      });
      if (this.disposed) return false;
      this.update(update ? { status: 'available', update } : { status: 'current', update: null });
      return true;
    } catch (error) {
      this.update({ status: 'error', update: null, error: error instanceof Error ? error.message : String(error) });
      return false;
    }
  }
  async install() {
    const update = this.state.update;
    if (this.disposed || this.state.status !== 'available' || !update) return false;
    this.update({ status: 'installing', error: '' });
    try {
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
