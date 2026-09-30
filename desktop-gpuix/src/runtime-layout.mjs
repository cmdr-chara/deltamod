// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';

export const GPUIX_VERSION = '0.10.0';
export const TARGETS = Object.freeze({
  'win32-x64': { triple: 'x86_64-pc-windows-msvc', addon: 'gpuix-native.win32-x64-msvc.node', formats: ['nsis'] },
  'darwin-arm64': { triple: 'aarch64-apple-darwin', addon: 'gpuix-native.darwin-arm64.node', formats: ['app'] },
  'linux-x64': { triple: 'x86_64-unknown-linux-gnu', addon: 'gpuix-native.linux-x64-gnu.node', formats: ['deb', 'appimage'] },
});
export function targetFor(platform = process.platform, arch = process.arch) {
  const target = TARGETS[`${platform}-${arch}`];
  if (!target) throw new Error(`GPUIX ${GPUIX_VERSION} has no reviewed native package target for ${platform}-${arch}.`);
  return { ...target, platform, arch, id: `${platform}-${arch}` };
}

/** Resolve every component, rejecting links, traversal, devices and empty files.
 * The package directory is the trust boundary, not a renderer-provided filename. */
export function regularFile(root, relative, maxBytes = 512 * 1024 * 1024) {
  if (typeof relative !== 'string' || relative.length > 1024 || /[\\:\x00-\x1f\x7f]/.test(relative)
    || relative.split('/').some(part => !part || part === '.' || part === '..') || path.isAbsolute(relative)) {
    throw new Error('Invalid packaged resource path.');
  }
  let current = fs.realpathSync(root);
  const parts = relative.split('/');
  for (let i = 0; i < parts.length; i++) {
    current = path.join(current, parts[i]);
    const stat = fs.lstatSync(current);
    if (stat.isSymbolicLink() || (i < parts.length - 1 ? !stat.isDirectory() : !stat.isFile())) {
      throw new Error('Packaged resources must not contain links or special files.');
    }
    if (i === parts.length - 1 && (stat.size === 0 || stat.size > maxBytes)) throw new Error('Packaged resource exceeds its byte limit.');
  }
  return current;
}
export function readJsonResource(root, relative, maxBytes = 256 * 1024) {
  return JSON.parse(fs.readFileSync(regularFile(root, relative, maxBytes), 'utf8'));
}
export function fileDigest(filename) {
  const hash = createHash('sha256');
  const fd = fs.openSync(filename, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
  try {
    const stat = fs.fstatSync(fd);
    if (!stat.isFile() || stat.size > 512 * 1024 * 1024) throw new Error('Invalid packaged binary.');
    const buffer = Buffer.alloc(1024 * 1024);
    let count;
    while ((count = fs.readSync(fd, buffer, 0, buffer.length, null)) !== 0) hash.update(buffer.subarray(0, count));
    return hash.digest('hex');
  } finally { fs.closeSync(fd); }
}

/** Header checks reject accidental cross-target staging. They do not establish
 * code-signing identity, dependencies, ABI compatibility or executable safety. */
export function binaryTarget(header) {
  if (!Buffer.isBuffer(header) || header.length < 64) throw new Error('Truncated native binary.');
  if (header.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46]))) {
    if (header[4] !== 2 || header[5] !== 1 || header.readUInt16LE(18) !== 62) throw new Error('Unsupported ELF target.');
    return 'linux-x64';
  }
  if (header.readUInt32LE(0) === 0xfeedfacf) {
    if (header.readUInt32LE(4) !== 0x0100000c) throw new Error('Unsupported Mach-O target.');
    return 'darwin-arm64';
  }
  if (header.toString('ascii', 0, 2) === 'MZ') {
    const offset = header.readUInt32LE(60);
    if (offset < 64 || offset + 6 > header.length || header.toString('binary', offset, offset + 4) !== 'PE\0\0'
      || header.readUInt16LE(offset + 4) !== 0x8664) throw new Error('Unsupported PE target.');
    return 'win32-x64';
  }
  throw new Error('Unsupported native executable format.');
}
export function verifyBinary(filename, expectedTarget) {
  const fd = fs.openSync(filename, fs.constants.O_RDONLY | (fs.constants.O_NOFOLLOW || 0));
  try {
    const bytes = Buffer.alloc(64 * 1024);
    const count = fs.readSync(fd, bytes, 0, bytes.length, 0);
    if (!fs.fstatSync(fd).isFile() || binaryTarget(bytes.subarray(0, count)) !== expectedTarget) throw new Error('Packaged native target mismatch.');
  } finally { fs.closeSync(fd); }
  if (!expectedTarget.startsWith('win32-') && (fs.statSync(filename).mode & 0o111) === 0 && !filename.endsWith('.node')) {
    throw new Error('Packaged helper is not executable.');
  }
}
export function checkedArtifact(root, record, target) {
  if (!record || typeof record !== 'object' || typeof record.sha256 !== 'string' || !/^[a-f0-9]{64}$/.test(record.sha256)) {
    throw new Error('Invalid native artifact record.');
  }
  const file = regularFile(root, record.path);
  verifyBinary(file, target);
  if (fileDigest(file) !== record.sha256) throw new Error('Packaged native artifact checksum mismatch.');
  return file;
}

export function readRuntimeManifest(resourcesRoot, target = targetFor()) {
  const manifest = readJsonResource(resourcesRoot, 'native/runtime.json');
  if (!manifest || manifest.schemaVersion !== 1 || manifest.gpuixVersion !== GPUIX_VERSION || manifest.target !== target.id) {
    throw new Error('Native runtime manifest version or target mismatch.');
  }
  return manifest;
}

/** Must run before importing @gpuix/native. Development can use installed npm
 * packages. A compiled executable must never silently fall back to development
 * paths, a different addon, a WASI binding or an environment-selected library. */
export function prepareNativeRuntime(options, env = process.env, executable = process.execPath) {
  const packaged = /^deltamod-gpuix(?:\.exe)?$/i.test(path.basename(executable));
  const exists = fs.existsSync(path.join(options.resourcesRoot, 'native', 'runtime.json'));
  if (!exists && !packaged) return null;
  const target = targetFor();
  const manifest = readRuntimeManifest(options.resourcesRoot, target);
  const addon = checkedArtifact(options.resourcesRoot, manifest.addon, target.id);
  if (path.basename(addon) !== target.addon) throw new Error('Unexpected GPUIX addon filename.');
  const host = regularFile(path.dirname(options.executable), path.basename(options.executable));
  verifyBinary(host, target.id);
  if (fileDigest(host) !== manifest.hostSha256) throw new Error('Native host does not match the packaged manifest.');
  if (env.NAPI_RS_FORCE_WASI && env.NAPI_RS_FORCE_WASI !== '0') throw new Error('WASI is not a supported desktop runtime.');
  env.NAPI_RS_NATIVE_LIBRARY_PATH = addon;
  return manifest;
}
