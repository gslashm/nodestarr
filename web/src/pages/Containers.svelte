<!--
  Every container in one table (or as the yard): filter, sort, search,
  group by stack, and act on one or many at once.
-->
<script lang="ts">
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import ArchiveIcon from '@lucide/svelte/icons/archive';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import ChevronUp from '@lucide/svelte/icons/chevron-up';
  import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import FileText from '@lucide/svelte/icons/file-text';
  import LayoutGrid from '@lucide/svelte/icons/layout-grid';
  import List from '@lucide/svelte/icons/list';
  import Play from '@lucide/svelte/icons/play';
  import Plus from '@lucide/svelte/icons/plus';
  import RotateCw from '@lucide/svelte/icons/rotate-cw';
  import Search from '@lucide/svelte/icons/search';
  import SearchX from '@lucide/svelte/icons/search-x';
  import Square from '@lucide/svelte/icons/square';
  import X from '@lucide/svelte/icons/x';
  import AppIcon from '../components/AppIcon.svelte';
  import Button from '../components/Button.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import Menu from '../components/Menu.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import Sparkline from '../components/Sparkline.svelte';
  import StatePill from '../components/StatePill.svelte';
  import TimeAgo from '../components/TimeAgo.svelte';
  import Yard from '../components/Yard.svelte';
  import { backupNow, confirmUpdate, containerMenu, runAction, stackAction, startUpdate, webUrl } from '../lib/actions';
  import { api } from '../lib/api';
  import { clock } from '../lib/clock.svelte';
  import { bytes, duration, hasUpdate, imageTag, isUp, plural, shortImage, stateOf, updateLabel } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href, router } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { ContainerView } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params: _params }: { params: Record<string, string> } = $props();

  type Filter = 'all' | 'running' | 'stopped' | 'updates' | 'attention';
  type SortKey = 'name' | 'state' | 'cpu' | 'mem' | 'uptime' | 'backup';

  const VIEW_KEY = 'berth.containers.view';
  let view = $state<'table' | 'yard'>(readView());
  let query = $state(router.query.get('q') ?? '');
  let filter = $state<Filter>((router.query.get('state') as Filter) || 'all');
  let sort = $state<SortKey>('name');
  let desc = $state(false);
  let selected = $state<Set<string>>(new Set());
  const grouped = $derived(session.settings?.general.ui.group_by_stack ?? true);

  function readView(): 'table' | 'yard' {
    try {
      return localStorage.getItem(VIEW_KEY) === 'yard' ? 'yard' : 'table';
    } catch {
      return 'table';
    }
  }

  function setView(v: 'table' | 'yard') {
    view = v;
    try {
      localStorage.setItem(VIEW_KEY, v);
    } catch {
      // Not persisted; fine.
    }
  }

  function needsAttention(c: ContainerView): boolean {
    const s = stateOf(c);
    return s === 'unhealthy' || s === 'crashed' || s === 'restarting' || s === 'dead' || c.uptime?.state === 'down' || !!c.app_status?.health.some((h) => h.level === 'error');
  }

  const counts = $derived({
    all: live.list.length,
    running: live.list.filter((c) => isUp(stateOf(c))).length,
    stopped: live.list.filter((c) => !isUp(stateOf(c))).length,
    updates: live.list.filter((c) => hasUpdate(c.update) && c.policy !== 'ignore').length,
    attention: live.list.filter(needsAttention).length,
  });

  const filtered = $derived.by(() => {
    const q = query.trim().toLowerCase();
    return live.list.filter((c) => {
      const s = stateOf(c);
      if (filter === 'running' && !isUp(s)) return false;
      if (filter === 'stopped' && isUp(s)) return false;
      if (filter === 'updates' && !(hasUpdate(c.update) && c.policy !== 'ignore')) return false;
      if (filter === 'attention' && !needsAttention(c)) return false;
      if (!q) return true;
      return [c.display_name, c.name, c.image, c.stack ?? '', c.app_name ?? '', c.version ?? '', ...c.prefs.tags].some((v) => v.toLowerCase().includes(q));
    });
  });

  const STATE_ORDER: Record<string, number> = { crashed: 0, unhealthy: 1, restarting: 2, dead: 3, starting: 4, running: 5, healthy: 5, paused: 6, stopped: 7, created: 8 };

  const sorted = $derived.by(() => {
    const dir = desc ? -1 : 1;
    const value = (c: ContainerView): number | string => {
      switch (sort) {
        case 'state':
          return STATE_ORDER[stateOf(c)] ?? 9;
        case 'cpu':
          return c.stats?.cpu ?? -1;
        case 'mem':
          return c.stats?.mem ?? -1;
        case 'uptime':
          return isUp(stateOf(c)) && c.started_at ? -c.started_at : -Infinity;
        case 'backup':
          return c.last_backup ?? 0;
        default:
          return c.display_name.toLowerCase();
      }
    };
    return [...filtered].sort((a, b) => {
      const va = value(a);
      const vb = value(b);
      return (va < vb ? -1 : va > vb ? 1 : 0) * dir;
    });
  });

  const groups = $derived.by(() => {
    if (!grouped || sort !== 'name') return [{ stack: null as string | null, items: sorted }];
    const map = new Map<string, ContainerView[]>();
    for (const c of sorted) {
      const k = c.stack ?? '';
      if (!map.has(k)) map.set(k, []);
      map.get(k)!.push(c);
    }
    return [...map.entries()]
      .sort(([a], [b]) => (a === '' ? 1 : b === '' ? -1 : a.localeCompare(b)))
      .map(([stack, items]) => ({ stack: stack || null, items }));
  });

  const selectedViews = $derived(live.list.filter((c) => selected.has(c.id)));
  const allVisibleSelected = $derived(sorted.length > 0 && sorted.every((c) => selected.has(c.id)));

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  function toggleAll() {
    selected = allVisibleSelected ? new Set() : new Set(sorted.map((c) => c.id));
  }

  function setSort(k: SortKey) {
    if (sort === k) desc = !desc;
    else {
      sort = k;
      desc = k === 'cpu' || k === 'mem' || k === 'backup';
    }
  }

  function setFilter(f: Filter) {
    filter = f;
    router.setQuery({ state: f === 'all' ? null : f });
  }

  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  function onSearch() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(() => router.setQuery({ q: query.trim() || null }), 300);
  }

  async function bulk(action: 'start' | 'stop' | 'restart' | 'update' | 'backup') {
    const items = selectedViews;
    if (!items.length) return;
    if (action === 'stop' || action === 'restart') {
      const ok = await ui.confirm({
        title: `${action === 'stop' ? 'Stop' : 'Restart'} ${plural(items.length, 'container')}?`,
        message: items.map((c) => c.display_name).join(', '),
        confirmLabel: action === 'stop' ? 'Stop all' : 'Restart all',
      });
      if (!ok) return;
    }
    let done = 0;
    for (const c of items) {
      try {
        if (action === 'update') {
          if (!hasUpdate(c.update)) continue;
          await startUpdate(c, c.update?.newer_tag?.tag);
        } else if (action === 'backup') {
          await backupNow(c);
        } else {
          await api.post(`/containers/${encodeURIComponent(c.id)}/actions/${action}`);
        }
        done++;
      } catch (err) {
        ui.error(err, c.display_name);
      }
    }
    if (action !== 'update' && action !== 'backup') ui.toast(`${done} of ${plural(items.length, 'container')} ${action === 'stop' ? 'stopped' : action === 'start' ? 'started' : 'restarted'}`, 'success');
    selected = new Set();
  }

  async function checkAll() {
    try {
      const r = await api.post<{ started: boolean; message?: string }>('/updates/check');
      ui.toast(r.started ? 'Checking every container for updates…' : (r.message ?? 'A check is already running'));
    } catch (err) {
      ui.error(err);
    }
  }

  function uptimeText(c: ContainerView): string {
    void clock.tick;
    const s = stateOf(c);
    if (isUp(s) && c.started_at) return `Up ${duration(clock.now - c.started_at)}`;
    if (c.finished_at) return `${duration(clock.now - c.finished_at)} ago`;
    return '';
  }

  const filters: { key: Filter; label: string }[] = [
    { key: 'all', label: 'All' },
    { key: 'running', label: 'Running' },
    { key: 'stopped', label: 'Stopped' },
    { key: 'updates', label: 'Updates' },
    { key: 'attention', label: 'Needs attention' },
  ];
