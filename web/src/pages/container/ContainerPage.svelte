<!--
  One container: header with its state and main actions, and tabs for the
  overview, logs, console, resources, updates, backups, files and settings.
-->
<script lang="ts">
  import type { Component } from 'svelte';
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import PackageX from '@lucide/svelte/icons/package-x';
  import Pause from '@lucide/svelte/icons/pause';
  import Play from '@lucide/svelte/icons/play';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Square from '@lucide/svelte/icons/square';
  import AppIcon from '../../components/AppIcon.svelte';
  import Button from '../../components/Button.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Menu from '../../components/Menu.svelte';
  import PageHeader from '../../components/PageHeader.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import StatePill from '../../components/StatePill.svelte';
  import Tabs from '../../components/Tabs.svelte';
  import { backupNow, confirmUpdate, containerMenu, runAction, webUrl } from '../../lib/actions';
  import { api, seg } from '../../lib/api';
  import { clock } from '../../lib/clock.svelte';
  import { duration, hasUpdate, isUp, stateOf, updateLabel } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { ContainerDetail } from '../../lib/types';

  let { params }: { params: Record<string, string> } = $props();

  const tabLoaders: Record<string, () => Promise<{ default: Component<{ c: NonNullable<typeof c>; detail: ContainerDetail | null; reload: () => void }> }>> = {
    overview: () => import('./Overview.svelte'),
    logs: () => import('./Logs.svelte'),
    console: () => import('./Console.svelte'),
    stats: () => import('./Stats.svelte'),
    updates: () => import('./UpdatesTab.svelte'),
    backups: () => import('./BackupsTab.svelte'),
    files: () => import('./Files.svelte'),
    settings: () => import('./Prefs.svelte'),
    edit: () => import('./Edit.svelte'),
  };

  const c = $derived(live.find(params.id));
  const tab = $derived(params.tab && tabLoaders[params.tab] ? params.tab : 'overview');
  let detail = $state<ContainerDetail | null>(null);
  let detailFor = '';
  let Tab = $state<Component<{ c: NonNullable<typeof c>; detail: ContainerDetail | null; reload: () => void }> | null>(null);

  async function loadDetail() {
    if (!c) return;
    const id = c.id;
    try {
      const d = await api.get<ContainerDetail>(`/containers/${seg(id)}`);
      if (c?.id === id) detail = d;
    } catch {
      // The header still works from live data; tabs show their own errors.
    }
  }

  // Reload details when the container is recreated (new id) or first seen.
  $effect(() => {
    const id = c?.id;
    if (id && id !== detailFor) {
      detailFor = id;
      detail = null;
      void loadDetail();
    }
  });

  $effect(() => {
    const key = tab;
    tabLoaders[key]().then((m) => {
      if (key === tab) Tab = m.default;
    });
  });

  const st = $derived(c ? stateOf(c) : 'stopped');
  const up = $derived(isUp(st));
  const web = $derived(c ? webUrl(c) : null);
  const base = $derived(`/containers/${encodeURIComponent(params.id)}`);
  const issues = $derived(c?.app_status?.health.filter((h) => h.level !== 'notice').length ?? 0);

  const tabList = $derived([
    { key: 'overview', label: 'Overview', path: base, badge: issues || null, tone: 'warn' as const },
    { key: 'logs', label: 'Logs', path: `${base}/logs` },
    ...(session.isAdmin ? [{ key: 'console', label: 'Console', path: `${base}/console` }] : []),
    { key: 'stats', label: 'Resources', path: `${base}/stats` },
    { key: 'updates', label: 'Updates', path: `${base}/updates`, badge: c && hasUpdate(c.update) ? '1' : null, tone: 'update' as const },
    { key: 'backups', label: 'Backups', path: `${base}/backups` },
    ...(session.isAdmin ? [{ key: 'files', label: 'Files', path: `${base}/files` }] : []),
    { key: 'settings', label: 'Settings', path: `${base}/settings` },
    ...(session.isAdmin && (detail?.can_edit_spec || (c?.stack && c.service && !c.is_self)) ? [{ key: 'edit', label: 'Edit', path: `${base}/edit` }] : []),
  ]);

  const subtitle = $derived.by(() => {
    if (!c) return '';
    void clock.tick;
    const parts: string[] = [];
    if (c.app_name && c.app_name !== c.display_name) parts.push(c.app_name);
    if (c.version) parts.push(c.version);
    if (up && c.started_at) parts.push(`up ${duration(clock.now - c.started_at)}`);
    else if (c.finished_at) parts.push(`stopped ${duration(clock.now - c.finished_at)} ago`);
    return parts.join(', ');
  });
