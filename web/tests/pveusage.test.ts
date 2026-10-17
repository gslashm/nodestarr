import assert from 'node:assert/strict';
import { test } from 'node:test';

import { breakdown, featuredGuests, guestsOn, nodeCapacities } from '../src/lib/pveusage.ts';
import type { PveGuest, PveOverview } from '../src/lib/types.ts';

const GIB = 1024 ** 3;

function guest(vmid: number, over: Partial<PveGuest> = {}): PveGuest {
  return {
    vmid,
    name: `g${vmid}`,
    kind: 'lxc',
    node: 'pve',
    status: 'running',
    cpu: 0,
    cpus: 2,
    mem: 0,
    maxmem: 4 * GIB,
    disk: 0,
    maxdisk: 0,
    netin: 0,
    netout: 0,
    uptime: 100,
    tags: [],
    template: false,
    is_self: false,
    ...over,
  };
}

function overview(guests: PveGuest[], nodes: PveOverview['nodes'] = []): PveOverview {
  return {
    node: {
      node: 'pve',
      cpu: 0.5,
      cpus: 8,
      cpu_model: '',
      mem_used: 10 * GIB,
      mem_total: 16 * GIB,
      swap_used: 0,
      swap_total: 0,
      rootfs_used: 0,
      rootfs_total: 0,
      loadavg: [],
      uptime: 0,
      pve_version: '',
      kernel: '',
      ksm_shared: 0,
    },
    nodes,
    guests,
    storages: [],
    fetched_at: 0,
  };
}

test('capacities prefer the detailed status of our own node', () => {
  const o = overview([], [
    { node: 'pve2', online: true, cpu: 0.1, cpus: 4, mem: GIB, maxmem: 8 * GIB },
    { node: 'pve', online: true, cpu: 0.2, cpus: 8, mem: 9 * GIB, maxmem: 16 * GIB },
  ]);
  const caps = nodeCapacities(o);
  assert.deepEqual(caps.map((c) => c.node), ['pve', 'pve2']);
  assert.equal(caps[0].cpu, 0.5);
  assert.equal(caps[0].mem, 10 * GIB);
  // An older server without the node list still gets its own node.
  assert.deepEqual(nodeCapacities({ ...o, nodes: undefined as never }).map((c) => c.node), ['pve']);
});

test('guests on a node skip templates and other nodes', () => {
  const o = overview([guest(100), guest(101, { template: true }), guest(102, { node: 'pve2' })]);
  assert.deepEqual(guestsOn(o, 'pve').map((g) => g.vmid), [100]);
});

test('memory splits into guests, host and free', () => {
  const gs = [guest(100, { mem: 6 * GIB }), guest(101, { mem: 2 * GIB }), guest(102, { status: 'stopped', mem: 5 * GIB })];
  const cap = nodeCapacities(overview(gs))[0];
  const b = breakdown('mem', cap, gs, featuredGuests(gs));
  assert.deepEqual(
    b.slices.map((s) => [s.key, s.value / GIB]),
    [['g100', 6], ['g101', 2], ['host', 2], ['free', 6]],
  );
  assert.equal(b.total, 16 * GIB);
  assert.equal(b.used, 10 * GIB);
  assert.equal(b.guests, 8 * GIB);
});

test('cpu counts cores and never shows a negative host share', () => {
  // Guests add up to 5 cores while the node reports 4 (0.5 of 8).
  const gs = [guest(100, { cpu: 1, cpus: 3 }), guest(101, { cpu: 0.5, cpus: 4 })];
  const cap = nodeCapacities(overview(gs))[0];
  const b = breakdown('cpu', cap, gs, featuredGuests(gs));
  assert.deepEqual(b.slices.map((s) => [s.key, s.value]), [['g100', 3], ['g101', 2], ['free', 3]]);
  assert.equal(b.total, 8);
});

test('the busiest guests get slices, in VMID order, and the rest fold into Other', () => {
  const gs = [10, 20, 30, 40, 50, 60, 70].map((mb, i) => guest(100 + i, { mem: mb * 1024 ** 2 }));
  // 100 stands out for network only.
  gs[0].netin = 5 * GIB;
  const featured = featuredGuests(gs, 3);
  assert.deepEqual(featured.map((g) => g.vmid), [100, 105, 106]);
  const b = breakdown('mem', nodeCapacities(overview(gs))[0], gs, featured);
  const other = b.slices.find((s) => s.kind === 'other');
  assert.equal(other?.value, (20 + 30 + 40 + 50) * 1024 ** 2);
  assert.equal(b.others, 4);
  // Slots follow the featured order, so a guest's color is the same in every chart.
  assert.deepEqual(b.slices.filter((s) => s.kind === 'guest').map((s) => s.slot), [0, 1, 2]);
});

test('disk and network are shares of what the guests use', () => {
  const gs = [guest(100, { disk: 3 * GIB, netin: 10, netout: 5 }), guest(101, { kind: 'qemu', status: 'stopped', disk: GIB, netin: 99 })];
  const cap = nodeCapacities(overview(gs))[0];
  const disk = breakdown('disk', cap, gs, featuredGuests(gs));
  assert.equal(disk.total, 4 * GIB);
  assert.ok(disk.slices.every((s) => s.kind === 'guest'));
  const net = breakdown('net', cap, gs, featuredGuests(gs));
  assert.deepEqual(net.slices.map((s) => [s.key, s.value]), [['g100', 15]]);
});
