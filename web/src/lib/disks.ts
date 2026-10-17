// Words and numbers for the Proxmox disk health pages: verdicts, sizes,
// pool roles and redundancy, scrubs, self-tests and schedules, and what
// the SMART attributes mean.

import { describeCron } from './cron';
import { ago, duration, plural } from './format';
import type { DiskLevel, DiskMedia, DiskRole, DiskTest, DiskView, PoolKind, PoolScan, PoolTolerance, SmartAttribute, TestSchedule } from './types';

export type Tone = 'ok' | 'info' | 'warn' | 'bad' | 'idle';

export const LEVELS: Record<DiskLevel, { label: string; tone: Tone; rank: number }> = {
  ok: { label: 'Healthy', tone: 'ok', rank: 0 },
  info: { label: 'Healthy', tone: 'ok', rank: 0 },
  watch: { label: 'Watch', tone: 'info', rank: 1 },
  warning: { label: 'Needs attention', tone: 'warn', rank: 2 },
  failing: { label: 'Failing', tone: 'bad', rank: 3 },
};

/** A disk's verdict as shown: gone and never-read disks get their own words. */
export function verdictOf(d: Pick<DiskView, 'present' | 'sampled_at' | 'error' | 'verdict'>): { label: string; tone: Tone; rank: number } {
  if (!d.present) return { label: 'Gone', tone: 'idle', rank: -1 };
  if (d.sampled_at === null) return { label: d.error ? 'Unreadable' : 'Not read yet', tone: 'idle', rank: -1 };
  return LEVELS[d.verdict];
}

export const MEDIA: Record<DiskMedia, string> = { hdd: 'HDD', ssd: 'SSD', nvme: 'NVMe', unknown: 'Disk' };

export const POOL_KIND: Record<PoolKind, string> = { zfs: 'ZFS', md: 'mdadm', snapraid: 'SnapRAID', lvmthin: 'LVM-thin' };

