// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
export const GPUIX_VERSION: string;
export const TARGETS: Readonly<Record<string, { triple: string; addon: string; formats: string[] }>>;
export interface RuntimeTarget { id: string; platform: string; arch: string; triple: string; addon: string; formats: string[] }
export function targetFor(platform?: string, arch?: string): RuntimeTarget;
export function regularFile(root: string, relative: string, maxBytes?: number): string;
export function readJsonResource(root: string, relative: string, maxBytes?: number): any;
export function fileDigest(filename: string): string;
export function binaryTarget(header: Buffer): string;
export function verifyBinary(filename: string, expectedTarget: string): void;
export function checkedArtifact(root: string, record: { path: string; sha256: string }, target: string): string;
export function readRuntimeManifest(resourcesRoot: string, target?: RuntimeTarget): any;
export function prepareNativeRuntime(options: { resourcesRoot: string; executable: string }, env?: NodeJS.ProcessEnv, executable?: string): any;
