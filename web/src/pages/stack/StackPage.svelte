<!--
  One compose stack: its services and their state, the compose editor,
  combined logs and file history.
-->
<script lang="ts">
  import type { Component } from 'svelte';
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import Download from '@lucide/svelte/icons/download';
  import Layers from '@lucide/svelte/icons/layers';
  import Play from '@lucide/svelte/icons/play';
  import PowerOff from '@lucide/svelte/icons/power-off';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Square from '@lucide/svelte/icons/square';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Button from '../../components/Button.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import LogStream from '../../components/LogStream.svelte';
  import Menu, { type MenuEntry } from '../../components/Menu.svelte';
  import PageHeader from '../../components/PageHeader.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import Tabs from '../../components/Tabs.svelte';
  import { stackAction } from '../../lib/actions';
  import { ApiError, api, errorMessage, seg } from '../../lib/api';
  import { hasUpdate, isUp, plural, stateOf } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href, router } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { StackDetail } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { params }: { params: Record<string, string> } = $props();

  type TabProps = { name: string; detail: StackDetail; reload: () => Promise<void> };
  const tabLoaders: Record<string, () => Promise<{ default: Component<TabProps> }>> = {
    overview: () => import('./StackOverview.svelte'),
    compose: () => import('./StackCompose.svelte'),
    history: () => import('./StackHistory.svelte'),
  };

  const name = $derived(params.name);
  const tab = $derived(params.tab === 'logs' || (params.tab && tabLoaders[params.tab]) ? params.tab : 'overview');
  let detail = $state<StackDetail | null>(null);
  let error = $state('');
  let missing = $state(false);
  let Tab = $state<Component<TabProps> | null>(null);

  async function load() {
    const n = name;
    try {
      const d = await api.get<StackDetail>(`/stacks/${seg(n)}`);
      if (n !== name) return;
      detail = d;
      error = '';
      missing = false;
    } catch (err) {
      if (n !== name) return;
      if (err instanceof ApiError && err.status === 404) missing = true;
      else error = errorMessage(err);
    }
  }

  $effect(() => {
    void name;
    void live.stacksVersion;
    void load();
  });

  $effect(() => {
    const key = tab;
    if (key === 'logs') return;
    tabLoaders[key]().then((m) => {
      if (key === tab) Tab = m.default;
    });
  });

  const members = $derived(
    (detail?.stack.containers ?? []).map((c) => live.byIdentity.get(c.identity)).filter((c): c is NonNullable<typeof c> => !!c),
  );
  const running = $derived(members.filter((c) => isUp(stateOf(c))).length);
  const updates = $derived(members.filter((c) => hasUpdate(c.update)).length);
  const status = $derived(!detail ? '' : members.length === 0 ? 'Not deployed' : running === 0 ? 'Stopped' : running < members.length ? 'Partly running' : 'Running');
  const base = $derived(`/stacks/${encodeURIComponent(name)}`);
  const serviceNames = $derived((detail?.services ?? []).map((s) => s.name).filter((s): s is string => !!s));

  const tabList = $derived([
    { key: 'overview', label: 'Services', path: base, badge: detail?.config_error ? '!' : null, tone: 'bad' as const },
    ...(session.isAdmin && detail?.stack.editable ? [{ key: 'compose', label: 'Compose', path: `${base}/compose` }] : []),
    ...(members.length ? [{ key: 'logs', label: 'Logs', path: `${base}/logs` }] : []),
    ...(session.isAdmin && detail?.stack.editable ? [{ key: 'history', label: 'History', path: `${base}/history` }] : []),
  ]);

  async function backup() {
    try {
      const r = await api.post<{ task_id: string }>(`/stacks/${seg(name)}/backup`);
      ui.taskStarted(r.task_id, `Backing up the ${name} stack`);
    } catch (err) {
      ui.error(err, "Couldn't start the backup");
    }
  }

  async function remove() {
    const r = await ui.ask({
      title: `Remove the ${name} stack?`,
      message: 'Its containers are stopped and removed.',
      confirmLabel: 'Remove stack',
      danger: true,
      options: [
        { key: 'files', label: `Also move its folder (${detail?.stack.dir ?? ''}) to the stacks trash` },
        { key: 'volumes', label: 'Delete its named volumes (their data is lost)', danger: true },
      ],
      typeToConfirm: name,
    });
    if (!r.ok) return;
    try {
      const res = await api.del<{ task_id: string }>(`/stacks/${seg(name)}?remove_files=${r.options.files ? 1 : 0}&remove_volumes=${r.options.volumes ? 1 : 0}`);
      ui.taskStarted(res.task_id, `Removing the ${name} stack`);
      router.navigate('/stacks');
    } catch (err) {
      ui.error(err, "Couldn't remove it");
    }
  }

  const menuItems = $derived.by((): MenuEntry[] => {
    if (!session.isAdmin) return [];
    return [
      { label: 'Pull images only', icon: Download, run: () => stackAction(name, 'pull') },
      { label: 'Back up', icon: ArchiveIcon, run: backup, disabled: members.length === 0 },
      'separator',
      { label: 'Stop', icon: Square, run: () => stackAction(name, 'stop'), disabled: running === 0 },
      { label: 'Take down', icon: PowerOff, danger: true, run: () => stackAction(name, 'down'), disabled: members.length === 0 },
      { label: 'Remove stack', icon: Trash2, danger: true, run: remove, disabled: members.some((c) => c.is_self) },
    ];
  });
