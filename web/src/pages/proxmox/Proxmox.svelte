<!--
  Proxmox VE: the node Berth runs on and its guests (with power actions,
  snapshots and vzdump backups), backup archives, node tasks, and the
  node's disks, package updates and history.
-->
<script lang="ts">
  import type { Component } from 'svelte';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Server from '@lucide/svelte/icons/server';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Button from '../../components/Button.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import PageHeader from '../../components/PageHeader.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import Tabs from '../../components/Tabs.svelte';
  import { api } from '../../lib/api';
  import { ago } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { PveOverview } from '../../lib/types';

  let { params }: { params: Record<string, string> } = $props();

  interface Status {
    enabled: boolean;
    overview?: PveOverview | null;
    self_vmid?: number | null;
    error?: string | null;
  }

  type TabProps = { overview: PveOverview; selfVmid: number | null; reload: () => Promise<void> };
  const loaders: Record<string, () => Promise<{ default: Component<TabProps> }>> = {
    guests: () => import('./Guests.svelte'),
    backups: () => import('./PveBackups.svelte'),
    tasks: () => import('./PveTasks.svelte'),
    host: () => import('./Host.svelte'),
  };

  const tab = $derived(params.tab && loaders[params.tab] ? params.tab : 'guests');
  let status = $state<Status | null>(null);
  let loading = $state(false);
  let Tab = $state<Component<TabProps> | null>(null);

  async function load(refresh = false) {
    loading = true;
    try {
      status = await api.get<Status>(`/proxmox/status${refresh ? '?refresh=1' : ''}`);
    } catch (err) {
      status = { enabled: true, overview: status?.overview ?? null, error: String(err) };
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void load();
    // Keep the numbers fresh while the page is open (the server caches for 20 s).
    const t = setInterval(() => {
      if (!document.hidden) void load();
    }, 20_000);
    return () => clearInterval(t);
  });

  $effect(() => {
    const key = tab;
    loaders[key]().then((m) => {
      if (key === tab) Tab = m.default;
    });
  });

  const o = $derived(status?.overview ?? null);
  const tabList = [
    { key: 'guests', label: 'Guests', path: '/proxmox' },
    { key: 'backups', label: 'Backups', path: '/proxmox/backups' },
    { key: 'tasks', label: 'Tasks', path: '/proxmox/tasks' },
    { key: 'host', label: 'Node', path: '/proxmox/host' },
  ];
</script>

<PageHeader
  title={o ? `Proxmox: ${o.node.node}` : 'Proxmox'}
  subtitle={o ? `Proxmox VE ${o.node.pve_version}, updated ${ago(o.fetched_at)}` : ''}
>
  {#snippet actions()}
    {#if status?.enabled}
      <Button variant="ghost" icon={RefreshCw} busy={loading} onclick={() => load(true)}>Refresh</Button>
    {/if}
  {/snippet}
  {#snippet tabs()}
    {#if status?.enabled}<Tabs tabs={tabList} active={tab} />{/if}
  {/snippet}
</PageHeader>

<div class="page-body">
  {#if !status}
    <p class="faint"><Spinner size={13} /> Connecting to Proxmox…</p>
  {:else if !status.enabled}
    <EmptyState icon={Server} title="Proxmox isn't connected">
      Connect Berth to your Proxmox VE node to see its guests, take snapshots and vzdump backups, and watch the host.
      {#snippet actions()}
        {#if session.isAdmin}<a class="btn primary" href={href('/settings/proxmox')}>Set up Proxmox</a>{/if}
      {/snippet}
    </EmptyState>
  {:else}
    {#if status.error}
      <div class="banner">
        <TriangleAlert size={18} />
        <span>Couldn't reach Proxmox: {status.error}{o ? ' Showing the last data Berth received.' : ''} <a href={href('/settings/proxmox')}>Check the connection</a></span>
      </div>
    {/if}
    {#if o && Tab}
      {#key tab}
        <Tab overview={o} selfVmid={status.self_vmid ?? null} reload={() => load(true)} />
      {/key}
    {/if}
  {/if}
</div>

<style>
  .banner {
    display: flex;
    gap: 10px;
    margin-bottom: 16px;
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--bad) 40%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
  }
  .banner :global(svg) {
    flex: none;
    color: var(--bad);
  }
</style>
