<!--
  Updates for one container: current state and actions, switching to another
  tag (with a preview of the exact file change), release notes and history.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import Hand from '@lucide/svelte/icons/hand';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Search from '@lucide/svelte/icons/search';
  import SkipForward from '@lucide/svelte/icons/skip-forward';
  import Undo2 from '@lucide/svelte/icons/undo-2';
  import Button from '../../components/Button.svelte';
  import DiffView from '../../components/DiffView.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import UpdateHistory from '../../components/UpdateHistory.svelte';
  import { checkNow, confirmUpdate, startUpdate } from '../../lib/actions';
  import { api, errorMessage, qs, seg } from '../../lib/api';
  import { dateTime, hasUpdate, imageTag, shortDigest, updateLabel } from '../../lib/format';
  import { renderMarkdown } from '../../lib/markdown';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { ContainerDetail, ContainerView, HistoryEntry, Release, TagInfo, TagScope, UpdateInfo } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { c }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  interface TagsResponse {
    current: string;
    channels: TagInfo[];
    versions: TagInfo[];
    total: number;
    update: UpdateInfo | null;
    is_compose: boolean;
  }

  interface Preview {
    from: string;
    to: string;
    files: { path: string; diff: string }[];
  }

  const SCOPE_TEXT: Record<TagScope, string> = {
    digest: 'new builds of the same tag',
    patch: 'patch versions',
    minor: 'minor and patch versions',
    major: 'every new version',
  };

  let tags = $state<TagsResponse | null>(null);
  let tagsError = $state('');
  let tagsLoading = $state(false);
  let tagFilter = $state('');
  let showAllTags = $state(false);
  let selected = $state<string | null>(null);
  let preview = $state<Preview | null>(null);
  let previewError = $state('');

  let notes = $state<{ repo: string | null; releases: Release[] } | null>(null);
  let notesError = $state('');
  let history = $state<HistoryEntry[] | null>(null);
  let historyError = $state('');

  const u = $derived(c.update);
  const admin = $derived(session.isAdmin);
  const currentTag = $derived(imageTag(c.image));

  async function loadTags(refresh = false) {
    tagsLoading = true;
    tagsError = '';
    try {
      tags = await api.get<TagsResponse>(`/containers/${seg(c.id)}/tags${qs({ refresh })}`);
    } catch (err) {
      tagsError = errorMessage(err);
    } finally {
      tagsLoading = false;
    }
  }

  async function loadNotes() {
    try {
      notes = await api.get(`/containers/${seg(c.id)}/releases`);
    } catch (err) {
      notesError = errorMessage(err);
    }
  }

  async function loadHistory() {
    try {
      history = await api.get<HistoryEntry[]>(`/containers/${seg(c.id)}/update-history`);
      historyError = '';
    } catch (err) {
      historyError = errorMessage(err);
    }
  }

  onMount(() => {
    void loadTags();
    void loadNotes();
    void loadHistory();
  });

  // Reload the history when a task on this container finishes.
  let wasBusy = false;
  $effect(() => {
    const busy = c.busy;
    if (wasBusy && !busy) void loadHistory();
    wasBusy = busy;
  });

  async function select(tag: string) {
    if (selected === tag) {
      selected = null;
      return;
    }
    selected = tag;
    preview = null;
    previewError = '';
    try {
      const p = await api.get<Preview>(`/containers/${seg(c.id)}/tag-preview${qs({ tag })}`);
      if (selected === tag) preview = p;
    } catch (err) {
      if (selected === tag) previewError = errorMessage(err);
    }
  }

  async function switchTo(tag: string) {
    const backup = session.settings?.updates.backup_before_update && c.prefs.backup_enabled;
    const r = await ui.ask({
      title: `Switch ${c.display_name} to ${tag}?`,
      message: [
        `The image changes from ${currentTag} to ${tag}${tags?.is_compose ? ', and the compose files are edited as shown' : ''}.`,
        backup ? 'A backup is taken first.' : '',
        'If it fails to start, Nodestarr switches back automatically.',
      ]
        .filter(Boolean)
        .join(' '),
      confirmLabel: `Switch to ${tag}`,
      options: backup ? [{ key: 'skip', label: 'Skip the pre-update backup' }] : [],
    });
    if (!r.ok) return;
    if (await startUpdate(c, tag, !!r.options.skip)) selected = null;
  }

  async function skip(digest: string | null) {
    try {
      await api.post(`/containers/${seg(c.id)}/skip`, digest === null ? {} : { digest });
      ui.toast(digest === '' ? 'This build will be offered again' : 'Skipped; you will be told when a newer build comes out', 'success');
    } catch (err) {
      ui.error(err);
    }
  }

  async function setHold(held: boolean) {
    try {
      await api.put(`/containers/${seg(c.id)}/hold`, { held });
      ui.toast(held ? `${c.display_name} won't be updated automatically until you release the hold` : 'Hold released', 'success');
    } catch (err) {
      ui.error(err);
    }
  }

  const needle = $derived(tagFilter.trim().toLowerCase());
  const channels = $derived((tags?.channels ?? []).filter((t) => !needle || t.name.toLowerCase().includes(needle)));
  const versions = $derived((tags?.versions ?? []).filter((t) => !needle || t.name.toLowerCase().includes(needle)));
  const shownVersions = $derived(showAllTags || needle ? versions.slice(0, 300) : versions.slice(0, 25));

  function norm(v: string | null | undefined): string {
    return (v ?? '').trim().toLowerCase().replace(/^v(?=\d)/, '');
  }

  function releaseMark(r: Release): string {
    const t = norm(r.tag);
    if (t && t === norm(c.version)) return 'Installed';
    if (t && (t === norm(u?.latest_version) || t === norm(u?.newer_tag?.tag))) return 'Available';
    return '';
  }

