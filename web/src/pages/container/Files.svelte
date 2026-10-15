<!--
  Config file editor: browse the container's mounted folders, edit a file
  with syntax highlighting, save (optionally restarting the app), and look
  at or restore earlier versions Nodestarr kept.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import CornerLeftUp from '@lucide/svelte/icons/corner-left-up';
  import FileIcon from '@lucide/svelte/icons/file';
  import FileSymlink from '@lucide/svelte/icons/file-symlink';
  import Folder from '@lucide/svelte/icons/folder';
  import FolderX from '@lucide/svelte/icons/folder-x';
  import History from '@lucide/svelte/icons/history';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import Save from '@lucide/svelte/icons/save';
  import Button from '../../components/Button.svelte';
  import CodeEditor from '../../components/CodeEditor.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import DiffView from '../../components/DiffView.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Menu from '../../components/Menu.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { ApiError, api, errorMessage, qs, seg } from '../../lib/api';
  import { fillHeight } from '../../lib/fill';
  import { actorLabel, bytes, dateTime, isUp, stateOf } from '../../lib/format';
  import { router } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { ContainerDetail, ContainerView, FileVersion } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { c }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  interface Root {
    destination: string;
    source: string;
    accessible: boolean;
    data: boolean;
    read_only: boolean;
  }

  interface Entry {
    name: string;
    kind: 'file' | 'dir' | 'link';
    size: number;
    modified: number | null;
  }

  interface FileContent {
    path: string;
    content: string | null;
    size: number;
    modified: number | null;
    etag: string;
    binary: boolean;
    too_large: boolean;
    language: string;
  }

  type Apply = 'none' | 'restart' | 'stop_start';

  let roots = $state<Root[] | null>(null);
  let rootsError = $state('');
  let root = $state('');
  let dir = $state('');
  let entries = $state<Entry[] | null>(null);
  let entriesError = $state('');
  let listFilter = $state('');

  let file = $state<string | null>(null);
  let loaded = $state<FileContent | null>(null);
  let fileError = $state('');
  let fileLoading = $state(false);
  let text = $state('');
  let original = $state('');
  let saving = $state(false);

  let versionsOpen = $state(false);
  let versions = $state<FileVersion[] | null>(null);
  let versionsError = $state('');
  let picked = $state<{ v: FileVersion; content: string } | null>(null);

  const current = $derived(roots?.find((r) => r.destination === root) ?? null);
  const dirty = $derived(!!loaded && text !== original);
  const readOnly = $derived(!!current?.read_only || !session.isAdmin);
  const running = $derived(isUp(stateOf(c)));
  const crumbs = $derived(dir ? dir.split('/') : []);
  const shownEntries = $derived((entries ?? []).filter((e) => !listFilter || e.name.toLowerCase().includes(listFilter.toLowerCase())));

  function join(a: string, b: string): string {
    return a ? `${a}/${b}` : b;
  }

  function parent(p: string): string {
    const i = p.lastIndexOf('/');
    return i < 0 ? '' : p.slice(0, i);
  }

  function syncQuery() {
    router.setQuery({ root: root || null, dir: dir || null, file });
  }

  async function confirmDiscard(): Promise<boolean> {
    if (!dirty) return true;
    return ui.confirm({
      title: 'Discard your changes?',
      message: `${file} has changes that aren't saved.`,
      confirmLabel: 'Discard',
      danger: true,
    });
  }

  async function loadRoots() {
    try {
      roots = await api.get<Root[]>(`/containers/${seg(c.id)}/files/roots`);
    } catch (err) {
      rootsError = errorMessage(err);
      return;
    }
    const q = router.query;
    const wanted = roots.find((r) => r.destination === q.get('root'));
    const first = wanted ?? roots.find((r) => r.accessible && !r.data) ?? roots.find((r) => r.accessible) ?? roots[0];
    if (!first) return;
    root = first.destination;
    if (wanted) {
      dir = q.get('dir') ?? '';
      const f = q.get('file');
      void openDir(dir, false);
      if (f) void openFile(f, false);
    } else {
      void openDir('', false);
    }
  }

  async function openDir(path: string, sync = true) {
    dir = path;
    entries = null;
    entriesError = '';
    listFilter = '';
    if (sync) syncQuery();
    if (!current?.accessible) return;
    const want = { root, path };
    try {
      const list = await api.get<Entry[]>(`/containers/${seg(c.id)}/files${qs(want)}`);
      if (root === want.root && dir === want.path) entries = list;
    } catch (err) {
      if (root === want.root && dir === want.path) entriesError = errorMessage(err);
    }
  }

  async function chooseRoot(dest: string) {
    if (dest === root) return;
    if (!(await confirmDiscard())) return;
    root = dest;
    closeFile();
    await openDir('');
  }

  function closeFile() {
    file = null;
    loaded = null;
    text = '';
    original = '';
    fileError = '';
  }

  /** Opens a file; `force` reloads it even when it's already open. */
  async function openFile(path: string, sync = true, force = false) {
    if (path === file && loaded && !force) return;
    if (!force && !(await confirmDiscard())) return;
    file = path;
    loaded = null;
    fileError = '';
    fileLoading = true;
    if (sync) syncQuery();
    try {
      const f = await api.get<FileContent>(`/containers/${seg(c.id)}/files/content${qs({ root, path })}`);
      if (file !== path) return;
      loaded = f;
      text = f.content ?? '';
      original = text;
    } catch (err) {
      if (file === path) fileError = errorMessage(err);
    } finally {
      fileLoading = false;
    }
  }

  function onEntry(e: Entry) {
    if (e.kind === 'dir') void openDir(join(dir, e.name));
    else void openFile(join(dir, e.name));
  }

  /** Saves the editor content. `force` skips the changed-on-disk check. */
  async function save(apply: Apply = 'none', force = false) {
    if (!loaded || !file || saving || readOnly) return;
    if (apply !== 'none' && running && !force) {
      const ok = await ui.confirm({
        title: apply === 'restart' ? `Save and restart ${c.display_name}?` : `Stop ${c.display_name}, save, and start it?`,
        message:
          apply === 'restart'
            ? 'The app restarts right after saving, so the change takes effect.'
            : 'The app is stopped first, so it cannot overwrite the file while shutting down, then started again.',
        confirmLabel: apply === 'restart' ? 'Save and restart' : 'Stop, save, start',
      });
      if (!ok) return;
    }
    saving = true;
    const path = file;
    try {
      const f = await api.put<FileContent>(`/containers/${seg(c.id)}/files/content`, {
        root,
        path,
        content: text,
        etag: force ? null : loaded.etag,
        apply,
      });
      if (file === path) {
        loaded = f;
        original = text;
      }
      ui.toast(apply === 'none' || !running ? `Saved ${path}` : `Saved ${path} and restarted ${c.display_name}`, 'success');
    } catch (err) {
      if (err instanceof ApiError && err.status === 409 && !force) {
        const overwrite = await ui.confirm({
          title: 'The file changed on disk',
          message: 'It was saved by something else (often the app itself) after you opened it. Replace it with your version?',
          confirmLabel: 'Replace with mine',
          cancelLabel: 'Keep editing',
          danger: true,
        });
        if (overwrite) {
          saving = false;
          await save(apply, true);
          return;
        }
      } else {
        ui.error(err, "Couldn't save");
      }
    } finally {
      saving = false;
    }
  }

  async function revert() {
    if (!dirty) return;
    const ok = await ui.confirm({ title: 'Undo all changes?', message: 'The editor goes back to the saved file.', confirmLabel: 'Undo changes' });
    if (ok) text = original;
  }

  async function reloadFile() {
    if (file && (await confirmDiscard())) await openFile(file, false, true);
  }

  async function showVersions() {
    if (!file) return;
    versionsOpen = true;
    versions = null;
    picked = null;
    versionsError = '';
    try {
      versions = await api.get<FileVersion[]>(`/containers/${seg(c.id)}/files/versions${qs({ root, path: file })}`);
    } catch (err) {
      versionsError = errorMessage(err);
    }
  }

  async function pick(v: FileVersion) {
    try {
      const r = await api.get<{ path: string; content: string }>(`/files/versions/${v.id}`);
      picked = { v, content: r.content };
    } catch (err) {
      ui.error(err);
    }
  }

  async function restoreVersion() {
    if (!picked || !file) return;
    const ok = await ui.confirm({
      title: 'Restore this version?',
      message: `${file} goes back to how it was on ${dateTime(picked.v.ts)}. The current content is kept as a version too.`,
      confirmLabel: 'Restore',
    });
    if (!ok) return;
    try {
      const f = await api.post<FileContent>(`/containers/${seg(c.id)}/files/restore`, { root, path: file, version_id: picked.v.id });
      loaded = f;
      text = f.content ?? '';
      original = text;
      versionsOpen = false;
      ui.toast('Version restored. Restart the app if it only reads its config at startup.', 'success');
    } catch (err) {
      ui.error(err, "Couldn't restore it");
    }
  }

  function onKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 's' && loaded) {
      e.preventDefault();
      void save();
    }
  }

  function onBeforeUnload(e: BeforeUnloadEvent) {
    if (dirty) e.preventDefault();
  }

  onMount(() => {
    void loadRoots();
  });
