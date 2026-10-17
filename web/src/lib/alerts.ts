// What each kind of alert rule watches, in words, and which of its fields
// (threshold, duration) mean something. Shared by Monitoring and Settings.

import { duration } from './format';
import type { AlertRule } from './types';

export interface AlertKindInfo {
  label: string;
  /** Sentence describing when the rule fires. */
  describe: (r: Pick<AlertRule, 'threshold' | 'duration_secs'>) => string;
  /** Label and unit of the threshold, when the rule uses one. */
  threshold?: { label: string; unit: string; min: number; max?: number; step?: number };
  /** Whether the rule waits for the condition to last a while. */
  usesDuration: boolean;
}

function lasting(secs: number): string {
  return secs > 0 ? ` for ${duration(secs)}` : '';
}

export const ALERT_KINDS: Record<string, AlertKindInfo> = {
  host_memory: {
    label: 'Host memory',
    describe: (r) => `Available memory on the Docker host drops below ${r.threshold}%${lasting(r.duration_secs)}`,
    threshold: { label: 'Available below', unit: '%', min: 1, max: 99 },
    usesDuration: true,
  },
  proxmox_memory: {
    label: 'Proxmox memory',
    describe: (r) => `The Proxmox node uses more than ${r.threshold}% of its memory${lasting(r.duration_secs)}`,
    threshold: { label: 'Used above', unit: '%', min: 1, max: 100 },
    usesDuration: true,
  },
  proxmox_storage: {
    label: 'Proxmox storage',
    describe: (r) => `A Proxmox storage is more than ${r.threshold}% full`,
    threshold: { label: 'Full above', unit: '%', min: 1, max: 100 },
    usesDuration: false,
  },
  container_memory: {
    label: 'Container memory',
    describe: (r) => `A container uses more than ${r.threshold}% of its memory limit${lasting(r.duration_secs)}`,
    threshold: { label: 'Above', unit: '% of limit', min: 1, max: 100 },
    usesDuration: true,
  },
  container_cpu: {
    label: 'Container CPU',
    describe: (r) => `A container uses more than ${r.threshold}% CPU${lasting(r.duration_secs)}`,
    threshold: { label: 'Above', unit: '% CPU', min: 1, step: 5 },
    usesDuration: true,
  },
  restart_loop: {
    label: 'Restart loop',
    describe: (r) => `A container restarts ${r.threshold} times within ${duration(r.duration_secs)}`,
    threshold: { label: 'Restarts', unit: 'times', min: 2 },
    usesDuration: true,
  },
  unhealthy: {
    label: 'Health check',
    describe: (r) => `A container's health check fails${lasting(r.duration_secs)}`,
    usesDuration: true,
  },
  unexpected_exit: {
    label: 'Crash',
    describe: () => 'A container exits with an error without being stopped',
    usesDuration: false,
  },
  oom: {
    label: 'Out of memory',
    describe: () => 'A container is killed for using too much memory',
    usesDuration: false,
  },
  disk: {
    label: 'Disk usage',
    describe: (r) => `A watched disk is more than ${r.threshold}% full`,
    threshold: { label: 'Full above', unit: '%', min: 1, max: 100 },
    usesDuration: false,
  },
  backup_age: {
    label: 'Backup age',
    describe: (r) => `A container with backups on has no backup newer than ${r.threshold} hours`,
    threshold: { label: 'Older than', unit: 'hours', min: 1 },
    usesDuration: false,
  },
  disk_health: {
    label: 'Disk health',
    describe: () => 'A Proxmox disk reports a problem: failing SMART health, unreadable sectors, a failed self-test or worn-out flash',
    usesDuration: false,
  },
  disk_temperature: {
    label: 'Disk temperature',
    describe: (r) => `A hard disk is hotter than ${r.threshold} °C${lasting(r.duration_secs)} (SSDs: their own limit, or ${r.threshold + 20} °C)`,
    threshold: { label: 'Hotter than', unit: '°C', min: 30, max: 90 },
    usesDuration: true,
  },
  pool_health: {
    label: 'Pool health',
    describe: () => 'A ZFS pool, mdadm or SnapRAID array is degraded or faulted, or has data errors',
    usesDuration: false,
  },
  pool_scrub_age: {
    label: 'Pool scrub age',
    describe: (r) => `A ZFS pool or SnapRAID array wasn't scrubbed for ${r.threshold} days`,
    threshold: { label: 'Older than', unit: 'days', min: 1 },
    usesDuration: false,
  },
};

export function alertKind(kind: string): AlertKindInfo {
  return ALERT_KINDS[kind] ?? { label: kind, describe: () => kind, usesDuration: false };
}
