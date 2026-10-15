<!--
  Updates across all containers: what's available (with bulk updating),
  every container's update state and policy, the schedule, and history.
-->
<script lang="ts">
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import History from '@lucide/svelte/icons/history';
  import Play from '@lucide/svelte/icons/play';
  import Search from '@lucide/svelte/icons/search';
  import Settings from '@lucide/svelte/icons/settings';
  import AppIcon from '../components/AppIcon.svelte';
  import Button from '../components/Button.svelte';
  import EmptyState from '../components/EmptyState.svelte';
  import Menu from '../components/Menu.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import Spinner from '../components/Spinner.svelte';
  import Tabs from '../components/Tabs.svelte';
  import TimeAgo from '../components/TimeAgo.svelte';
  import UpdateHistory from '../components/UpdateHistory.svelte';
  import { confirmUpdate } from '../lib/actions';
  import { api, errorMessage, seg } from '../lib/api';
  import { ago, dateTime, hasUpdate, imageTag, plural, shortImage, updateLabel } from '../lib/format';
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { ContainerView, HistoryEntry, TagScope } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params }: { params: Record<string, string> } = $props();

  const tab = $derived(params.tab === 'all' || params.tab === 'history' ? params.tab : 'available');

  let schedule = $state<{ job: string; next: number | null }[]>([]);
  let lastCheck = $state<number | null>(null);
  let history = $state<HistoryEntry[] | null>(null);
  let historyError = $state('');
  let selected = $state<Set<string>>(new Set());
  let applying = $state(false);
  let showAllFailed = $state(false);

  async function loadOverview() {
    try {
      const r = await api.get<{ schedule: { job: string; next: number | null }[]; last_check: number | null }>('/updates');
      schedule = r.schedule;
      lastCheck = r.last_check;
    } catch {
      // The page still works from live data.
    }
  }

  async function loadHistory() {
    try {
      history = await api.get<HistoryEntry[]>('/updates/history?limit=200');
      historyError = '';
    } catch (err) {
      historyError = errorMessage(err);
    }
  }

  // Refresh the schedule when a check finishes.
  $effect(() => {
    void live.checkingUpdates;
    void loadOverview();
  });

  $effect(() => {
    if (live.lastUpdateCheck) lastCheck = live.lastUpdateCheck;
  });

  $effect(() => {
    if (tab === 'history') void loadHistory();
  });

  const SCOPE: Record<TagScope, string> = { digest: 'new build', patch: 'patch', minor: 'minor', major: 'major' };

  const available = $derived(live.list.filter((c) => hasUpdate(c.update) && c.policy !== 'ignore' && !c.prefs.hidden));
  const failed = $derived(live.list.filter((c) => c.update?.status === 'error' && !c.prefs.hidden));
  const next = $derived(Object.fromEntries(schedule.map((s) => [s.job, s.next])) as Record<string, number | null>);

  const tabList = $derived([
    { key: 'available', label: 'Available', path: '/updates', badge: available.length || null, tone: 'update' as const },
    { key: 'all', label: 'All containers', path: '/updates/all' },
    { key: 'history', label: 'History', path: '/updates/history' },
  ]);

  function toggle(id: string) {
    const s = new Set(selected);
    if (s.has(id)) s.delete(id);
    else s.add(id);
    selected = s;
  }

  const selectable = $derived(available.filter((c) => !c.is_self && !c.busy));
  const allSelected = $derived(selectable.length > 0 && selectable.every((c) => selected.has(c.id)));

  function toggleAll() {
    selected = allSelected ? new Set() : new Set(selectable.filter((c) => !c.prefs.held).map((c) => c.id));
  }

  async function checkAll() {
    try {
      const r = await api.post<{ started: boolean; message?: string }>('/updates/check');
      if (r.started) {
        live.checkingUpdates = true;
        ui.toast('Checking every container for updates…');
      } else ui.toast(r.message ?? 'A check is already running');
    } catch (err) {
      ui.error(err);
    }
  }

  async function runAuto() {
    try {
      await api.post('/updates/auto/run');
      ui.toast('Running automatic updates now. Containers set to update automatically are updated if their update qualifies.');
    } catch (err) {
      ui.error(err);
    }
  }

  async function applySelected() {
    const targets = available.filter((c) => selected.has(c.id));
    if (!targets.length) return;
    const backup = session.settings?.updates.backup_before_update;
    const r = await ui.ask({
      title: `Update ${plural(targets.length, 'container')}?`,
      message: `${targets.map((c) => `${c.display_name} (${updateLabel(c.update)})`).join(', ')}.${backup ? ' Each is backed up first.' : ''} They update one after another; any that fail to start are rolled back.`,
      confirmLabel: 'Update',
      options: backup ? [{ key: 'skip', label: 'Skip the pre-update backups' }] : [],
    });
    if (!r.ok) return;
    applying = true;
    try {
      const res = await api.post<{ task_ids: string[]; errors: { target: string; error: string }[] }>('/updates/apply', {
        targets: targets.map((c) => c.id),
        skip_backup: !!r.options.skip,
      });
      selected = new Set();
      if (res.task_ids.length) ui.toast(`Updating ${plural(res.task_ids.length, 'container')}`, 'info', { label: 'Show progress', run: () => (ui.openTaskId = res.task_ids[0]) });
      for (const e of res.errors) ui.toast(`${e.target}: ${e.error}`, 'error');
    } catch (err) {
      ui.error(err, "Couldn't start the updates");
    } finally {
      applying = false;
    }
  }

  async function skip(c: ContainerView) {
    try {
      await api.post(`/containers/${seg(c.id)}/skip`);
      ui.toast(`Skipped this build of ${c.display_name}`, 'success');
    } catch (err) {
      ui.error(err);
    }
  }

  function policyText(c: ContainerView): string {
    if (c.policy === 'ignore') return 'Ignored';
    if (c.policy === 'notify') return 'Tell me';
    return `Automatic (${SCOPE[c.auto_scope]})`;
  }

  function statusText(c: ContainerView): { text: string; tone: string } {
    const u = c.update;
    if (c.policy === 'ignore') return { text: 'Not checked', tone: 'faint' };
    switch (u?.status) {
      case 'available':
      case 'pending_recreate':
        return { text: updateLabel(u) ?? 'Available', tone: 'text-update' };
      case 'up_to_date':
        return { text: 'Up to date', tone: 'text-ok' };
      case 'error':
        return { text: 'Check failed', tone: 'text-bad' };
      case 'local':
        return { text: 'Local image', tone: 'faint' };
      case 'pinned':
        return { text: 'Pinned', tone: 'faint' };
      default:
        return { text: 'Not checked yet', tone: 'faint' };
    }
  }
