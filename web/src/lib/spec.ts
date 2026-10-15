// Helpers for the container form: shell-style argument lists and sizes.

import type { ContainerSpec } from './types';

/** Splits a command line into arguments, honoring single and double quotes and backslashes. */
export function shellSplit(s: string): string[] {
  const out: string[] = [];
  let cur = '';
  let inToken = false;
  let quote: '"' | "'" | null = null;
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (quote) {
      if (ch === quote) quote = null;
      else if (ch === '\\' && quote === '"' && i + 1 < s.length && (s[i + 1] === '"' || s[i + 1] === '\\')) cur += s[++i];
      else cur += ch;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
      inToken = true;
    } else if (ch === '\\' && i + 1 < s.length) {
      cur += s[++i];
      inToken = true;
    } else if (/\s/.test(ch)) {
      if (inToken) out.push(cur);
      cur = '';
      inToken = false;
    } else {
      cur += ch;
      inToken = true;
    }
  }
  if (inToken) out.push(cur);
  return out;
}

/** Joins arguments into a command line that shellSplit turns back into the same list. */
export function shellJoin(args: string[]): string {
  return args.map((a) => (a !== '' && /^[\w@%+=:,./-]+$/.test(a) ? a : `'${a.replace(/'/g, `'"'"'`)}'`)).join(' ');
}

const SIZE_UNITS: Record<string, number> = { b: 1, k: 1024, m: 1024 ** 2, g: 1024 ** 3, t: 1024 ** 4 };

/** Parses "512m", "2g", "1.5G", "1024" (bytes) into bytes. Empty means no limit. */
export function parseSize(s: string): number | null | 'invalid' {
  const t = s.trim().toLowerCase().replace(/i?b$/, '');
  if (!t) return null;
  const m = /^(\d+(?:\.\d+)?)\s*([bkmgt]?)$/.exec(t);
  if (!m) return 'invalid';
  return Math.round(Number(m[1]) * SIZE_UNITS[m[2] || 'b']);
}

/** Formats bytes the way parseSize reads them ("512m", "2g"). */
export function formatSize(n: number | null | undefined): string {
  if (!n) return '';
  const units = ['t', 'g', 'm', 'k'] as const;
  for (const u of units) {
    if (n % SIZE_UNITS[u] === 0) return `${n / SIZE_UNITS[u]}${u}`;
  }
  // Not a whole number of any unit: the largest unit that fits, rounded.
  for (const u of units) {
    const v = n / SIZE_UNITS[u];
    if (v >= 1) return `${Math.round(v * 100) / 100}${u}`;
  }
  return String(n);
}

/** A spec with every field set to its empty value. */
export function emptySpec(): ContainerSpec {
  return {
    name: '',
    image: '',
    restart: 'unless-stopped',
    network_mode: 'bridge',
    networks: [],
    ports: [],
    volumes: [],
    env: [],
    labels: [],
    devices: [],
    group_add: [],
    cap_add: [],
    cap_drop: [],
    privileged: false,
    user: '',
    hostname: '',
    working_dir: '',
    command: [],
    entrypoint: [],
    memory: null,
    cpus: null,
    shm_size: null,
    extra_hosts: [],
    dns: [],
    gpus: '',
    tty: false,
    stdin_open: false,
    init: false,
    healthcheck: null,
    log_driver: '',
    log_opts: [],
    security_opt: [],
    sysctls: [],
    tmpfs: [],
  };
}