/** Disk sizes the way vendors label them (powers of 1000): "4 TB", "500 GB", "1.92 TB". */
export function diskSize(bytes: number | null | undefined): string {
  if (!bytes || !Number.isFinite(bytes)) return '–';
  const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
  let v = bytes;
  let i = 0;
  while (v >= 1000 && i < units.length - 1) {
    v /= 1000;
    i++;
  }
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${Number(v.toFixed(digits))} ${units[i]}`;
}

/** Power-on time: "4.7 years", "120 days", "36 hours". */
export function powerOn(hours: number | null | undefined): string {
  if (hours === null || hours === undefined) return '–';
  if (hours >= 8766) return `${(hours / 8766).toFixed(1)} years`;
  if (hours >= 48) return plural(Math.round(hours / 24), 'day');
  return plural(hours, 'hour');
}

/** How a temperature compares with the disk's limit. */
export function tempTone(t: number | null | undefined, limit: number | null | undefined): Tone | '' {
  if (t === null || t === undefined) return '';
  const l = limit ?? 50;
  if (t > l) return 'bad';
  if (t >= l - 5) return 'warn';
  return 'ok';
}

/** A disk's place in a pool, in a few words: "tank · raidz2-0", "SnapRAID parity 2". */
export function roleLabel(r: DiskRole): string {
  switch (r.kind) {
    case 'zfs':
      if (r.role !== 'data') return `${r.pool_name} · ${r.role}`;
      return /^(raidz|draid|mirror)/.test(r.group) ? `${r.pool_name} · ${r.group}` : r.pool_name;
    case 'md':
      return r.role === 'data' ? r.pool_name : `${r.pool_name} · ${r.role}`;
    case 'snapraid':
      return `SnapRAID ${r.member}`;
    case 'lvmthin':
      return `LVM ${r.pool_name}`;
  }
}

/** Redundancy in words, with how worrying it is. A single disk without
 * redundancy is a choice; several striped ones are worth a warning. */
export function toleranceText(t: PoolTolerance | null | undefined, members = 2, rebuilding = false): { text: string; tone: Tone | '' } {
  if (!t) return { text: '', tone: '' };
  if (t.remaining < 0) return { text: 'More disks are down than it can survive', tone: 'bad' };
  if (t.designed === 0) {
    return members <= 1
      ? { text: 'Single disk, no redundancy', tone: '' }
      : { text: 'No redundancy: losing any disk loses the pool', tone: 'warn' };
  }
  if (t.failed > 0) {
    if (t.remaining === 0 && rebuilding) return { text: 'Rebuilding: no redundancy until it finishes', tone: 'warn' };
    return t.remaining === 0
      ? { text: `${plural(t.failed, 'disk')} down, no redundancy left`, tone: 'bad' }
      : { text: `${plural(t.failed, 'disk')} down, survives ${plural(t.remaining, 'more failure')}`, tone: 'warn' };
  }
  return { text: `Survives ${plural(t.designed, 'disk failure')}`, tone: 'ok' };
}

export const SCAN_LABEL: Record<string, string> = {
  scrub: 'Scrub',
  resilver: 'Resilver',
  expand: 'Expansion',
  recovery: 'Rebuild',
  resync: 'Resync',
  check: 'Check',
  reshape: 'Reshape',
  repair: 'Repair',
};

function share(p: number): string {
  return `${(p * 100).toFixed(p < 0.1 ? 1 : 0)}%`;
}

/** A scrub, resilver or rebuild in one line. */
export function scanText(s: PoolScan | null | undefined): string {
  if (!s) return '';
  const label = SCAN_LABEL[s.kind] ?? s.kind;
  switch (s.state) {
    case 'running':
      return [
        `${label} ${s.progress !== null ? share(s.progress) : 'running'}`,
        s.eta_secs ? `${duration(s.eta_secs)} left` : '',
        s.speed ? `at ${s.speed}` : '',
      ]
        .filter(Boolean)
        .join(' · ');
    case 'pending':
      return `${label} waiting to start`;
    case 'paused':
      return `${label} paused${s.progress !== null ? ` at ${share(s.progress)}` : ''}`;
    case 'canceled':
      return `${label} canceled${s.finished_at ? ` ${ago(s.finished_at)}` : ''}`;
    case 'finished': {
      const what = s.kind === 'resilver' ? 'resilvered' : 'repaired';
      const parts = [`${what} ${s.repaired ?? '0B'}`, plural(s.errors ?? 0, 'error')];
      if (s.duration_secs) parts.push(`took ${duration(s.duration_secs)}`);
      return `Last ${label.toLowerCase()} ${ago(s.finished_at)}: ${parts.join(', ')}`;
    }
    default:
      return s.kind === 'scrub' ? 'Never scrubbed' : '';
  }
}

export const TEST_KIND: Record<string, string> = {
  short: 'Short',
  long: 'Long',
  conveyance: 'Conveyance',
  offline: 'Offline scan',
  selective: 'Selective',
  vendor: 'Vendor',
};

export const TEST_STATUS: Record<string, { label: string; tone: Tone }> = {
  queued: { label: 'Waiting', tone: 'idle' },
  running: { label: 'Running', tone: 'info' },
  passed: { label: 'Passed', tone: 'ok' },
  failed: { label: 'Failed', tone: 'bad' },
  aborted: { label: 'Aborted', tone: 'warn' },
  interrupted: { label: "Didn't finish", tone: 'warn' },
  skipped: { label: 'Skipped', tone: 'idle' },
  error: { label: "Couldn't start", tone: 'warn' },
  unknown: { label: 'Unknown', tone: 'idle' },
};

/** Seconds a running test still needs: from its progress once there is
 * some, else from the drive's own estimate. */
export function testRemaining(t: Pick<DiskTest, 'status' | 'started_at' | 'progress' | 'expected_secs'>, now = Date.now() / 1000): number | null {
  if (t.status !== 'running' || !t.started_at) return null;
  const elapsed = Math.max(0, now - t.started_at);
  if (t.progress !== null && t.progress >= 0.1) return Math.max(0, elapsed / t.progress - elapsed);
  if (t.expected_secs) return Math.max(0, t.expected_secs - elapsed);
  return null;
}

/** "2 min", "8 h 16 min": the drive's estimate for a test. */
export function testLength(minutes: number | null | undefined): string {
  if (!minutes) return '';
  return duration(minutes * 60);
}

export const SCHEDULE_TARGETS: Record<TestSchedule['disks'], string> = {
  all: 'All disks',
  hdd: 'Hard disks',
  ssd: 'SSDs and NVMe',
  nvme: 'NVMe drives',
  list: 'Chosen disks',
};

function lowerFirst(s: string): string {
  return s ? s[0].toLowerCase() + s.slice(1) : s;
}

/** "Short test on all disks, every Sunday at 03:00". */
export function describeSchedule(s: Pick<TestSchedule, 'kind' | 'cron' | 'disks' | 'ids' | 'nodes'>): string {
  const when = describeCron(s.cron);
  const what = s.disks === 'list' ? plural(s.ids.length, 'chosen disk') : lowerFirst(SCHEDULE_TARGETS[s.disks]);
  const nodes = s.nodes.length ? ` on ${s.nodes.join(', ')}` : '';
  return `${TEST_KIND[s.kind] ?? s.kind} test on ${what}${nodes}, ${when ? lowerFirst(when) : `at "${s.cron}"`}`;
}

/** A weekly short test everywhere and a monthly long test on hard disks. */
export function recommendedSchedules(): TestSchedule[] {
  return [
    { id: 'weekly-short', name: 'Weekly short test', enabled: true, kind: 'short', cron: '0 3 * * 0', disks: 'all', ids: [], nodes: [] },
    { id: 'monthly-long', name: 'Monthly long test', enabled: true, kind: 'long', cron: '0 2 1 * *', disks: 'hdd', ids: [], nodes: [] },
  ];
}

function span(days: number): string {
  const d = Math.abs(days);
  if (d >= 335) return plural(Math.max(1, Math.round(d / 365)), 'year');
  if (d >= 60) return plural(Math.round(d / 30.4), 'month');
  return plural(d, 'day');
}

/** Warranty status from a "YYYY-MM-DD" end date. */
export function warranty(until: string, now = Date.now()): { text: string; tone: Tone } | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(until)) return null;
  const end = Date.parse(`${until}T00:00:00`);
  if (Number.isNaN(end)) return null;
  const days = Math.round((end - now) / 86_400_000);
  if (days < 0) return { text: `Warranty ended ${span(days)} ago`, tone: 'idle' };
  if (days <= 90) return { text: `Warranty ends in ${span(days)}`, tone: 'warn' };
  return { text: `Under warranty for ${span(days)}`, tone: 'ok' };
}

/** How old a disk is, from its purchase date (whole years, like an age). */
export function ageFrom(purchased: string, now = Date.now()): string {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(purchased)) return '';
  const start = Date.parse(`${purchased}T00:00:00`);
  if (Number.isNaN(start) || start > now) return '';
  const days = Math.round((now - start) / 86_400_000);
  if (days >= 365) return `bought ${plural(Math.floor(days / 365.25), 'year')} ago`;
  return `bought ${span(days)} ago`;
}

/** What the common ATA SMART attributes mean. */
export const ATA_ATTRIBUTES: Record<number, string> = {
  1: 'Read error rate. Vendor specific: on Seagate disks the raw value is a large composite counter that means nothing on its own.',
  3: 'Time the disk takes to spin up.',
  4: 'Start/stop count.',
  5: 'Reallocated sectors: bad sectors the disk replaced with spares. A small, stable number is tolerable; growth means trouble.',
  7: 'Seek error rate. Vendor specific: Seagate raw values are composite counters.',
  9: 'Power-on time.',
  10: 'Spin-up retries: the motor needed more than one attempt to reach speed.',
  11: 'Calibration retries.',
  12: 'Power cycles.',
  22: 'Helium level, 100 when full (helium-filled disks).',
  170: 'Reserved space left for bad blocks (SSD).',
  171: 'Program (write) failures (SSD).',
  172: 'Erase failures (SSD).',
  173: 'Wear leveling: average erase count (SSD).',
  174: 'Unexpected power losses.',
  177: 'Wear leveling: the normalized value is the remaining life (Samsung).',
  179: 'Reserved blocks used.',
  180: 'Reserved blocks unused.',
  181: 'Program failures.',
  182: 'Erase failures.',
  183: 'Runtime bad blocks, or link speed downshifts (vendor specific).',
  184: 'End-to-end errors: data corrupted between the disk\'s cache and its media.',
  187: 'Reported uncorrectable errors: reads that failed even after error correction.',
  188: 'Command timeouts, often cabling or power. Seagate packs three counters into the raw value.',
  189: 'High fly writes: the head flew too high while writing.',
  190: 'Airflow temperature (some disks store 100 minus the temperature as the value).',
  191: 'Shock and vibration events.',
  192: 'Emergency head retracts after power loss.',
  193: 'Head load/unload cycles (parking). Many disks are rated for 300,000 to 600,000.',
  194: 'Temperature.',
  195: 'Errors recovered by ECC (vendor specific).',
  196: 'Reallocation events.',
  197: 'Pending sectors: unreadable, waiting to be reallocated when they are written again.',
  198: 'Offline uncorrectable: sectors the disk\'s own surface scan couldn\'t read.',
  199: 'Interface CRC errors: corruption on the cable between disk and controller.',
  200: 'Write error rate.',
  202: 'Life remaining in percent (Crucial and Micron SSDs), vendor specific elsewhere.',
  220: 'Disk shift: the platters moved relative to the spindle, usually from a shock.',
  222: 'Hours with the heads loaded.',
  223: 'Load/unload retries.',
  225: 'Head load/unload cycles (some disks).',
  231: 'SSD life left (temperature on a few disks).',
  232: 'Reserved space left / endurance remaining (vendor specific).',
  233: 'Media wearout: the normalized value is the remaining life (Intel and others).',
  240: 'Hours the heads were flying.',
  241: 'Data written over the disk\'s life (units vary by vendor).',
  242: 'Data read over the disk\'s life (units vary by vendor).',
  250: 'Read error retries.',
  254: 'Free-fall protection events.',
};

/** Attributes that predict failure: shown prominently when not zero. */
export const CRITICAL_ATTRIBUTES = new Set([5, 10, 22, 184, 187, 188, 196, 197, 198, 199]);

/** How an attribute stands: failing now, failed in the past, close to its threshold, or fine. */
export function attributeTone(a: SmartAttribute): Tone | '' {
  if (a.failed === 'now') return 'bad';
  if (a.failed === 'past') return 'warn';
  if (a.thresh && a.value !== null && a.value - a.thresh <= 10) return 'warn';
  // Unreadable sectors: data is affected right now.
  if ((a.id === 197 || a.id === 198) && /^[1-9]/.test(a.raw.trim())) return 'bad';
  if (CRITICAL_ATTRIBUTES.has(a.id) && a.id !== 22 && a.id !== 188 && /^[1-9]/.test(a.raw.trim())) return 'warn';
  return '';
}

/** The disks' worst state first, then node and device. */
export function compareDisks(a: DiskView, b: DiskView): number {
  return verdictOf(b).rank - verdictOf(a).rank || a.node.localeCompare(b.node) || a.devpath.localeCompare(b.devpath, undefined, { numeric: true });
}

/** A short name for a disk: its bay label, else its device. */
export function shortName(d: Pick<DiskView, 'devpath' | 'label'>): string {
  return d.label.bay || d.devpath.replace('/dev/', '');
}

/** Bytes a day as "+12 GB/day" (decimal), "–" when unknown. */
export function growth(perDay: number | null | undefined): string {
  if (perDay === null || perDay === undefined) return '–';
  if (Math.abs(perDay) < 1e6) return 'no growth';
  return `${perDay > 0 ? '+' : '−'}${diskSize(Math.abs(perDay))}/day`;
}

/** "in about 8 months", for a pool filling up. */
export function fullIn(days: number | null | undefined): string {
  if (days === null || days === undefined || !Number.isFinite(days)) return '';
  if (days > 3650) return '';
  return `full in about ${span(Math.max(1, Math.round(days)))}`;
}
