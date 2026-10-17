import assert from 'node:assert/strict';
import { test } from 'node:test';

import {
  ageFrom,
  attributeTone,
  compareDisks,
  describeSchedule,
  diskSize,
  fullIn,
  growth,
  powerOn,
  recommendedSchedules,
  roleLabel,
  scanText,
  tempTone,
  testRemaining,
  toleranceText,
  verdictOf,
  warranty,
} from '../src/lib/disks.ts';
import type { DiskRole, DiskView, PoolScan } from '../src/lib/types.ts';

test('sizes are decimal, like on the label', () => {
  assert.equal(diskSize(4_000_787_030_016), '4 TB');
  assert.equal(diskSize(500_107_862_016), '500 GB');
  assert.equal(diskSize(1_920_383_410_176), '1.92 TB');
  assert.equal(diskSize(12_000_138_625_024), '12 TB');
  assert.equal(diskSize(null), '–');
});

test('power-on time reads naturally', () => {
  assert.equal(powerOn(41_234), '4.7 years');
  assert.equal(powerOn(2_900), '121 days');
  assert.equal(powerOn(36), '36 hours');
  assert.equal(powerOn(null), '–');
});

test('temperature tones follow the limit', () => {
  assert.equal(tempTone(34, 50), 'ok');
  assert.equal(tempTone(46, 50), 'warn');
  assert.equal(tempTone(51, 50), 'bad');
  assert.equal(tempTone(null, 50), '');
  assert.equal(tempTone(52, null), 'bad');
});

test('verdicts for gone and unread disks', () => {
  const d = { present: true, sampled_at: 1, error: null, verdict: 'warning' as const };
  assert.equal(verdictOf(d).label, 'Needs attention');
  assert.equal(verdictOf({ ...d, present: false }).label, 'Gone');
  assert.equal(verdictOf({ ...d, sampled_at: null }).label, 'Not read yet');
  assert.equal(verdictOf({ ...d, sampled_at: null, error: 'x' }).label, 'Unreadable');
});

test('pool roles in a few words', () => {
  const r = (o: Partial<DiskRole>): DiskRole => ({ pool: 'p', pool_name: 'tank', kind: 'zfs', role: 'data', group: 'raidz2-0', member: 'sda', state: null, ok: true, ...o });
  assert.equal(roleLabel(r({})), 'tank · raidz2-0');
  assert.equal(roleLabel(r({ group: 'sdb' })), 'tank');
  assert.equal(roleLabel(r({ role: 'log', group: 'mirror-1' })), 'tank · log');
  assert.equal(roleLabel(r({ kind: 'snapraid', pool_name: 'snapraid', role: 'parity', member: 'parity 2' })), 'SnapRAID parity 2');
  assert.equal(roleLabel(r({ kind: 'md', pool_name: 'md0', role: 'spare' })), 'md0 · spare');
  assert.equal(roleLabel(r({ kind: 'lvmthin', pool_name: 'pve/data' })), 'LVM pve/data');
});

test('redundancy in words', () => {
  assert.deepEqual(toleranceText({ designed: 2, failed: 0, remaining: 2 }), { text: 'Survives 2 disk failures', tone: 'ok' });
  assert.equal(toleranceText({ designed: 2, failed: 1, remaining: 1 }).text, '1 disk down, survives 1 more failure');
  assert.equal(toleranceText({ designed: 1, failed: 1, remaining: 0 }).tone, 'bad');
  assert.deepEqual(toleranceText({ designed: 1, failed: 1, remaining: 0 }, 2, true), { text: 'Rebuilding: no redundancy until it finishes', tone: 'warn' });
  assert.equal(toleranceText({ designed: 0, failed: 0, remaining: 0 }).tone, 'warn');
  assert.deepEqual(toleranceText({ designed: 0, failed: 0, remaining: 0 }, 1), { text: 'Single disk, no redundancy', tone: '' });
  assert.equal(toleranceText({ designed: 1, failed: 2, remaining: -1 }).text, 'More disks are down than it can survive');
  assert.equal(toleranceText(null).text, '');
});