</script>

<svelte:window onkeydown={onKey} onbeforeunload={onBeforeUnload} />

{#if rootsError}
  <p class="text-bad">{rootsError}</p>
{:else if !roots}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else if roots.length === 0}
  <EmptyState icon={FolderX} title="No folders to browse">
    {c.display_name} has no folders mounted from the host. Files inside named volumes or the image itself can be changed from the Console.
  </EmptyState>
{:else}
  <div class="files">
    <aside class="browser panel">
      <div class="roots">
        <select class="input" value={root} onchange={(e) => chooseRoot(e.currentTarget.value)} aria-label="Folder">
          {#each roots as r (r.destination)}
            <option value={r.destination}>{r.destination}{r.data ? ' (data)' : ''}{r.read_only ? ' (read-only)' : ''}</option>
          {/each}
        </select>
        {#if current}<p class="faint src ellipsis" title={current.source}>{current.source}</p>{/if}
      </div>
      {#if current && !current.accessible}
        <div class="hint">
          <p><strong>Nodestarr can't see this folder.</strong></p>
          <p class="muted">Mount <code>{current.source}</code> into the Nodestarr container at the same path, for example <code>{current.source}:{current.source}</code>.</p>
        </div>
      {:else}
        <nav class="crumbs" aria-label="Path">
          <button type="button" class="crumb" onclick={() => openDir('')}>{root}</button>
          {#each crumbs as part, i (i)}
            <span class="faint">/</span>
            <button type="button" class="crumb" onclick={() => openDir(crumbs.slice(0, i + 1).join('/'))}>{part}</button>
          {/each}
        </nav>
        {#if current?.data}<p class="warn small">This looks like media or bulk data, not configuration. Large folders can be slow to list.</p>{/if}
        {#if entries && entries.length > 15}
          <input class="input filter" type="search" placeholder="Filter" bind:value={listFilter} aria-label="Filter files" />
        {/if}
        <ul class="entries">
          {#if dir}
            <li><button type="button" onclick={() => openDir(parent(dir))}><CornerLeftUp size={15} /> <span>..</span></button></li>
          {/if}
          {#if entriesError}
            <li class="text-bad msg">{entriesError}</li>
          {:else if !entries}
            <li class="faint msg"><Spinner size={13} /> Loading…</li>
          {:else}
            {#each shownEntries as e (e.name)}
              <li>
                <button type="button" class:active={file === join(dir, e.name)} class:dir={e.kind === 'dir'} onclick={() => onEntry(e)} title={e.modified ? `Changed ${dateTime(e.modified)}` : e.name}>
                  {#if e.kind === 'dir'}<Folder size={15} />{:else if e.kind === 'link'}<FileSymlink size={15} />{:else}<FileIcon size={15} />{/if}
                  <span class="name">{e.name}</span>
                  {#if e.kind === 'file'}<span class="size num faint">{bytes(e.size, 0)}</span>{/if}
                </button>
              </li>
            {:else}
              <li class="faint msg">{listFilter ? 'Nothing matches.' : 'Empty folder.'}</li>
            {/each}
          {/if}
        </ul>
      {/if}
    </aside>

    <section class="editor-pane">
      {#if !file}
        <div class="placeholder panel">
          <EmptyState icon={FileIcon} title="Pick a file to edit">
            Nodestarr keeps the previous version every time you save, so changes can be undone.
          </EmptyState>
        </div>
      {:else}
        <div class="bar">
          <span class="path mono ellipsis" title={loaded?.path ?? file}>{file}</span>
          {#if dirty}<span class="chip unsaved">Unsaved</span>{/if}
          {#if loaded?.modified}<span class="faint small">saved <TimeAgo ts={loaded.modified} /></span>{/if}
          <span class="grow"></span>
          <Button small variant="ghost" icon={History} onclick={showVersions}>History</Button>
          <Button small variant="ghost" icon={RotateCcw} label="Reload from disk" onclick={reloadFile} />
          {#if !readOnly && loaded && loaded.content !== null}
            {#if dirty}<button type="button" class="btn small ghost" onclick={revert}>Undo changes</button>{/if}
            <div class="split">
              <Button small variant="primary" icon={Save} busy={saving} disabled={!dirty} onclick={() => save('none')}>Save</Button>
              <Menu
                items={[
                  { label: 'Save and restart', hint: running ? '' : 'not running', run: () => save('restart'), disabled: !dirty },
                  { label: 'Stop, save, then start', run: () => save('stop_start'), disabled: !dirty },
                ]}
                label="More ways to save"
              >
                {#snippet trigger()}
                  <span class="btn small primary caret" aria-hidden="true"><ChevronDown size={15} /></span>
                {/snippet}
              </Menu>
            </div>
          {/if}
        </div>
        {#if readOnly && session.isAdmin}<p class="faint small note">This folder is mounted read-only, so the file can't be saved here.</p>{/if}

        {#if fileError}
          <p class="text-bad">{fileError}</p>
        {:else if fileLoading || !loaded}
          <p class="faint"><Spinner size={13} /> Opening…</p>
        {:else if loaded.binary}
          <div class="panel"><EmptyState icon={FileIcon} title="This is a binary file">It can't be edited as text ({bytes(loaded.size)}).</EmptyState></div>
        {:else if loaded.too_large}
          <div class="panel"><EmptyState icon={FileIcon} title="Too large to edit here">It's {bytes(loaded.size)}. Use the Console for files this big.</EmptyState></div>
        {:else}
          <div use:fillHeight={{ bottom: 44, min: 360 }} class="edit">
            {#key `${root}:${file}`}
              <CodeEditor bind:value={text} language={loaded.language} readonly={readOnly} height="100%" label="Contents of {file}" onsave={() => save()} />
            {/key}
          </div>
          <p class="faint small note">Ctrl+S saves. Some apps rewrite their config when they stop; for those, use “Stop, save, then start”.</p>
        {/if}
      {/if}
    </section>
  </div>
{/if}

<Dialog open={versionsOpen} title="Earlier versions of {file ?? ''}" width="860px" drawer onclose={() => (versionsOpen = false)}>
  {#if versionsError}
    <p class="text-bad">{versionsError}</p>
  {:else if !versions}
    <p class="faint"><Spinner size={13} /> Loading…</p>
  {:else if versions.length === 0}
    <p class="muted">No earlier versions yet. Nodestarr keeps one each time the file is saved from here, from the compose editor, or by an update.</p>
  {:else}
    <div class="versions">
      <ul>
        {#each versions as v (v.id)}
          <li>
            <button type="button" class:active={picked?.v.id === v.id} onclick={() => pick(v)}>
              <span class="num">{dateTime(v.ts)}</span>
              <span class="faint small">{actorLabel(v.actor)}{v.note ? `, ${v.note}` : ''}, {bytes(v.size)}</span>
            </button>
          </li>
        {/each}
      </ul>
      <div class="vdiff">
        {#if picked}
          <p class="muted small">Changes from this version to what's in the editor now:</p>
          <DiffView before={picked.content} after={text} empty="Same as the editor content." />
        {:else}
          <p class="faint">Pick a version to compare it with the current content.</p>
        {/if}
      </div>
    </div>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (versionsOpen = false)}>Close</button>
    {#if session.isAdmin && !current?.read_only}
      <button type="button" class="btn primary" disabled={!picked} onclick={restoreVersion}>Restore this version</button>
    {/if}
  {/snippet}
</Dialog>

<style>
  .files {
    display: grid;
    grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
    gap: 16px;
    align-items: start;
  }
  .browser {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 210px);
    min-height: 300px;
    overflow: hidden;
  }
  .roots {
    padding: 10px 10px 6px;
  }
  .roots .input {
    min-height: 32px;
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .src {
    margin: 4px 2px 0;
    font-size: var(--fs-xs);
  }
  .hint {
    padding: 8px 12px;
  }
  .hint p {
    margin-bottom: 6px;
  }
  .crumbs {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 2px;
    padding: 4px 10px 8px;
    border-bottom: 1px solid var(--line);
  }
  .crumb {
    padding: 1px 4px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--accent-text);
    font: 400 12.5px / 1.4 var(--font-code);
    cursor: pointer;
  }
  .crumb:hover {
    background: var(--raised);
  }
  .warn {
    margin: 8px 10px 0;
    color: var(--warn);
  }
  .filter {
    width: auto;
    min-height: 30px;
    margin: 8px 10px 0;
    padding: 3px 8px;
  }
  .entries {
    flex: 1;
    margin: 0;
    padding: 6px;
    overflow-y: auto;
    list-style: none;
  }
  .entries button {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 5px 8px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: 400 var(--fs-sm) / 1.3 var(--font-ui);
    text-align: left;
    cursor: pointer;
  }
  .entries button :global(svg) {
    flex: none;
    color: var(--text-3);
  }
  .entries button.dir :global(svg) {
    color: var(--accent-text);
  }
  .entries button:hover {
    background: var(--raised);
  }
  .entries button.active {
    background: var(--accent-soft);
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .size {
    font-size: var(--fs-xs);
  }
  .msg {
    padding: 8px;
    font-size: var(--fs-sm);
  }
  .editor-pane {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }
  .placeholder {
    min-height: 300px;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .path {
    max-width: 50%;
    font-size: 13.5px;
  }
  .unsaved {
    color: var(--warn);
  }
  .small {
    font-size: var(--fs-sm);
  }
  .note {
    margin: 0;
  }
  .split {
    display: inline-flex;
    align-items: center;
  }
  .split > :global(.btn) {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }
  .caret {
    width: 26px;
    padding: 0;
    border-top-left-radius: 0;
    border-bottom-left-radius: 0;
    border-left: 1px solid color-mix(in srgb, var(--accent-ink) 25%, transparent);
  }
  .edit {
    display: flex;
    flex-direction: column;
  }
  .versions {
    display: grid;
    grid-template-columns: 240px minmax(0, 1fr);
    gap: 16px;
    align-items: start;
  }
  .versions ul {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .versions li button {
    display: grid;
    gap: 2px;
    width: 100%;
    padding: 7px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .versions li button:hover {
    background: var(--raised);
  }
  .versions li button.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .vdiff {
    min-width: 0;
  }
  @media (max-width: 860px) {
    .files {
      grid-template-columns: 1fr;
    }
    .browser {
      max-height: 320px;
    }
    .path {
      max-width: 100%;
    }
    .versions {
      grid-template-columns: 1fr;
    }
  }
</style>
