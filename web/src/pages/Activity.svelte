<!--
  Activity: a searchable log of everything that happened (container
  events, updates, backups, sign-ins, setting changes), and the list of
  background tasks with their logs.
-->
<script lang="ts">
  import { untrack } from 'svelte';
  import History from '@lucide/svelte/icons/history';
  import Search from '@lucide/svelte/icons/search';
  import X from '@lucide/svelte/icons/x';
  import EmptyState from '../components/EmptyState.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import Spinner from '../components/Spinner.svelte';
  import Tabs from '../components/Tabs.svelte';
  import { api, errorMessage, qs } from '../lib/api';
  import { actorLabel, duration, time } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href, router } from '../lib/router.svelte';
  import type { ActivityEntry, TaskSnapshot } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params }: { params: Record<string, string> } = $props();

  const tab = $derived(params.tab === 'tasks' ? 'tasks' : 'log');
  const PAGE = 100;

  const CATEGORIES = [
    { value: '', label: 'Everything' },
    { value: 'container', label: 'Containers' },
    { value: 'update', label: 'Updates' },
    { value: 'backup', label: 'Backups' },
    { value: 'restore', label: 'Restores' },
    { value: 'stack', label: 'Stacks' },
    { value: 'alert', label: 'Alerts' },
    { value: 'uptime', label: 'Uptime' },
    { value: 'docker', label: 'Images, volumes, networks' },
    { value: 'proxmox', label: 'Proxmox' },
    { value: 'security', label: 'Sign-ins and accounts' },
    { value: 'settings', label: 'Settings' },
    { value: 'system', label: 'Nodestarr' },
  ];

  let category = $state(router.query.get('kind') ?? '');
  let severity = $state(router.query.get('severity') ?? '');
  let search = $state(router.query.get('q') ?? '');
  let entries = $state<ActivityEntry[] | null>(null);
  let error = $state('');
  let loadingMore = $state(false);
  let done = $state(false);

  let tasks = $state<TaskSnapshot[] | null>(null);

  async function loadLog(more = false) {
    const before = more && entries?.length ? entries[entries.length - 1].id : null;
    if (more) loadingMore = true;
    try {
      const r = await api.get<{ entries: ActivityEntry[] }>(
        `/activity${qs({ kind: category, severity, search: search.trim(), before, limit: PAGE })}`,
      );
      entries = more && entries ? [...entries, ...r.entries] : r.entries;
      done = r.entries.length < PAGE;
      error = '';
    } catch (err) {
      error = errorMessage(err);
    } finally {
      loadingMore = false;
    }
  }

  // Reload when the filters change (debounced for typing), and keep them in the URL.
  $effect(() => {
    const k = category;
    const s = severity;
    const q = search.trim();
    if (tab !== 'log') return;
    const t = setTimeout(() => {
      router.setQuery({ kind: k || null, severity: s || null, q: q || null });
      void loadLog();
    }, 250);
    return () => clearTimeout(t);
  });

  // New entries arrive live; add the ones that match the filters.
  $effect(() => {
    const latest = live.activity[0];
    if (!latest) return;
    untrack(() => {
      if (!entries || entries.some((e) => e.id === latest.id)) return;
      const k = category;
      const matches =
        (!k || latest.kind === k || latest.kind.startsWith(`${k}.`)) &&
        (!severity || latest.severity === severity) &&
        (!search.trim() || latest.message.toLowerCase().includes(search.trim().toLowerCase()));
      if (matches) entries = [latest, ...entries];
    });
  });

  async function loadTasks() {
    try {
      const r = await api.get<{ tasks: TaskSnapshot[] }>('/tasks?limit=200');
      tasks = r.tasks;
    } catch (err) {
      error = errorMessage(err);
    }
  }

  $effect(() => {
    if (tab === 'tasks') void loadTasks();
  });

  // Merge live task updates into the list.
  const taskList = $derived.by(() => {
    if (!tasks) return null;
    const byId = new Map(tasks.map((t) => [t.id, t]));
    for (const t of Object.values(live.tasks)) byId.set(t.id, t);
    return [...byId.values()].sort((a, b) => b.created_at - a.created_at);
  });

  /** Entries grouped by calendar day, newest first. */
  const days = $derived.by(() => {
    const groups: { label: string; items: ActivityEntry[] }[] = [];
    const today = new Date().toDateString();
    const yesterday = new Date(Date.now() - 86_400_000).toDateString();
    for (const e of entries ?? []) {
      const d = new Date(e.ts * 1000);
      const key = d.toDateString();
      const label =
        key === today ? 'Today' : key === yesterday ? 'Yesterday' : d.toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric', year: d.getFullYear() === new Date().getFullYear() ? undefined : 'numeric' });
      const last = groups[groups.length - 1];
      if (last?.label === label) last.items.push(e);
      else groups.push({ label, items: [e] });
    }
    return groups;
  });

  function targetHref(e: ActivityEntry): string | null {
    if (!e.target) return null;
    if (e.target.startsWith('stack:')) return href(`/stacks/${encodeURIComponent(e.target.slice(6))}`);
    const c = live.byIdentity.get(e.target);
    return c ? href(`/containers/${encodeURIComponent(c.name)}`) : null;
  }

  async function cancel(t: TaskSnapshot) {
    try {
      await api.post(`/tasks/${encodeURIComponent(t.id)}/cancel`);
      ui.toast('Cancelling…');
    } catch (err) {
      ui.error(err);
    }
  }

  const STATUS: Record<string, string> = { queued: 'Waiting', running: 'Running', succeeded: 'Done', failed: 'Failed', cancelled: 'Cancelled' };

  const tabList = [
    { key: 'log', label: 'Activity', path: '/activity' },
    { key: 'tasks', label: 'Tasks', path: '/activity/tasks' },
  ];
