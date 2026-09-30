// SPDX-FileCopyrightText: 2026 Deltamod Community contributors
// SPDX-License-Identifier: EUPL-1.2
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
const exec = promisify(execFile);
export function parsePs(stdout) {
  return stdout.trim().split(/\r?\n/).filter(Boolean).map(line => {
    const fields = line.trim().split(/\s+/).map(Number);
    if (fields.length !== 3 || fields.some(value => !Number.isSafeInteger(value) || value < 0)) throw new Error('Malformed process snapshot.');
    return { pid: fields[0], parent: fields[1], bytes: fields[2] * 1024 };
  });
}
export function sumTree(records, pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0 || records.some(row =>
    !Number.isSafeInteger(row.pid) || row.pid < 0 || !Number.isSafeInteger(row.parent) || row.parent < 0
    || !Number.isSafeInteger(row.bytes) || row.bytes < 0)) throw new Error('Invalid process identity or memory.');
  const byId = new Map(records.map(row => [row.pid, row]));
  if (!byId.has(pid) || byId.size !== records.length) throw new Error('Missing root or duplicate process ID.');
  const queue = [pid];
  const seen = new Set();
  let bytes = 0;
  while (queue.length) {
    const current = queue.pop();
    if (seen.has(current)) continue;
    seen.add(current);
    if (seen.size > 256) throw new Error('Process tree exceeds benchmark limit.');
    const item = byId.get(current);
    if (!Number.isSafeInteger(item.bytes) || item.bytes < 0) throw new Error('Invalid process memory value.');
    bytes += item.bytes;
    for (const row of records) if (row.parent === current && !seen.has(row.pid)) queue.push(row.pid);
  }
  if (!Number.isSafeInteger(bytes) || bytes <= 0) throw new Error('Invalid process-tree memory.');
  return { bytes, count: seen.size };
}
export async function sampleTree(pid, platform = process.platform) {
  const options = { windowsHide: true, timeout: 5000, maxBuffer: 2 * 1024 * 1024 };
  let records;
  if (platform === 'win32') {
    const { stdout } = await exec('powershell.exe', ['-NoProfile', '-NonInteractive', '-Command',
      'Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,WorkingSetSize | ConvertTo-Json -Compress'], options);
    const parsed = JSON.parse(stdout);
    records = (Array.isArray(parsed) ? parsed : [parsed]).map(row => ({ pid: Number(row.ProcessId), parent: Number(row.ParentProcessId), bytes: Number(row.WorkingSetSize) }));
  } else {
    records = parsePs((await exec('ps', ['-e', '-o', 'pid=,ppid=,rss='], options)).stdout);
  }
  return sumTree(records, pid);
}
export function summarize(samples, field) {
  const values = samples.map(sample => sample[field]).sort((a, b) => a - b);
  if (values.length !== 7 || values.some(value => !Number.isFinite(value) || value <= 0)) throw new Error('Seven positive measurements required.');
  return { minimum: values[0], median: values[3], p95NearestRank: values[6] };
}