test('scans in one line', () => {
  const s = (o: Partial<PoolScan>): PoolScan => ({
    kind: 'scrub',
    state: 'running',
    progress: 0.2815,
    eta_secs: 3 * 3600 + 23 * 60,
    speed: '223M/s',
    started_at: null,
    finished_at: null,
    duration_secs: null,
    repaired: null,
    errors: null,
    text: '',
    ...o,
  });
  assert.equal(scanText(s({})), 'Scrub 28% · 3h 23m left · at 223M/s');
  assert.equal(scanText(s({ progress: 0.05, eta_secs: null, speed: null })), 'Scrub 5.0%');
  assert.equal(scanText(s({ state: 'paused' })), 'Scrub paused at 28%');
  const finished = s({ state: 'finished', finished_at: Date.now() / 1000 - 5 * 86400, repaired: '0B', errors: 0, duration_secs: 3600 * 3 + 60 * 24 });
  assert.equal(scanText(finished), 'Last scrub 5 days ago: repaired 0B, 0 errors, took 3h 24m');
  assert.equal(scanText(s({ kind: 'resilver', state: 'finished', finished_at: Date.now() / 1000 - 10, repaired: '1.2T', errors: 1 })), 'Last resilver just now: resilvered 1.2T, 1 error');
  assert.equal(scanText(s({ state: 'none' })), 'Never scrubbed');
  assert.equal(scanText(null), '');
});

test('remaining test time', () => {
  const now = 10_000;
  const t = { status: 'running', started_at: now - 600, progress: 0.0, expected_secs: 120 * 60 };
  assert.equal(testRemaining(t, now), 6600);
  // With progress, the actual pace wins: 10 minutes for 25% → 30 more.
  assert.equal(testRemaining({ ...t, progress: 0.25 }, now), 1800);
  assert.equal(testRemaining({ ...t, status: 'passed' }, now), null);
  assert.equal(testRemaining({ ...t, expected_secs: null }, now), null);
});

test('schedules in words', () => {
  const [weekly, monthly] = recommendedSchedules();
  assert.equal(describeSchedule(weekly), 'Short test on all disks, every Sunday at 03:00');
  assert.equal(describeSchedule(monthly), 'Long test on hard disks, on day 1 of every month at 02:00');
  assert.equal(describeSchedule({ ...weekly, disks: 'list', ids: ['a', 'b'], nodes: ['pve2'] }), 'Short test on 2 chosen disks on pve2, every Sunday at 03:00');
  assert.equal(describeSchedule({ ...weekly, cron: '*/7 * * 2 *' }), 'Short test on all disks, at "*/7 * * 2 *"');
});

test('warranty and age', () => {
  const now = Date.parse('2026-10-16T12:00:00');
  assert.deepEqual(warranty('2027-10-16', now), { text: 'Under warranty for 1 year', tone: 'ok' });
  assert.equal(warranty('2026-11-15', now)?.tone, 'warn');
  assert.equal(warranty('2023-01-01', now)?.text, 'Warranty ended 4 years ago');
  assert.equal(warranty('soon', now), null);
  assert.equal(ageFrom('2021-03-14', now), 'bought 5 years ago');
  assert.equal(ageFrom('2026-01-01', now), 'bought 9 months ago');
  assert.equal(ageFrom('2030-01-01', now), '');
  // Calendar days are counted, so the hour, time zone and daylight saving
  // don't change the answer.
  assert.equal(ageFrom('2026-01-01', Date.parse('2026-10-16T23:59:00')), 'bought 9 months ago');
  assert.equal(ageFrom('2026-01-01', Date.parse('2026-10-16T00:01:00')), 'bought 9 months ago');
  assert.equal(warranty('2026-02-30', now), null);
});

test('attribute tones', () => {
  const a = { id: 5, name: 'Reallocated_Sector_Ct', value: 200, worst: 200, thresh: 140, raw: '0', prefail: true, failed: '' };
  assert.equal(attributeTone(a), '');
  assert.equal(attributeTone({ ...a, raw: '8' }), 'warn');
  assert.equal(attributeTone({ ...a, id: 197, thresh: 0, raw: '2' }), 'bad');
  assert.equal(attributeTone({ ...a, failed: 'now' }), 'bad');
  assert.equal(attributeTone({ ...a, value: 145 }), 'warn');
  // Seagate's composite command timeout counter isn't judged by its raw text.
  assert.equal(attributeTone({ ...a, id: 188, thresh: 0, raw: '4295032833' }), '');
});

test('pool growth and fill estimates', () => {
  assert.equal(growth(12e9), '+12 GB/day');
  assert.equal(growth(-3e9), '−3 GB/day');
  assert.equal(growth(10), 'no growth');
  assert.equal(growth(null), '–');
  assert.equal(fullIn(240), 'full in about 8 months');
  assert.equal(fullIn(20_000), '');
});

test('worst disks sort first', () => {
  const d = (id: string, verdict: DiskView['verdict'], devpath: string) => ({ id, verdict, devpath, node: 'pve', present: true, sampled_at: 1, error: null }) as DiskView;
  const list = [d('a', 'ok', '/dev/sdb'), d('b', 'failing', '/dev/sdc'), d('c', 'ok', '/dev/sda'), d('d', 'watch', '/dev/sdd')];
  assert.deepEqual(list.sort(compareDisks).map((x) => x.id), ['b', 'd', 'c', 'a']);
});
