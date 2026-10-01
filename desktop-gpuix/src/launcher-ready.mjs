// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import path from 'node:path';

/** Private launcher handshake. It means delivery readiness, not GPU paint,
 * successful import or update trust. Root identity stays on the private pipe. */
export function createLauncherReady(env = process.env, output = process.stdout) {
  const nonce = env.DELTAMOD_GPUIX_LAUNCH_NONCE;
  delete env.DELTAMOD_GPUIX_LAUNCH_NONCE; // Never propagate it to backend/tools.
  if (nonce !== undefined && !/^[a-f0-9]{32}$/.test(nonce)) throw new Error('Invalid native launcher handshake.');
  let sent = false;
  return options => {
    if (sent || nonce === undefined) return;
    const { stateRoot, managedDataRoot = '' } = options;
    const valid = value => typeof value === 'string' && Buffer.byteLength(value) <= 8192
      && !/[\x00-\x1f\x7f]/.test(value) && path.isAbsolute(value);
    if (!valid(stateRoot) || (managedDataRoot !== '' && !valid(managedDataRoot))) throw new Error('Invalid native launcher data-root identity.');
    sent = true;
    output.write(`\nGPUIX-LAUNCH-READY ${JSON.stringify({ nonce, stateRoot, managedDataRoot })}\n`);
  };
}