</script>

<PageHeader title="Activity">
  {#snippet tabs()}
    <Tabs tabs={tabList} active={tab} />
  {/snippet}
</PageHeader>

<div class="page-body">
  {#if tab === 'log'}
    <div class="filters">
      <label class="search">
        <Search size={15} />
        <input class="input" type="search" placeholder="Search messages" bind:value={search} aria-label="Search activity" />
      </label>
      <select class="input" bind:value={category} aria-label="What">
        {#each CATEGORIES as c (c.value)}<option value={c.value}>{c.label}</option>{/each}
      </select>
      <select class="input" bind:value={severity} aria-label="Severity">
        <option value="">Any severity</option>
        <option value="error">Problems</option>
        <option value="warning">Warnings</option>
        <option value="success">Successes</option>
        <option value="info">Information</option>
      </select>
      {#if category || severity || search}
        <button type="button" class="btn small ghost" onclick={() => ((category = ''), (severity = ''), (search = ''))}><X size={14} />Clear</button>
      {/if}
    </div>

    {#if error}
      <p class="text-bad">{error}</p>
    {:else if !entries}
      <p class="faint"><Spinner size={13} /> Loading…</p>
    {:else if entries.length === 0}
      <EmptyState icon={History} title={category || severity || search ? 'Nothing matches' : 'Nothing has happened yet'} />
    {:else}
      <div class="log">
        {#each days as day (day.label)}
          <section>
            <h2 class="day">{day.label}</h2>
            <ul class="panel">
              {#each day.items as e (e.id)}
                {@const link = targetHref(e)}
                <li class="sev-{e.severity}">
                  <span class="t num">{time(e.ts)}</span>
                  <span class="mark" aria-hidden="true"></span>
                  <span class="msg">
                    {e.message}
                    {#if e.target_name && link}<a class="target" href={link}>{e.target_name}</a>{/if}
                  </span>
                  <span class="who faint">{actorLabel(e.actor)}</span>
                </li>
              {/each}
            </ul>
          </section>
        {/each}
        {#if !done}
          <button type="button" class="btn more" onclick={() => loadLog(true)} disabled={loadingMore}>
            {#if loadingMore}<Spinner size={14} />{/if} Show older
          </button>
        {/if}
      </div>
    {/if}
  {:else}
    {#if !taskList}
      <p class="faint"><Spinner size={13} /> Loading…</p>
    {:else if taskList.length === 0}
      <EmptyState icon={History} title="No tasks yet">Updates, backups, restores and stack actions run as tasks and show up here.</EmptyState>
    {:else}
      <section class="panel">
        <div class="table-wrap">
          <table class="table">
            <thead>
              <tr><th>Started</th><th>Task</th><th>By</th><th>Took</th><th>Result</th><th class="shrink"></th></tr>
            </thead>
            <tbody>
              {#each taskList as t (t.id)}
                <tr class="clickable" onclick={() => (ui.openTaskId = t.id)}>
                  <td class="shrink num">{time(t.created_at)} <span class="faint small">{new Date(t.created_at * 1000).toLocaleDateString(undefined, { month: 'short', day: 'numeric' })}</span></td>
                  <td>
                    {t.title}
                    {#if t.error}<div class="text-bad small">{t.error}</div>{:else if t.message && t.status === 'running'}<div class="faint small">{t.message}</div>{/if}
                  </td>
                  <td class="shrink">{actorLabel(t.actor)}</td>
                  <td class="shrink num">{t.started_at ? duration((t.finished_at ?? Date.now() / 1000) - t.started_at) : '–'}</td>
                  <td class="shrink"><span class="status {t.status}">{STATUS[t.status] ?? t.status}</span></td>
                  <td class="shrink">
                    {#if t.status === 'running' || t.status === 'queued'}
                      <button type="button"
                        class="btn small ghost"
                        onclick={(e) => {
                          e.stopPropagation();
                          void cancel(t);
                        }}>Cancel</button
                      >
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    {/if}
  {/if}
</div>

<style>
  .filters {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 16px;
  }
  .filters select {
    width: auto;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    width: min(320px, 100%);
    color: var(--text-3);
  }
  .search :global(svg) {
    position: absolute;
    left: 10px;
    pointer-events: none;
  }
  .search .input {
    padding-left: 32px;
  }
  .log {
    display: grid;
    gap: 18px;
    max-width: 1100px;
  }
  .day {
    margin-bottom: 8px;
    color: var(--text-2);
    font: 600 var(--fs-sm) / 1 var(--font-data);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  ul {
    margin: 0;
    padding: 4px 0;
    list-style: none;
  }
  li {
    display: grid;
    grid-template-columns: 56px 10px minmax(0, 1fr) auto;
    align-items: baseline;
    gap: 10px;
    padding: 7px 16px;
  }
  li + li {
    border-top: 1px solid var(--line);
  }
  .t {
    color: var(--text-3);
    font-size: var(--fs-sm);
  }
  .mark {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--info);
    align-self: center;
  }
  .sev-success .mark {
    background: var(--ok);
  }
  .sev-warning .mark {
    background: var(--warn);
  }
  .sev-error .mark {
    background: var(--bad);
  }
  .msg {
    overflow-wrap: anywhere;
  }
  .target {
    margin-left: 6px;
    font-size: var(--fs-sm);
  }
  .who {
    font-size: var(--fs-sm);
    white-space: nowrap;
  }
  .more {
    justify-self: center;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .status {
    font-weight: 500;
  }
  .status.succeeded {
    color: var(--ok);
  }
  .status.failed {
    color: var(--bad);
  }
  .status.running,
  .status.queued {
    color: var(--info);
  }
  .status.cancelled {
    color: var(--text-3);
  }
  @media (max-width: 760px) {
    li {
      grid-template-columns: 48px 10px minmax(0, 1fr);
    }
    .who {
      grid-column: 3;
    }
  }
</style>
