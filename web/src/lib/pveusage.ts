// How a Proxmox node's CPU, memory, disk and network split across its
// guests, for the usage donuts on the Proxmox page. The few guests that use
// the most get their own slice (and keep it across all four charts); the
// rest share one "Other" slice. CPU and memory are set against the node's
// capacity, so they also show what the node uses itself and what is free.

import type { PveGuest, PveOverview } from './types';

export type UsageMetric = 'cpu' | 'mem' | 'disk' | 'net';
export const METRICS: UsageMetric[] = ['cpu', 'mem', 'disk', 'net'];

/** Guests with a slice of their own; everyone else goes into "Other". */
export const MAX_FEATURED = 5;

export interface NodeCapacity {
  node: string;
  online: boolean;
  /** Fraction of all cores in use (0..1). */
  cpu: number;
  cpus: number;
  mem: number;
  maxmem: number;
}

export type SliceKind = 'guest' | 'other' | 'host' | 'free';

export interface Slice {
  key: string;
  kind: SliceKind;
  label: string;
  value: number;
  /** Position among the featured guests, which picks the color. */
  slot?: number;
  vmid?: number;
}

export interface Breakdown {
  metric: UsageMetric;
  slices: Slice[];
  /** Sum of every slice: the node's capacity for CPU and memory, otherwise what the guests use. */
  total: number;
  /** What the guests use together. */
  guests: number;
  /** What the node reports in use (guests and host), for CPU and memory. */
  used: number;
  /** Guests folded into "Other" that use some of this. */
  others: number;
}

/** Every node, with the more detailed status of the node Nodestarr talks to. */
export function nodeCapacities(o: PveOverview): NodeCapacity[] {
  const n = o.node;
  const own: NodeCapacity = { node: n.node, online: true, cpu: n.cpu, cpus: n.cpus, mem: n.mem_used, maxmem: n.mem_total };
  const list = (o.nodes ?? []).map((x) => (x.node === n.node ? own : { ...x }));
  if (!list.some((x) => x.node === n.node)) list.unshift(own);
  return list.sort((a, b) => a.node.localeCompare(b.node));
}

/** What a guest uses: cores for CPU, bytes otherwise (network counts both directions since the guest started). */
export function guestValue(g: PveGuest, metric: UsageMetric): number {
  const on = g.status === 'running';
  switch (metric) {
    case 'cpu':
      return on ? Math.max(0, g.cpu * g.cpus) : 0;
    case 'mem':
      return on ? g.mem : 0;
    case 'disk':
      return g.disk;
    case 'net':
      return on ? g.netin + g.netout : 0;
  }
}

export function guestsOn(o: PveOverview, node: string): PveGuest[] {
  return o.guests.filter((g) => !g.template && g.node === node);
}

/**
 * The guests that get their own slice: the ones with the biggest share of
 * any one metric, so a guest that only stands out for CPU still shows. In
 * VMID order, so a guest's color doesn't change as the numbers move.
 */
export function featuredGuests(guests: PveGuest[], max = MAX_FEATURED): PveGuest[] {
  const totals = Object.fromEntries(METRICS.map((m) => [m, guests.reduce((s, g) => s + guestValue(g, m), 0)])) as Record<UsageMetric, number>;
  const score = (g: PveGuest) => Math.max(...METRICS.map((m) => (totals[m] > 0 ? guestValue(g, m) / totals[m] : 0)));
  return guests
    .map((g) => ({ g, s: score(g) }))
    .filter((x) => x.s > 0)
    .sort((a, b) => b.s - a.s || a.g.vmid - b.g.vmid)
    .slice(0, max)
    .map((x) => x.g)
    .sort((a, b) => a.vmid - b.vmid);
}

export function breakdown(metric: UsageMetric, cap: NodeCapacity, guests: PveGuest[], featured: PveGuest[]): Breakdown {
  const slices: Slice[] = [];
  let guestSum = 0;
  featured.forEach((g, slot) => {
    const value = guestValue(g, metric);
    guestSum += value;
    if (value > 0) slices.push({ key: `g${g.vmid}`, kind: 'guest', label: g.name || String(g.vmid), value, slot, vmid: g.vmid });
  });
  const ids = new Set(featured.map((g) => g.vmid));
  const rest = guests.filter((g) => !ids.has(g.vmid) && guestValue(g, metric) > 0);
  const other = rest.reduce((s, g) => s + guestValue(g, metric), 0);
  guestSum += other;
  if (other > 0) slices.push({ key: 'other', kind: 'other', label: 'Other', value: other });

  let used = guestSum;
  let total = guestSum;
  if (metric === 'cpu' || metric === 'mem') {
    const capacity = metric === 'cpu' ? cap.cpus : cap.maxmem;
    const nodeUsed = metric === 'cpu' ? cap.cpu * cap.cpus : cap.mem;
    // Guests can add up to a little more than the node reports (they're
    // sampled at different moments); the host's share is then just zero.
    used = Math.max(nodeUsed, guestSum);
    const host = used - guestSum;
    if (host > 0) slices.push({ key: 'host', kind: 'host', label: 'Host', value: host });
    const free = Math.max(0, capacity - used);
    if (free > 0) slices.push({ key: 'free', kind: 'free', label: 'Free', value: free });
    total = Math.max(capacity, used);
  }
  return { metric, slices, total, guests: guestSum, used, others: rest.length };
}
