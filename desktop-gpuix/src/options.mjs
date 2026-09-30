// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { homedir } from 'node:os';
import path from 'node:path';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { parseIntent } from './deep-links.mjs';

export function parseOptions(argv, env = process.env, platform = process.platform) {
  const developmentRoot = path.resolve(fileURLToPath(new URL('../..', import.meta.url)));
  const executableDir = path.dirname(process.execPath);
  const packagedCandidates = platform === 'darwin'
    ? [path.resolve(executableDir, '../Resources/deltamod')]
    : platform === 'linux'
      ? [path.resolve(executableDir, '../lib/deltamod'), ...(env.APPDIR ? [path.join(env.APPDIR, 'usr', 'lib', 'deltamod')] : [])]
      : [path.join(executableDir, 'deltamod')];
  const explicit = env.DELTAMOD_GPUIX_RESOURCES ? path.resolve(env.DELTAMOD_GPUIX_RESOURCES) : '';
  const root = explicit || packagedCandidates.find(candidate => existsSync(path.join(candidate, 'games'))) || developmentRoot;
  const defaultState = platform === 'win32'
    ? path.join(env.LOCALAPPDATA || path.join(homedir(), 'AppData', 'Local'), 'DeltamodCommunityGPUIX')
    : platform === 'darwin' ? path.join(homedir(), 'Library', 'Application Support', 'DeltamodCommunityGPUIX')
    : path.join(env.XDG_DATA_HOME || path.join(homedir(), '.local', 'share'), 'deltamod-community-gpuix');
  const options = { resourcesRoot: root, stateRoot: defaultState, sourceProfile: '',
    executable: existsSync(path.join(executableDir, `deltamod-gpuix-host${platform === 'win32' ? '.exe' : ''}`))
      ? path.join(executableDir, `deltamod-gpuix-host${platform === 'win32' ? '.exe' : ''}`)
      : path.join(developmentRoot, 'desktop-gpuix', 'native', 'target', 'release', `deltamod-gpuix-host${platform === 'win32' ? '.exe' : ''}`),
    focus: true, benchmarkFile: '', reducedMotion: null, opaque: null, openLink: '', managedDataRoot: '' };
  const flags = new Map([['--resources-root', 'resourcesRoot'], ['--state-root', 'stateRoot'],
    ['--source-profile', 'sourceProfile'], ['--managed-data-root', 'managedDataRoot'], ['--backend', 'executable'], ['--benchmark-file', 'benchmarkFile']]);
  const seen = new Set();
  for (let i = 0; i < argv.length; i++) {
    const flag = argv[i];
    if (seen.has(flag)) throw new Error(`Duplicate argument: ${flag}`);
    seen.add(flag);
    if (flag === '--no-focus') { options.focus = false; continue; }
    if (flag === '--reduce-motion') { options.reducedMotion = true; continue; }
    if (flag === '--opaque') { options.opaque = true; continue; }
    if (flag === '--open') {
      const raw = argv[++i];
      parseIntent(raw);
      options.openLink = raw;
      continue;
    }
    const key = flags.get(flag);
    if (!key || !argv[i + 1] || argv[i + 1].startsWith('--')) throw new Error(`Unknown or incomplete argument: ${flag}`);
    options[key] = path.resolve(argv[++i]);
  }
  if (options.openLink && options.benchmarkFile) throw new Error('Link handling cannot alter a benchmark launch.');
  return options;
}

export function openModPage(raw, spawnImpl = spawn, platform = process.platform) {
  // Never pass an arbitrary provider URL, filesystem path or shell expression to
  // the OS opener. This stage supports GameBanana public mod pages only.
  if (typeof raw !== 'string' || !/^https:\/\/gamebanana\.com\/mods\/[1-9]\d*$/.test(raw)) {
    return Promise.reject(new Error('The mod page URL is not allowed.'));
  }
  const [command, args] = platform === 'win32'
    ? ['rundll32.exe', ['url.dll,FileProtocolHandler', raw]]
    : platform === 'darwin' ? ['open', [raw]] : ['xdg-open', [raw]];
  return new Promise((resolve, reject) => {
    const child = spawnImpl(command, args, { shell: false, windowsHide: true, stdio: 'ignore' });
    child.once('error', reject);
    child.once('exit', code => code === 0 ? resolve() : reject(new Error('The browser could not be opened.')));
  });
}