</script>

<PageHeader title="Updates" subtitle={`${available.length ? plural(available.length, 'update') + ' available' : 'Everything is up to date'}${lastCheck ? `, checked ${ago(lastCheck)}` : ''}`}>
  {#snippet actions()}
    {#if session.isAdmin}
      <Button variant="ghost" icon={Search} busy={live.checkingUpdates} onclick={checkAll}>Check now</Button>
      <Menu
        label="More"
        items={[
          { label: 'Run automatic updates now', icon: Play, run: runAuto },
          { label: 'Update settings', icon: Settings, href: href('/settings/updates') },
        ]}
      />
    {/if}
  {/snippet}
  {#snippet tabs()}
    <Tabs tabs={tabList} active={tab} />
  {/snippet}
</PageHeader>

<div class="page-body">
  <div class="schedule">
    <span><span class="faint">Next check</span> {next['update-check'] ? dateTime(next['update-check']) : 'not scheduled'}</span>
    <span>
      <span class="faint">Automatic updates</span>
      {next['auto-update'] ? dateTime(next['auto-update']) : 'off'}
    </span>
    {#if session.settings}
      <span><span class="faint">New containers</span> {session.settings.updates.default_policy === 'auto' ? 'update automatically' : session.settings.updates.default_policy === 'notify' ? 'tell you about updates' : 'are not checked'}</span>
    {/if}
    <a href={href('/settings/updates')}>Change</a>
  </div>

  {#if tab === 'available'}
    {#if !live.loaded}
      <p class="faint"><Spinner size={13} /> Loading…</p>
    {:else if available.length === 0}
      <EmptyState icon={CircleCheck} title="Everything is up to date">
        {lastCheck ? `Last checked ${ago(lastCheck)}.` : 'No check has run yet.'}
        {#snippet actions()}
          {#if session.isAdmin}<Button icon={Search} busy={live.checkingUpdates} onclick={checkAll}>Check now</Button>{/if}
        {/snippet}
      </EmptyState>
    {:else}
      {#if session.isAdmin && selected.size}
        <div class="bulk">
          <span><strong>{selected.size}</strong> selected</span>
          <Button small variant="primary" icon={ArrowUp} busy={applying} onclick={applySelected}>Update selected</Button>
          <button type="button" class="btn small ghost" onclick={() => (selected = new Set())}>Clear</button>
        </div>
      {/if}
      <section class="panel">
        <div class="table-wrap">
          <table class="table">
            <thead>
              <tr>
                {#if session.isAdmin}<th class="shrink"><input type="checkbox" checked={allSelected} onchange={toggleAll} aria-label="Select all" /></th>{/if}
                <th>Container</th>
                <th>Running</th>
                <th>Available</th>
                <th class="hide-md">Published</th>
                <th class="hide-md">Policy</th>
                <th class="shrink"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              {#each available as c (c.id)}
                {@const u = c.update!}
                <tr>
                  {#if session.isAdmin}
                    <td class="shrink"><input type="checkbox" checked={selected.has(c.id)} onchange={() => toggle(c.id)} disabled={c.is_self || c.busy} aria-label="Select {c.display_name}" /></td>
                  {/if}
                  <td>
                    <a class="who" href={href(`/containers/${encodeURIComponent(c.name)}/updates`)}>
                      <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={26} />
                      <span class="names">
                        <span class="name">{c.display_name}</span>
                        <span class="faint small ellipsis">{c.stack ?? shortImage(c.image)}</span>
                      </span>
                    </a>
                  </td>
                  <td class="ver">{c.version ?? imageTag(c.image)}</td>
                  <td>
                    <span class="ver text-update">{updateLabel(u)}</span>
                    <span class="faint small">{u.newer_tag ? `${SCOPE[u.newer_tag.scope]} version` : 'new build'}</span>
                  </td>
                  <td class="hide-md">{#if u.latest_created}<TimeAgo ts={u.latest_created} />{:else}<span class="faint">–</span>{/if}</td>
                  <td class="hide-md">
                    {policyText(c)}
                    {#if c.prefs.held}<span class="chip held">On hold</span>{/if}
                  </td>
                  <td class="shrink">
                    {#if session.isAdmin}
                      <div class="row-actions">
                        {#if u.status === 'available' && !u.newer_tag && u.latest_digest}
                          <button type="button" class="btn small ghost" onclick={() => skip(c)} title="Don't offer this build again">Skip</button>
                        {/if}
                        <Button small icon={ArrowUp} onclick={() => confirmUpdate(c)} disabled={c.busy || c.is_self} busy={c.busy}>Update</Button>
                      </div>
                    {/if}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
      {#if available.some((c) => c.is_self)}
        <p class="faint small note">Berth updates itself from Settings > System.</p>
      {/if}
    {/if}

    {#if failed.length}
      <section class="panel failed">
        <div class="panel-head"><h2>Checks that failed</h2></div>
        <ul>
          {#each showAllFailed ? failed : failed.slice(0, 5) as c (c.id)}
            <li>
              <a href={href(`/containers/${encodeURIComponent(c.name)}/updates`)}>{c.display_name}</a>
              <span class="faint small">{shortImage(c.image)}</span>
              <span class="text-bad small err">{c.update?.error}</span>
            </li>
          {/each}
        </ul>
        {#if failed.length > 5 && !showAllFailed}
          <button type="button" class="btn small ghost more" onclick={() => (showAllFailed = true)}>Show {failed.length - 5} more</button>
        {/if}
      </section>
    {/if}
  {:else if tab === 'all'}
    <section class="panel">
      <div class="table-wrap">
        <table class="table">
          <thead>
            <tr>
              <th>Container</th>
              <th>Image</th>
              <th>Version</th>
              <th>Status</th>
              <th class="hide-md">Policy</th>
              <th class="hide-md">Checked</th>
            </tr>
          </thead>
          <tbody>
            {#each live.list.filter((c) => !c.prefs.hidden) as c (c.id)}
              {@const st = statusText(c)}
              <tr>
                <td><a href={href(`/containers/${encodeURIComponent(c.name)}/updates`)}>{c.display_name}</a></td>
                <td class="mono small ellipsis image" title={c.image}>{shortImage(c.image)}</td>
                <td class="ver">{c.version ?? '–'}</td>
                <td class={st.tone} title={c.update?.error ?? ''}>{st.text}</td>
                <td class="hide-md">{policyText(c)}{#if c.prefs.held} <span class="chip held">On hold</span>{/if}</td>
                <td class="hide-md">{#if c.update?.checked_at}<TimeAgo ts={c.update.checked_at} />{:else}<span class="faint">never</span>{/if}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </section>
  {:else}
    <section class="panel">
      {#if historyError}
        <p class="panel-body text-bad">{historyError}</p>
      {:else if !history}
        <p class="panel-body faint"><Spinner size={13} /> Loading…</p>
      {:else if history.length === 0}
        <EmptyState icon={History} title="No updates yet">Updates made by Berth, by hand or automatically, show up here.</EmptyState>
      {:else}
        <UpdateHistory entries={history} showTarget />
      {/if}
    </section>
  {/if}
</div>

<style>
  .schedule {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 22px;
    margin-bottom: 16px;
    font-size: var(--fs-sm);
  }
  .bulk {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 12px;
    padding: 8px 12px;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: var(--accent-soft);
  }
  .who {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text);
    min-width: 0;
  }
  .who:hover {
    text-decoration: none;
  }
  .who:hover .name {
    text-decoration: underline;
  }
  .names {
    display: grid;
    min-width: 0;
  }
  .name {
    font-weight: 500;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .ver {
    font-family: var(--font-data);
    font-variant-numeric: tabular-nums;
  }
  td .ver + .small {
    display: block;
  }
  .held {
    margin-left: 4px;
    color: var(--warn);
  }
  .row-actions {
    display: flex;
    justify-content: flex-end;
    gap: 4px;
  }
  .note {
    margin-top: 8px;
  }
  .failed {
    margin-top: 16px;
  }
  .failed ul {
    display: grid;
    gap: 8px;
    margin: 0;
    padding: 12px 16px;
    list-style: none;
  }
  .failed li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 4px 10px;
  }
  .err {
    flex-basis: 100%;
  }
  .more {
    margin: 0 0 10px 12px;
  }
  .image {
    max-width: 280px;
    color: var(--text-2);
  }
  @media (max-width: 1000px) {
    .hide-md {
      display: none;
    }
  }
</style>
