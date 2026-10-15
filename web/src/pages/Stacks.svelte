<!--
  Compose stacks: every project Docker knows about plus the folders in the
  stacks directories that were never deployed. States come from the live
  container feed, so they stay current without polling.
-->
<script lang="ts">
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import Download from '@lucide/svelte/icons/download';
  import FileCode from '@lucide/svelte/icons/file-code';
  import Layers from '@lucide/svelte/icons/layers';
  import Play from '@lucide/svelte/icons/play';
  import Plus from '@lucide/svelte/icons/plus';
  import PowerOff from '@lucide/svelte/icons/power-off';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import ScrollText from '@lucide/svelte/icons/scroll-text';
  import Square from '@lucide/svelte/icons/square';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import Button from '../components/Button.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import Menu, { type MenuEntry } from '../components/Menu.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import Spinner from '../components/Spinner.svelte';
  import StateDot from '../components/StateDot.svelte';
  import { stackAction } from '../lib/actions';
  import { api, errorMessage, seg } from '../lib/api';
  import { hasUpdate, isUp, plural, stateOf, type StateKey } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { StackSummary } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params: _params }: { params: Record<string, string> } = $props();

  let stacks = $state<StackSummary[] | null>(null);
  let composeAvailable = $state(true);
  let composeVersion = $state<string | null>(null);
  let error = $state('');
  let query = $state('');

  async function load() {
    try {
      const r = await api.get<{ stacks: StackSummary[]; compose_available: boolean; compose_version: string | null }>('/stacks');
      stacks = r.stacks;
      composeAvailable = r.compose_available;
      composeVersion = r.compose_version;
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  $effect(() => {
    void live.stacksVersion;
    void load();
  });

  interface Member {
    service: string;
    name: string;
    state: StateKey;
    update: boolean;
  }

  interface Row {
    stack: StackSummary;
    members: Member[];
    running: number;
    updates: number;
    status: 'running' | 'partial' | 'stopped' | 'inactive';
  }

  const rows = $derived.by((): Row[] => {
    if (!stacks) return [];
    const q = query.trim().toLowerCase();
    const order = { running: 0, partial: 0, stopped: 1, inactive: 2 } as const;
    return stacks
      .filter((s) => !q || s.name.toLowerCase().includes(q) || s.containers.some((c) => (c.service ?? c.name).toLowerCase().includes(q)))
      .map((s) => {
        const members: Member[] = s.containers.map((c) => {
          const v = live.byIdentity.get(c.identity);
          return {
            service: c.service ?? c.name,
            name: v?.name ?? c.name,
            state: v ? stateOf(v) : stateOf({ state: c.state, health: c.health, exit_code: null, oom_killed: false }),
            update: !!v && hasUpdate(v.update),
          };
        });
        const running = members.filter((m) => isUp(m.state)).length;
        const status: Row['status'] = members.length === 0 ? 'inactive' : running === 0 ? 'stopped' : running < members.length ? 'partial' : 'running';
        return { stack: s, members, running, updates: members.filter((m) => m.update).length, status };
      })
      .sort((a, b) => order[a.status] - order[b.status] || a.stack.name.localeCompare(b.stack.name));
  });

  const STATUS_TEXT = { running: 'Running', partial: 'Partly running', stopped: 'Stopped', inactive: 'Not deployed' } as const;

  function menu(r: Row): MenuEntry[] {
    const name = r.stack.name;
    const path = `/stacks/${encodeURIComponent(name)}`;
    const items: MenuEntry[] = [
      { label: 'Edit compose file', icon: FileCode, href: href(`${path}/compose`), disabled: !r.stack.editable },
      { label: 'Logs', icon: ScrollText, href: href(`${path}/logs`), disabled: r.members.length === 0 },
    ];
    if (!session.isAdmin) return items;
    items.push(
      'separator',
      { label: 'Pull and update', icon: ArrowUp, run: () => stackAction(name, 'update') },
      { label: 'Pull images only', icon: Download, run: () => stackAction(name, 'pull') },
      { label: 'Back up', icon: ArchiveIcon, run: () => backup(name), disabled: r.members.length === 0 },
      'separator',
      { label: 'Stop', icon: Square, run: () => stackAction(name, 'stop'), disabled: r.running === 0 },
      { label: 'Take down', icon: PowerOff, danger: true, run: () => stackAction(name, 'down'), disabled: r.members.length === 0 },
    );
    return items;
  }

  async function backup(name: string) {
    try {
      const r = await api.post<{ task_id: string }>(`/stacks/${seg(name)}/backup`);
      ui.taskStarted(r.task_id, `Backing up the ${name} stack`);
    } catch (err) {
      ui.error(err, "Couldn't start the backup");
    }
  }

  const totals = $derived({
    stacks: rows.length,
    running: rows.filter((r) => r.status === 'running' || r.status === 'partial').length,
  });
</script>

<PageHeader title="Stacks" subtitle={stacks ? `${plural(totals.stacks, 'stack')}, ${totals.running} running` : ''}>
  {#snippet actions()}
    {#if session.isAdmin}
      <a class="btn primary" href={href('/stacks/new')}><Plus size={17} />Create stack</a>
    {/if}
  {/snippet}
</PageHeader>

<div class="page-body">
  {#if !composeAvailable}
    <div class="banner">
      <TriangleAlert size={18} />
      <span>Docker Compose isn't available, so stacks can't be deployed or changed. Install the compose plugin, or set the compose command in <a href={href('/settings/docker')}>Settings</a>.</span>
    </div>
  {/if}

  {#if error}
    <p class="text-bad">{error}</p>
  {:else if !stacks}
    <p class="faint"><Spinner size={13} /> Loading…</p>
  {:else if stacks.length === 0}
    <EmptyState icon={Layers} title="No stacks yet">
      Stacks are groups of containers defined in a compose file. Berth finds them on its own, and also lists folders in your stacks directory that haven't been deployed.
      {#snippet actions()}
        {#if session.isAdmin}<a class="btn primary" href={href('/stacks/new')}>Create a stack</a>{/if}
      {/snippet}
    </EmptyState>
  {:else}
    <div class="tools">
      <input class="input filter" type="search" placeholder="Filter stacks or services" bind:value={query} aria-label="Filter stacks" />
      {#if composeVersion}<span class="faint small">Compose {composeVersion}</span>{/if}
    </div>
    <div class="grid">
      {#each rows as r (r.stack.name)}
        <article class="card panel {r.status}">
          <header>
            <a class="name" href={href(`/stacks/${encodeURIComponent(r.stack.name)}`)}>{r.stack.name}</a>
            <span class="status">{STATUS_TEXT[r.status]}</span>
            {#if r.updates}<span class="chip upd">{plural(r.updates, 'update')}</span>{/if}
            <span class="grow"></span>
            {#if session.isAdmin}
              {#if r.status === 'inactive' || r.status === 'stopped'}
                <Button small icon={Play} onclick={() => stackAction(r.stack.name, r.status === 'inactive' ? 'up' : 'start')} disabled={!composeAvailable}>
                  {r.status === 'inactive' ? 'Deploy' : 'Start'}
                </Button>
              {:else}
                <Button small variant="ghost" icon={RotateCw} label="Restart" onclick={() => stackAction(r.stack.name, 'restart')} />
              {/if}
            {/if}
            <Menu items={menu(r)} small label="Stack actions" />
          </header>
          {#if r.stack.dir}<p class="dir mono ellipsis" title={r.stack.dir}>{r.stack.dir}</p>{/if}
          {#if r.members.length}
            <ul class="members">
              {#each r.members as m (m.name)}
                <li>
                  <a href={href(`/containers/${encodeURIComponent(m.name)}`)} title={m.name}>
                    <StateDot state={m.state} size={8} />
                    <span class="ellipsis">{m.service}</span>
                    {#if m.update}<span class="dot-upd" title="Update available"></span>{/if}
                  </a>
                </li>
              {/each}
            </ul>
          {:else}
            <p class="faint small none">Not running. {r.stack.editable ? 'Deploy it to create its containers.' : ''}</p>
          {/if}
          <footer class="faint small">
            {#if r.members.length}{r.running} of {r.members.length} running{/if}
            {#if !r.stack.editable}<span title="Berth can't read this stack's compose files; mount its folder into Berth to edit it.">, files not readable</span>{/if}
          </footer>
        </article>
      {/each}
    </div>
    {#if rows.length === 0}<p class="faint">No stacks match “{query}”.</p>{/if}
  {/if}
</div>

<style>
  .banner {
    display: flex;
    align-items: flex-start;
    gap: 10px;
    margin-bottom: 16px;
    padding: 12px 14px;
    border: 1px solid color-mix(in srgb, var(--warn) 40%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--warn) 8%, var(--panel));
    color: var(--text);
  }
  .banner :global(svg) {
    flex: none;
    color: var(--warn);
  }
  .tools {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 14px;
  }
  .filter {
    max-width: 320px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(330px, 1fr));
    gap: 14px;
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
    border-left: 3px solid var(--idle);
  }
  .card.running {
    border-left-color: var(--ok);
  }
  .card.partial {
    border-left-color: var(--warn);
  }
  .card.inactive {
    border-left-style: dashed;
  }
  header {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
  }
  .name {
    color: var(--text);
    font: 600 var(--fs-lg) / 1.2 var(--font-data);
  }
  .status {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .upd {
    color: var(--update);
  }
  .dir {
    margin: -4px 0 0;
    color: var(--text-3);
    font-size: 12px;
  }
  .members {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(130px, 1fr));
    gap: 4px 10px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .members a {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    padding: 3px 0;
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .dot-upd {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--update);
  }
  .none {
    margin: 0;
  }
  footer {
    margin-top: auto;
  }
</style>