</script>

<PageHeader title="Containers" subtitle="{counts.running} running, {counts.stopped} stopped">
  {#snippet actions()}
    <Button variant="ghost" icon={RotateCw} onclick={() => live.refresh()}>Refresh</Button>
    {#if session.isAdmin}
      <Button variant="ghost" icon={CircleArrowUp} busy={live.checkingUpdates} onclick={checkAll}>Check for updates</Button>
      <a class="btn primary" href={href('/containers/new')}><Plus size={17} />Add container</a>
    {/if}
  {/snippet}
</PageHeader>

<div class="toolbar">
  <div class="filters" role="tablist" aria-label="Show">
    {#each filters as f (f.key)}
      <button type="button" role="tab" aria-selected={filter === f.key} class:active={filter === f.key} class:hot={f.key === 'attention' && counts.attention > 0} onclick={() => setFilter(f.key)}>
        {f.label}
        <span class="num count">{counts[f.key]}</span>
      </button>
    {/each}
  </div>
  <div class="right">
    <label class="search">
      <Search size={16} />
      <input placeholder="Filter by name, image, stack or tag" bind:value={query} oninput={onSearch} aria-label="Filter containers" />
      {#if query}<button type="button" class="clear" aria-label="Clear filter" onclick={() => ((query = ''), onSearch())}><X size={14} /></button>{/if}
    </label>
    <div class="views" role="radiogroup" aria-label="View">
      <button type="button" role="radio" aria-checked={view === 'table'} class:active={view === 'table'} title="Table" onclick={() => setView('table')}><List size={17} /></button>
      <button type="button" role="radio" aria-checked={view === 'yard'} class:active={view === 'yard'} title="Yard" onclick={() => setView('yard')}><LayoutGrid size={17} /></button>
    </div>
  </div>
</div>

{#if selected.size > 0 && session.isAdmin}
  <div class="bulk" role="region" aria-label="Selected containers">
    <span class="num"><strong>{selected.size}</strong> selected</span>
    <Button small icon={Play} onclick={() => bulk('start')}>Start</Button>
    <Button small icon={RotateCw} onclick={() => bulk('restart')}>Restart</Button>
    <Button small icon={Square} onclick={() => bulk('stop')}>Stop</Button>
    <Button small icon={ArrowUp} onclick={() => bulk('update')} disabled={!selectedViews.some((c) => hasUpdate(c.update))}>Update</Button>
    <Button small icon={ArchiveIcon} onclick={() => bulk('backup')}>Back up</Button>
    <button type="button" class="btn ghost small" onclick={() => (selected = new Set())}>Clear selection</button>
  </div>
{/if}

<div class="body">
  {#if !live.loaded}
    <p class="faint pad">Loading…</p>
  {:else if sorted.length === 0}
    <EmptyState icon={SearchX} title={live.list.length ? 'No containers match' : 'No containers yet'}>
      {live.list.length ? 'Try another filter or search.' : 'Add a container or create a compose stack to get started.'}
      {#snippet actions()}
        {#if live.list.length}
          <button type="button" class="btn" onclick={() => ((query = ''), setFilter('all'), onSearch())}>Show all containers</button>
        {:else if session.isAdmin}
          <a class="btn primary" href={href('/containers/new')}>Add a container</a>
        {/if}
      {/snippet}
    </EmptyState>
  {:else if view === 'yard'}
    <div class="panel yard"><Yard containers={sorted} /></div>
  {:else}
    <div class="panel table-wrap">
      <table class="table">
        <thead>
          <tr>
            {#if session.isAdmin}
              <th class="shrink"><input type="checkbox" checked={allVisibleSelected} onchange={toggleAll} aria-label="Select all" /></th>
            {/if}
            <th><button type="button" class="sort" onclick={() => setSort('name')}>Name {#if sort === 'name'}{#if desc}<ChevronDown size={13} />{:else}<ChevronUp size={13} />{/if}{/if}</button></th>
            <th><button type="button" class="sort" onclick={() => setSort('state')}>State {#if sort === 'state'}{#if desc}<ChevronDown size={13} />{:else}<ChevronUp size={13} />{/if}{/if}</button></th>
            <th>Version</th>
            <th class="hide-md">Image</th>
            <th class="num"><button type="button" class="sort" onclick={() => setSort('cpu')}>CPU {#if sort === 'cpu'}{#if desc}<ChevronDown size={13} />{:else}<ChevronUp size={13} />{/if}{/if}</button></th>
            <th class="num"><button type="button" class="sort" onclick={() => setSort('mem')}>Memory {#if sort === 'mem'}{#if desc}<ChevronDown size={13} />{:else}<ChevronUp size={13} />{/if}{/if}</button></th>
            <th class="hide-lg"><button type="button" class="sort" onclick={() => setSort('backup')}>Backed up {#if sort === 'backup'}{#if desc}<ChevronDown size={13} />{:else}<ChevronUp size={13} />{/if}{/if}</button></th>
            <th class="shrink"><span class="sr-only">Actions</span></th>
          </tr>
        </thead>
        {#each groups as g (g.stack ?? '__standalone')}
          <tbody>
            {#if grouped && sort === 'name'}
              <tr class="group">
                <td colspan={session.isAdmin ? 9 : 8}>
                  <div class="group-row">
                    {#if g.stack}
                      <a href={href(`/stacks/${encodeURIComponent(g.stack)}`)} class="group-name">{g.stack}</a>
                      <span class="faint num">{g.items.length} {g.items.length === 1 ? 'container' : 'containers'}</span>
                      {#if session.isAdmin}
                        <span class="grow"></span>
                        <Menu
                          small
                          label="Stack actions"
                          items={[
                            { label: 'Restart stack', icon: RotateCw, run: () => stackAction(g.stack!, 'restart') },
                            { label: 'Pull and update stack', icon: CircleArrowUp, run: () => stackAction(g.stack!, 'update') },
                            { label: 'Deploy (compose up)', icon: Play, run: () => stackAction(g.stack!, 'up') },
                            { label: 'Stop stack', icon: Square, run: () => stackAction(g.stack!, 'stop') },
                          ]}
                        />
                      {/if}
                    {:else}
                      <span class="group-name plain">Standalone</span>
                      <span class="faint num">{g.items.length} {g.items.length === 1 ? 'container' : 'containers'}</span>
                    {/if}
                  </div>
                </td>
              </tr>
            {/if}
            {#each g.items as c (c.id)}
              {@const st = stateOf(c)}
              {@const up = isUp(st)}
              {@const web = webUrl(c)}
              <tr class="clickable" class:selected={selected.has(c.id)} onclick={(e) => {
                if ((e.target as HTMLElement).closest('a, button, input, .menu')) return;
                router.navigate(`/containers/${encodeURIComponent(c.name)}`);
              }}>
                {#if session.isAdmin}
                  <td class="shrink"><input type="checkbox" checked={selected.has(c.id)} onchange={() => toggle(c.id)} aria-label="Select {c.display_name}" /></td>
                {/if}
                <td>
                  <div class="name-cell">
                    <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={30} />
                    <div class="names">
                      <a href={href(`/containers/${encodeURIComponent(c.name)}`)} class="name">{c.display_name}</a>
                      <span class="sub ellipsis">{c.name}{#if c.prefs.tags.length}{#each c.prefs.tags as t (t)}<span class="tag">{t}</span>{/each}{/if}</span>
                    </div>
                  </div>
                </td>
                <td>
                  <div class="state-cell">
                    <StatePill state={st} />
                    <span class="faint num small">{uptimeText(c)}</span>
                  </div>
                </td>
                <td>
                  <div class="version">
                    <span class="num">{c.version ?? imageTag(c.image)}</span>
                    {#if hasUpdate(c.update) && c.policy !== 'ignore'}
                      <button type="button" class="upd" title="Update available" onclick={() => (session.isAdmin ? confirmUpdate(c) : router.navigate(`/containers/${encodeURIComponent(c.name)}/updates`))}>
                        <ArrowUp size={13} strokeWidth={2.5} />{updateLabel(c.update)}
                      </button>
                    {:else if c.update?.status === 'error'}
                      <span class="faint small" title={c.update.error ?? ''}>check failed</span>
                    {/if}
                  </div>
                </td>
                <td class="hide-md"><span class="image ellipsis" title={c.image}>{shortImage(c.image)}</span></td>
                <td class="num">
                  {#if up && c.stats}
                    <div class="cpu">
                      <Sparkline values={live.cpuTrail[c.identity] ?? []} width={54} height={18} label="CPU trend" />
                      <span>{c.stats.cpu.toFixed(c.stats.cpu < 10 ? 1 : 0)}%</span>
                    </div>
                  {:else}<span class="faint">–</span>{/if}
                </td>
                <td class="num">{#if up && c.stats}{bytes(c.stats.mem)}{:else}<span class="faint">–</span>{/if}</td>
                <td class="hide-lg">
                  {#if c.last_backup}<TimeAgo ts={c.last_backup} />{:else if c.prefs.backup_enabled}<span class="faint">never</span>{:else}<span class="faint">off</span>{/if}
                </td>
                <td class="shrink">
                  <div class="row-actions">
                    {#if web}
                      <a class="btn ghost icon small" href={web} target="_blank" rel="noopener" aria-label="Open {c.display_name}" title="Open web UI"><ExternalLink size={15} /></a>
                    {/if}
                    <a class="btn ghost icon small" href={href(`/containers/${encodeURIComponent(c.name)}/logs`)} aria-label="Logs of {c.display_name}" title="Logs"><FileText size={15} /></a>
                    {#if session.isAdmin}
                      {#if up}
                        <Button small variant="ghost" icon={RotateCw} label="Restart {c.display_name}" onclick={() => runAction(c, 'restart')} busy={c.busy} />
                      {:else}
                        <Button small variant="ghost" icon={Play} label="Start {c.display_name}" onclick={() => runAction(c, 'start')} busy={c.busy} />
                      {/if}
                    {/if}
                    <Menu small items={containerMenu(c)} label="More for {c.display_name}" />
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        {/each}
      </table>
    </div>
  {/if}
</div>

<style>
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 14px 24px 0;
  }
  .filters {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .filters button {
    display: inline-flex;
    align-items: center;
    gap: 7px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid transparent;
    border-radius: 16px;
    background: none;
    color: var(--text-2);
    font: 500 var(--fs-md) / 1 var(--font-ui);
    cursor: pointer;
  }
  .filters button:hover {
    color: var(--text);
    background: var(--raised);
  }
  .filters button.active {
    border-color: var(--line-strong);
    background: var(--panel);
    color: var(--text);
  }
  .count {
    color: var(--text-3);
    font-size: var(--fs-sm);
  }
  .filters button.hot .count {
    color: var(--bad);
  }
  .right {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 300px;
    height: 34px;
    padding: 0 8px 0 10px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
    background: var(--sunken);
    color: var(--text-3);
  }
  .search:focus-within {
    border-color: var(--accent);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
  }
  .search input:focus {
    outline: none;
  }
  .clear {
    display: grid;
    place-items: center;
    border: 0;
    background: none;
    color: var(--text-3);
    cursor: pointer;
  }
  .views {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius);
  }
  .views button {
    display: grid;
    place-items: center;
    width: 30px;
    height: 28px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-3);
    cursor: pointer;
  }
  .views button.active {
    background: var(--raised);
    color: var(--text);
  }
  .bulk {
    position: sticky;
    top: var(--toolbar-h);
    z-index: 15;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin: 12px 24px 0;
    padding: 8px 12px;
    border: 1px solid color-mix(in srgb, var(--accent) 50%, var(--line));
    border-radius: var(--radius);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
  .bulk .num {
    margin-right: 6px;
  }
  .body {
    padding: 14px 24px 40px;
  }
  .pad {
    padding: 20px 0;
  }
  .yard {
    padding: 14px 18px;
  }
  .sort {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 0;
    border: 0;
    background: none;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .sort:hover {
    color: var(--text);
  }
  tr.group td {
    height: 40px;
    padding-top: 10px;
    background: var(--bg);
  }
  tbody:first-of-type tr.group td {
    padding-top: 4px;
  }
  .table tbody tr.group:hover td {
    background: var(--bg);
  }
  .group-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .group-name {
    color: var(--text);
    font: 600 var(--fs-md) / 1 var(--font-data);
  }
  .group-name.plain {
    color: var(--text-2);
  }
  .name-cell {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 200px;
  }
  .names {
    display: grid;
    min-width: 0;
  }
  .name {
    color: var(--text);
    font-weight: 600;
  }
  .sub {
    display: flex;
    gap: 6px;
    align-items: center;
    color: var(--text-3);
    font-size: var(--fs-sm);
  }
  .tag {
    padding: 0 6px;
    border-radius: 8px;
    background: var(--raised);
    color: var(--text-2);
    font-size: var(--fs-xs);
  }
  .state-cell {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
  }
  .small {
    font-size: var(--fs-xs);
  }
  .version {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 3px;
  }
  .upd {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    height: 20px;
    padding: 0 7px 0 5px;
    border: 0;
    border-radius: 10px;
    background: color-mix(in srgb, var(--update) 16%, transparent);
    color: var(--update);
    font: 600 var(--fs-xs) / 1 var(--font-data);
    cursor: pointer;
  }
  .upd:hover {
    background: color-mix(in srgb, var(--update) 26%, transparent);
  }
  .image {
    display: block;
    max-width: 260px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .cpu {
    display: inline-flex;
    align-items: center;
    gap: 8px;
  }
  .row-actions {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 2px;
  }
  @media (max-width: 1280px) {
    .hide-lg {
      display: none;
    }
  }
  @media (max-width: 1100px) {
    .hide-md {
      display: none;
    }
  }
  @media (max-width: 760px) {
    .toolbar,
    .body {
      padding-left: 12px;
      padding-right: 12px;
    }
    .search {
      width: 100%;
    }
    .right {
      width: 100%;
    }
    .bulk {
      margin: 10px 12px 0;
    }
    /* Rows become cards on phones. */
    .table thead {
      display: none;
    }
    .table,
    .table tbody,
    .table tr,
    .table td {
      display: block;
    }
    .table tr {
      position: relative;
      padding: 10px 12px;
      border-bottom: 1px solid var(--line);
    }
    .table td {
      height: auto;
      padding: 2px 0;
      border: 0;
    }
    .table td.num {
      display: inline-block;
      text-align: left;
      margin-right: 14px;
    }
    .table td.shrink:first-child {
      position: absolute;
      top: 18px;
      right: 12px;
    }
    .table td.shrink:last-child {
      margin-top: 6px;
    }
    .row-actions {
      justify-content: flex-start;
    }
    .name-cell {
      min-width: 0;
    }
    tr.group td {
      padding: 0;
    }
  }
</style>
