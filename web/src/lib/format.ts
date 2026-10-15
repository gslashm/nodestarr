// Formatting helpers for sizes, rates, times and container states.

import type { ContainerSummary, UpdateInfo } from './types';

const UNITS = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB'];

export function bytes(n: number | null | undefined, digits = 1): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '–';
  if (n < 1024) return `${Math.round(n)} B`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? Math.round(v) : v.toFixed(digits)} ${UNITS[i]}`;
}

export function rate(bytesPerSec: number | null | undefined): string {
  if (bytesPerSec === null || bytesPerSec === undefined) return '–';
  return `${bytes(bytesPerSec)}/s`;
}

export function pct(n: number | null | undefined, digits = 0): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '–';
  return `${n.toFixed(digits)}%`;
}

/** "1 file", "2,048 files". */
export function plural(n: number, word: string, pluralWord = `${word}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? word : pluralWord}`;
}

/** Compact duration: "45s", "12m", "3h 20m", "4d 6h". */
export function duration(secs: number | null | undefined): string {
  if (secs === null || secs === undefined || !Number.isFinite(secs)) return '–';
  const s = Math.max(0, Math.floor(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
  const d = Math.floor(h / 24);
  return h % 24 ? `${d}d ${h % 24}h` : `${d}d`;
}

const now = () => Date.now() / 1000;

/** "just now", "5 min ago", "in 3 h", "2 days ago". */
export function ago(ts: number | null | undefined): string {
  if (!ts) return 'never';
  const diff = now() - ts;
  const future = diff < 0;
  const a = Math.abs(diff);
  let text: string;
  if (a < 45) return future ? 'in a moment' : 'just now';
  else if (a < 3600) text = `${Math.round(a / 60)} min`;
  else if (a < 86400) text = `${Math.round(a / 3600)} h`;
  else if (a < 86400 * 60) text = plural(Math.round(a / 86400), 'day');
  else text = plural(Math.round(a / (86400 * 30)), 'month');
  return future ? `in ${text}` : `${text} ago`;
}

let hour12 = false;

/** Applies the 12h/24h preference from settings. */
export function setTimeFormat(format: string): void {
  hour12 = format === '12h';
}

/** A time of day in the configured format: "03:30" or "3:30 AM". */
export function clockTime(h: number, m: number): string {
  const mm = String(m).padStart(2, '0');
  if (hour12) return `${((h + 11) % 12) + 1}:${mm} ${h < 12 ? 'AM' : 'PM'}`;
  return `${String(h).padStart(2, '0')}:${mm}`;
}

export function dateTime(ts: number | null | undefined): string {
  if (!ts) return '–';
  const d = new Date(ts * 1000);
  const sameYear = d.getFullYear() === new Date().getFullYear();
  return d.toLocaleString(undefined, {
    year: sameYear ? undefined : 'numeric',
    month: 'short',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    hour12,
  });
}

export function time(ts: number | null | undefined, seconds = false): string {
  if (!ts) return '–';
  return new Date(ts * 1000).toLocaleTimeString(undefined, {
    hour: '2-digit',
    minute: '2-digit',
    second: seconds ? '2-digit' : undefined,
    hour12,
  });
}

/** Axis label for a chart tick: the time for short spans, the date for long ones. */
export function tickLabel(ts: number, spanSecs: number): string {
  const d = new Date(ts * 1000);
  if (spanSecs > 2 * 86400) return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
  return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hour12 });
}

export function shortDigest(d: string | null | undefined): string {
  if (!d) return '–';
  return d.replace(/^sha256:/, '').slice(0, 12);
}

/** Drops the default registry noise: "docker.io/library/nginx:1" → "nginx:1". */
export function shortImage(image: string): string {
  return image.replace(/^docker\.io\//, '').replace(/^library\//, '');
}

export function imageTag(image: string): string {
  const noDigest = image.split('@')[0];
  const lastSlash = noDigest.lastIndexOf('/');
  const colon = noDigest.lastIndexOf(':');
  return colon > lastSlash ? noDigest.slice(colon + 1) : 'latest';
}

export function imageRepo(image: string): string {
  const noDigest = image.split('@')[0];
  const lastSlash = noDigest.lastIndexOf('/');
  const colon = noDigest.lastIndexOf(':');
  return colon > lastSlash ? noDigest.slice(0, colon) : noDigest;
}

// ---------------------------------------------------------------- state

export type StateKey = 'running' | 'healthy' | 'unhealthy' | 'starting' | 'restarting' | 'paused' | 'stopped' | 'crashed' | 'created' | 'dead';

export function stateOf(c: Pick<ContainerSummary, 'state' | 'health' | 'exit_code' | 'oom_killed'>): StateKey {
  switch (c.state) {
    case 'running':
      if (c.health === 'unhealthy') return 'unhealthy';
      if (c.health === 'starting') return 'starting';
      if (c.health === 'healthy') return 'healthy';
      return 'running';
    case 'restarting':
      return 'restarting';
    case 'paused':
      return 'paused';
    case 'created':
      return 'created';
    case 'dead':
      return 'dead';
    default:
      return c.oom_killed || (c.exit_code !== null && c.exit_code !== 0 && c.exit_code !== 143 && c.exit_code !== 137)
        ? 'crashed'
        : 'stopped';
  }
}

export const STATE_LABEL: Record<StateKey, string> = {
  running: 'Running',
  healthy: 'Healthy',
  unhealthy: 'Unhealthy',
  starting: 'Starting',
  restarting: 'Restarting',
  paused: 'Paused',
  stopped: 'Stopped',
  crashed: 'Exited',
  created: 'Never started',
  dead: 'Dead',
};

/** CSS color variable for a state. */
export function stateColor(s: StateKey): string {
  switch (s) {
    case 'running':
    case 'healthy':
      return 'var(--ok)';
    case 'unhealthy':
    case 'crashed':
    case 'dead':
      return 'var(--bad)';
    case 'starting':
    case 'restarting':
      return 'var(--warn)';
    case 'paused':
      return 'var(--paused)';
    default:
      return 'var(--idle)';
  }
}

export function isUp(s: StateKey): boolean {
  return s === 'running' || s === 'healthy' || s === 'unhealthy' || s === 'starting';
}

/** What an update would bring, in a few words ("5.0.0.2600", "new build", "tag 4.1"). */
export function updateLabel(u: UpdateInfo | null | undefined): string | null {
  if (!u) return null;
  if (u.status === 'pending_recreate') return 'Pulled, recreate to apply';
  if (u.status !== 'available') return null;
  if (u.newer_tag) return u.newer_tag.tag;
  return u.latest_version ?? 'New build';
}

export function hasUpdate(u: UpdateInfo | null | undefined): boolean {
  return !!u && (u.status === 'available' || u.status === 'pending_recreate');
}

/** Replaces the "{host}" placeholder in links with the host the browser used. */
export function resolveLink(url: string): string {
  return url.replaceAll('{host}', location.hostname);
}

export function titleCase(s: string): string {
  return s.replace(/[_-]+/g, ' ').replace(/^\w/, (c) => c.toUpperCase());
}

/** Who did something, from the server's actor string ("user:jake", "api:ha", "schedule"). */
export function actorLabel(actor: string | null | undefined): string {
  if (!actor) return 'Berth';
  if (actor.startsWith('user:')) return actor.slice(5);
  if (actor.startsWith('api:')) return `API key "${actor.slice(4)}"`;
  switch (actor) {
    case 'system':
      return 'Berth';
    case 'auto-update':
      return 'Automatic update';
    case 'schedule':
      return 'Schedule';
    case 'homeassistant':
      return 'Home Assistant';
    default:
      return actor;
  }
}