</script>

{#if !live.loaded}
  <div class="loading"><Spinner /> Loading…</div>
{:else if !c}
  <PageHeader title={params.id} back={{ label: 'Containers', path: '/containers' }} />
  <EmptyState icon={PackageX} title="This container doesn't exist any more">
    It may have been removed or renamed.
    {#snippet actions()}<a class="btn" href={href('/containers')}>All containers</a>{/snippet}
  </EmptyState>
{:else}
  <PageHeader title={c.display_name} {subtitle} back={{ label: 'Containers', path: '/containers' }}>
    {#snippet lead()}
      <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={38} />
    {/snippet}
    {#snippet actions()}
      <StatePill state={st} />
      {#if c.stack}<a class="chip stack-chip" href={href(`/stacks/${encodeURIComponent(c.stack)}`)}>{c.stack}</a>{/if}
      {#if web}<a class="btn ghost" href={web} target="_blank" rel="noopener"><ExternalLink size={16} />Open</a>{/if}
      {#if session.isAdmin}
        {#if st === 'paused'}
          <Button variant="ghost" icon={Play} onclick={() => runAction(c, 'unpause')}>Resume</Button>
        {:else if up || st === 'restarting'}
          <Button variant="ghost" icon={RotateCw} busy={c.busy} onclick={() => runAction(c, 'restart')}>Restart</Button>
          <Button variant="ghost" icon={Square} onclick={() => runAction(c, 'stop')} disabled={c.is_self}>Stop</Button>
          <Button variant="ghost" icon={Pause} label="Pause" onclick={() => runAction(c, 'pause')} disabled={c.is_self} />
        {:else}
          <Button variant="primary" icon={Play} busy={c.busy} onclick={() => runAction(c, 'start')}>Start</Button>
        {/if}
        {#if hasUpdate(c.update) && c.policy !== 'ignore'}
          <Button variant="primary" icon={ArrowUp} onclick={() => confirmUpdate(c)} title="Update to {updateLabel(c.update)}">Update</Button>
        {/if}
        <Button variant="ghost" icon={ArchiveIcon} label="Back up now" onclick={() => backupNow(c)} />
      {/if}
      <Menu items={containerMenu(c)} label="More actions" />
    {/snippet}
    {#snippet tabs()}
      <Tabs tabs={tabList} active={tab} />
    {/snippet}
  </PageHeader>

  <div class="tab-body" class:wide={tab === 'logs' || tab === 'console' || tab === 'files'}>
    {#if Tab}
      {#key `${tab}:${c.id}`}
        <Tab {c} {detail} reload={loadDetail} />
      {/key}
    {/if}
  </div>
{/if}

<style>
  .loading {
    display: flex;
    gap: 10px;
    align-items: center;
    padding: 40px 24px;
    color: var(--text-2);
  }
  .stack-chip {
    text-decoration: none;
  }
  .stack-chip:hover {
    color: var(--text);
    border-color: var(--line-strong);
  }
  .tab-body {
    padding: 18px 24px 40px;
  }
  .tab-body.wide {
    padding-bottom: 18px;
  }
  @media (max-width: 760px) {
    .tab-body {
      padding: 14px 12px 32px;
    }
  }
</style>