</script>

<div class="cols">
  <div class="main">
    <section class="panel status">
      <div class="panel-body">
        <div class="headline">
          {#if u?.status === 'available'}
            <span class="big text-update"><ArrowUp size={20} /> Update available</span>
            <span class="what">
              {#if u.newer_tag}
                New tag <strong>{u.newer_tag.tag}</strong> ({u.newer_tag.scope} update)
              {:else}
                New build of <strong>{currentTag}</strong>{u.latest_version ? `: ${u.latest_version}` : ''}
              {/if}
              {#if u.latest_created}<span class="faint">, published <TimeAgo ts={u.latest_created} /></span>{/if}
            </span>
          {:else if u?.status === 'pending_recreate'}
            <span class="big text-update"><ArrowUp size={20} /> Ready to apply</span>
            <span class="what">The new image is already downloaded; recreating the container applies it.</span>
          {:else if u?.status === 'up_to_date'}
            <span class="big text-ok">Up to date</span>
            <span class="what">Running the newest build of <strong>{currentTag}</strong>.</span>
          {:else if u?.status === 'error'}
            <span class="big text-bad">Couldn't check</span>
            <span class="what">{u.error}</span>
          {:else if c.policy === 'ignore' || u?.status === 'ignored'}
            <span class="big muted">Not checked</span>
            <span class="what">Updates are turned off for this container.</span>
          {:else if u?.status === 'local'}
            <span class="big muted">Local image</span>
            <span class="what">It was built on this machine, so there's no registry to compare with.</span>
          {:else if u?.status === 'pinned'}
            <span class="big muted">Pinned</span>
            <span class="what">The image is pinned to an exact digest and never changes.</span>
          {:else}
            <span class="big muted">Not checked yet</span>
            <span class="what">Check now to compare with the registry.</span>
          {/if}
        </div>

        {#if admin}
          <div class="actions">
            {#if hasUpdate(u) && c.policy !== 'ignore'}
              <Button variant="primary" icon={ArrowUp} onclick={() => confirmUpdate(c)} disabled={c.busy}>Update to {updateLabel(u)}</Button>
            {/if}
            <Button icon={Search} onclick={() => checkNow(c)}>Check now</Button>
            {#if u?.status === 'available' && !u.newer_tag && u.latest_digest}
              <Button variant="ghost" icon={SkipForward} onclick={() => skip(null)} title="Don't offer this build again">Skip this build</Button>
            {/if}
            {#if c.prefs.held}
              <Button variant="ghost" icon={Hand} onclick={() => setHold(false)}>Release hold</Button>
            {:else}
              <Button variant="ghost" icon={Hand} onclick={() => setHold(true)} title="Pause automatic updates for this container">Hold updates</Button>
            {/if}
          </div>
        {/if}

        <dl class="kv facts">
          <dt>Image</dt>
          <dd class="mono">{c.image}</dd>
          {#if c.version}<dt>Version</dt><dd class="num">{c.version}</dd>{/if}
          {#if u?.current_digest}<dt>Digest</dt><dd class="mono">{shortDigest(u.current_digest)}{#if u.latest_digest && u.latest_digest !== u.current_digest}<span class="faint"> → {shortDigest(u.latest_digest)}</span>{/if}</dd>{/if}
          <dt>Last check</dt>
          <dd>{#if u?.checked_at}<TimeAgo ts={u.checked_at} />{:else}<span class="faint">never</span>{/if}</dd>
          {#if u?.first_seen && hasUpdate(u)}<dt>Noticed</dt><dd><TimeAgo ts={u.first_seen} /></dd>{/if}
          {#if u?.source}<dt>Source</dt><dd><a href={u.source} target="_blank" rel="noopener">{u.source.replace(/^https?:\/\//, '')} <ExternalLink size={12} /></a></dd>{/if}
          <dt>Policy</dt>
          <dd>
            {#if c.policy === 'auto'}Installs {SCOPE_TEXT[c.auto_scope]} automatically{:else if c.policy === 'notify'}Tells you about updates; installs nothing on its own{:else}Ignored{/if}
            {#if c.prefs.held}<span class="chip warn-chip">On hold</span>{/if}
            <a href={href(`/containers/${encodeURIComponent(c.name)}/settings`)} class="faint">Change</a>
          </dd>
          {#if c.prefs.skipped_digest}
            <dt>Skipped</dt>
            <dd>
              Build <span class="mono">{shortDigest(c.prefs.skipped_digest)}</span>
              {#if admin}<button type="button" class="btn small ghost" onclick={() => skip('')}><Undo2 size={14} />Offer it again</button>{/if}
            </dd>
          {/if}
        </dl>
      </div>
    </section>

    <section class="panel">
      <div class="panel-head">
        <h2>Change version</h2>
        {#if tags}<span class="faint small">{tags.total.toLocaleString()} tags</span>{/if}
        <Button small variant="ghost" icon={RefreshCw} label="Reload tags" onclick={() => loadTags(true)} busy={tagsLoading} />
      </div>
      <div class="panel-body">
        <p class="muted intro">
          Running <strong class="mono">{currentTag}</strong>. Pick another tag to see exactly what changes before switching{tags?.is_compose ? ' (the compose file or its .env is edited)' : ''}.
        </p>
        <label class="search">
          <Search size={15} />
          <input class="input" type="search" placeholder="Filter tags" bind:value={tagFilter} aria-label="Filter tags" />
        </label>

        {#if tagsLoading && !tags}
          <p class="faint"><Spinner size={13} /> Asking the registry…</p>
        {:else if tagsError}
          <p class="text-bad">{tagsError}</p>
        {:else if tags}
          {#if channels.length}
            <h3 class="sub">Channels</h3>
            <div class="chips">
              {#each channels as t (t.name)}
                <button type="button" class="tag" class:current={t.name === currentTag} class:sel={t.name === selected} onclick={() => select(t.name)} title={t.updated ? `Updated ${dateTime(t.updated)}` : t.name}>
                  {t.name}
                </button>
              {/each}
            </div>
          {/if}
          {#if versions.length}
            <h3 class="sub">Versions</h3>
            <ul class="versions">
              {#each shownVersions as t (t.name)}
                <li>
                  <button type="button" class:current={t.name === currentTag} class:sel={t.name === selected} onclick={() => select(t.name)}>
                    <span class="mono name">{t.name}</span>
                    {#if t.name === currentTag}<span class="chip">current</span>{/if}
                    <span class="grow"></span>
                    {#if t.updated}<span class="faint num"><TimeAgo ts={t.updated} /></span>{/if}
                  </button>
                </li>
              {/each}
            </ul>
            {#if !showAllTags && !needle && versions.length > shownVersions.length}
              <button type="button" class="btn small ghost more" onclick={() => (showAllTags = true)}>Show all {versions.length} versions</button>
            {/if}
          {/if}
          {#if !channels.length && !versions.length}<p class="faint">No tags match.</p>{/if}
        {/if}

        {#if selected}
          <div class="preview">
            <div class="preview-head">
              <span>Switch <span class="mono">{currentTag}</span> → <strong class="mono">{selected}</strong></span>
              <span class="grow"></span>
              <button type="button" class="btn small ghost" onclick={() => (selected = null)}>Cancel</button>
              {#if admin}
                <Button small variant="primary" onclick={() => switchTo(selected!)} disabled={selected === currentTag || !!previewError || c.busy}>Switch to {selected}</Button>
              {/if}
            </div>
            {#if selected === currentTag}
              <p class="muted">This is the tag it runs now. To get its newest build, use Update or Check now.</p>
            {:else if previewError}
              <p class="text-bad">{previewError}</p>
            {:else if !preview}
              <p class="faint"><Spinner size={13} /> Working out the change…</p>
            {:else if preview.files.length}
              {#each preview.files as f (f.path)}
                <p class="faint file mono">{f.path}</p>
                <DiffView unified={f.diff} />
              {/each}
            {:else}
              <p class="muted">The container is recreated from <span class="mono">{preview.to}</span> with the same settings.</p>
            {/if}
          </div>
        {/if}
      </div>
    </section>

    <section class="panel">
      <div class="panel-head"><h2>History</h2></div>
      {#if historyError}
        <p class="panel-body text-bad">{historyError}</p>
      {:else if !history}
        <p class="panel-body faint"><Spinner size={13} /> Loading…</p>
      {:else if history.length === 0}
        <p class="panel-body muted">No updates yet. Updates made by Nodestarr show up here, and the last ones can be rolled back.</p>
      {:else}
        <UpdateHistory entries={history} />
      {/if}
    </section>
  </div>

  <section class="panel notes">
    <div class="panel-head">
      <h2>Release notes</h2>
      {#if notes?.repo}<a href="https://github.com/{notes.repo}/releases" target="_blank" rel="noopener" class="faint small">{notes.repo} <ExternalLink size={12} /></a>{/if}
    </div>
    <div class="panel-body">
      {#if notesError}
        <p class="text-bad">{notesError}</p>
      {:else if !notes}
        <p class="faint"><Spinner size={13} /> Loading…</p>
      {:else if !notes.repo}
        <p class="muted">Nodestarr doesn't know where this app publishes its releases. Images that set the <code>org.opencontainers.image.source</code> label to a GitHub repository get notes here.</p>
      {:else if notes.releases.length === 0}
        <p class="muted">No releases published on GitHub.</p>
      {:else}
        {#each notes.releases.slice(0, 12) as r, i (r.tag)}
          {@const mark = releaseMark(r)}
          <details open={i === 0 || mark === 'Available'}>
            <summary>
              <span class="rel-name">{r.name || r.tag}</span>
              {#if mark}<span class="chip mark {mark === 'Available' ? 'avail' : ''}">{mark}</span>{/if}
              {#if r.prerelease}<span class="chip">pre-release</span>{/if}
              <span class="grow"></span>
              {#if r.published_at}<span class="faint small num">{dateTime(Date.parse(r.published_at) / 1000)}</span>{/if}
            </summary>
            <div class="markdown body">{@html renderMarkdown(r.body || '_No description._')}</div>
            <a href={r.url} target="_blank" rel="noopener" class="small">Open on GitHub <ExternalLink size={12} /></a>
          </details>
        {/each}
      {/if}
    </div>
  </section>
</div>

<style>
  .cols {
    display: grid;
    grid-template-columns: minmax(0, 1.4fr) minmax(320px, 1fr);
    gap: 16px;
    align-items: start;
  }
  .main {
    display: grid;
    gap: 16px;
    min-width: 0;
  }
  .headline {
    display: grid;
    gap: 4px;
    margin-bottom: 14px;
  }
  .big {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: 600 var(--fs-xl) / 1.2 var(--font-data);
  }
  .what {
    color: var(--text-2);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 16px;
  }
  .facts {
    padding-top: 14px;
    border-top: 1px solid var(--line);
  }
  .facts dd {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .warn-chip {
    color: var(--warn);
  }
  .small {
    font-size: var(--fs-sm);
  }
  .intro {
    margin-bottom: 12px;
  }
  .search {
    position: relative;
    display: flex;
    align-items: center;
    max-width: 320px;
    margin-bottom: 14px;
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
  .sub {
    margin: 4px 0 8px;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-bottom: 14px;
  }
  .tag {
    height: 28px;
    padding: 0 10px;
    border: 1px solid var(--line-strong);
    border-radius: 14px;
    background: var(--raised);
    color: var(--text);
    font: 400 13px / 1 var(--font-code);
    cursor: pointer;
  }
  .tag:hover {
    border-color: var(--text-3);
  }
  .tag.current {
    border-color: var(--ok);
  }
  .tag.sel,
  .versions button.sel {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .versions {
    display: grid;
    gap: 2px;
    max-height: 420px;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    list-style: none;
  }
  .versions button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 34px;
    padding: 4px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .versions button:hover {
    background: var(--raised);
  }
  .versions .name {
    font-size: 13px;
  }
  .more {
    margin-top: 8px;
  }
  .preview {
    margin-top: 16px;
    padding: 12px;
    border: 1px solid var(--accent);
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--accent) 5%, var(--panel));
  }
  .preview-head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 10px;
  }
  .preview p {
    margin: 0 0 8px;
  }
  .file {
    font-size: var(--fs-sm);
  }
    .notes .panel-body {
    display: grid;
    gap: 6px;
    max-height: calc(100vh - 220px);
    overflow-y: auto;
  }
  details {
    border-bottom: 1px solid var(--line);
    padding-bottom: 6px;
  }
  details:last-child {
    border-bottom: 0;
  }
  summary {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 0;
    cursor: pointer;
    list-style: none;
  }
  summary::-webkit-details-marker {
    display: none;
  }
  summary::before {
    content: '▸';
    color: var(--text-3);
    transition: transform 120ms var(--ease);
  }
  details[open] summary::before {
    transform: rotate(90deg);
  }
  .rel-name {
    font-weight: 600;
  }
  .mark {
    color: var(--ok);
  }
  .mark.avail {
    color: var(--update);
  }
  .body {
    margin: 4px 0 8px 16px;
    font-size: var(--fs-sm);
  }
  @media (max-width: 1100px) {
    .cols {
      grid-template-columns: 1fr;
    }
    .notes .panel-body {
      max-height: none;
    }
  }
</style>
