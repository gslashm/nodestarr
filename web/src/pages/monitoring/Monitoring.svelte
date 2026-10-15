<!-- Monitoring: host and container resources, uptime checks and alerts. -->
<script lang="ts">
  import type { Component } from 'svelte';
  import Settings from '@lucide/svelte/icons/settings';
  import PageHeader from '../../components/PageHeader.svelte';
  import Tabs from '../../components/Tabs.svelte';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';

  let { params }: { params: Record<string, string> } = $props();

  const loaders: Record<string, () => Promise<{ default: Component }>> = {
    resources: () => import('./Resources.svelte'),
    uptime: () => import('./Uptime.svelte'),
    alerts: () => import('./Alerts.svelte'),
  };

  const tab = $derived(params.tab && loaders[params.tab] ? params.tab : 'resources');
  let Tab = $state<Component | null>(null);

  $effect(() => {
    const key = tab;
    loaders[key]().then((m) => {
      if (key === tab) Tab = m.default;
    });
  });

  const down = $derived(Object.values(live.uptime).filter((u) => u.state === 'down').length);

  const tabList = $derived([
    { key: 'resources', label: 'Resources', path: '/monitoring' },
    { key: 'uptime', label: 'Uptime', path: '/monitoring/uptime', badge: down || null, tone: 'bad' as const },
    { key: 'alerts', label: 'Alerts', path: '/monitoring/alerts', badge: live.alerts.length || null, tone: 'warn' as const },
  ]);
</script>

<PageHeader title="Monitoring">
  {#snippet actions()}
    {#if session.isAdmin}
      <a class="btn ghost" href={href('/settings/monitoring')}><Settings size={17} />Settings</a>
    {/if}
  {/snippet}
  {#snippet tabs()}
    <Tabs tabs={tabList} active={tab} />
  {/snippet}
</PageHeader>

<div class="page-body">
  {#if Tab}
    {#key tab}<Tab />{/key}
  {/if}
</div>