</script>

{#if missing}
  <PageHeader title={name} back={{ label: 'Stacks', path: '/stacks' }} />
  <EmptyState icon={Layers} title="This stack doesn't exist">
    It may have been removed, or its folder moved.
    {#snippet actions()}<a class="btn" href={href('/stacks')}>All stacks</a>{/snippet}
  </EmptyState>
{:else}
  <PageHeader title={name} subtitle={detail ? [status, members.length ? `${running} of ${plural(members.length, 'container')} running` : '', detail.stack.dir ?? ''].filter(Boolean).join(', ') : ''} back={{ label: 'Stacks', path: '/stacks' }}>
    {#snippet lead()}
      <span class="lead"><Layers size={20} /></span>
    {/snippet}
    {#snippet actions()}
      {#if session.isAdmin && detail}
        {#if updates}
          <Button variant="primary" icon={ArrowUp} onclick={() => stackAction(name, 'update')}>Update {plural(updates, 'service')}</Button>
        {/if}
        {#if members.length === 0 || running < members.length}
          <Button variant={updates ? '' : 'primary'} icon={Play} onclick={() => stackAction(name, 'up')} disabled={!detail.stack.editable && members.length === 0}>Deploy</Button>
        {/if}
        {#if running > 0}
          <Button variant="ghost" icon={RotateCw} onclick={() => stackAction(name, 'restart')}>Restart</Button>
        {/if}
        <Menu items={menuItems} label="More stack actions" />
      {/if}
    {/snippet}
    {#snippet tabs()}
      <Tabs tabs={tabList} active={tab} />
    {/snippet}
  </PageHeader>

  <div class="page-body">
    {#if error}
      <p class="text-bad">{error}</p>
    {:else if !detail}
      <p class="faint"><Spinner size={13} /> Loading…</p>
    {:else if tab === 'logs'}
      {#key name}
        <LogStream path="/stacks/{seg(name)}/logs/stream" services={serviceNames} running={running > 0} />
      {/key}
    {:else if Tab}
      {#key `${tab}:${name}`}
        <Tab {name} {detail} reload={load} />
      {/key}
    {/if}
  </div>
{/if}

<style>
  .lead {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border-radius: 9px;
    background: var(--raised);
    color: var(--accent-text);
  }
</style>
