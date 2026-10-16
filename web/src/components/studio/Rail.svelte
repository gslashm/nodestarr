<!--
  The stack editor's navigation: services, the stack overview and its
  variables, and every file being edited (with changed and problem marks).
-->
<script lang="ts">
  import FileCode from '@lucide/svelte/icons/file-code';
  import FilePlus from '@lucide/svelte/icons/file-plus';
  import FileText from '@lucide/svelte/icons/file-text';
  import Folder from '@lucide/svelte/icons/folder';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import Layers from '@lucide/svelte/icons/layers';
  import Plus from '@lucide/svelte/icons/plus';
  import AppIcon from '../AppIcon.svelte';
  import Dialog from '../Dialog.svelte';
  import Spinner from '../Spinner.svelte';
  import StateDot from '../StateDot.svelte';
  import { api, errorMessage, seg } from '../../lib/api';
  import { stackProblems } from '../../lib/compose/problems';
  import { get, serviceMap } from '../../lib/compose/service';
  import { fileKind, joinPath, useStudio } from '../../lib/compose/studio.svelte';
  import { appSlug } from '../../lib/compose/templates';
  import { parseYaml, toJs, type YDoc } from '../../lib/compose/yamldoc';
  import { bytes, stateOf } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import type { StackTreeEntry } from '../../lib/types';

  let { onadd }: { onadd: () => void } = $props();
  const studio = useStudio();

  const sel = $derived(studio.selection);
  const problems = $derived(stackProblems(studio.docs, studio.project, studio.usage));
  const missingVars = $derived(problems.filter((p) => p.fix?.kind === 'variable').length);
  const changedFiles = $derived(new Set(studio.changes.map((f) => f.rel)));

  // The saved files, parsed once per text.
  const savedDocs = new Map<string, YDoc>();
  function savedDoc(text: string): YDoc {
    let d = savedDocs.get(text);
    if (!d) {
      if (savedDocs.size > 8) savedDocs.clear();
      d = parseYaml(text);
      savedDocs.set(text, d);
    }
    return d;
  }

  /** Services whose definition changed since the last save. */
  const changedServices = $derived.by(() => {
    const out = new Set<string>();
    for (const { file, doc } of studio.docs) {
      const f = studio.files[file];
      if (!f || f.text === f.original) continue;
      const was = f.original ? savedDoc(f.original) : null;
      for (const name of studio.services) {
        const now = serviceMap(doc, name);
        const before = was ? serviceMap(was, name) : null;
        const a = JSON.stringify(now ? toJs(doc, now) : null);
        const b = JSON.stringify(before && was ? toJs(was, before) : null);
        if (a !== b) out.add(name);
      }
    }
    return out;
  });

  function info(name: string) {
    const doc = studio.doc(studio.serviceFile(name));
    const svc = doc ? serviceMap(doc, name) : null;
    const image = doc && svc ? String(get(doc, svc, 'image') ?? '') : '';
    const container = live.list.find((c) => c.stack === studio.name && c.service === name);
    return { image, container, slug: container?.app ?? appSlug(image) };
  }

  const fileList = $derived(
    Object.values(studio.files)
      .filter((f) => !(f.original === null && f.text === '' && !changedFiles.has(f.rel) && f.kind !== 'compose' && f.rel !== studio.envRel))
      .sort((a, b) => {
        const rank = (k: string) => ({ compose: 0, env: 1, env_file: 2, dockerfile: 3, dockerignore: 4, other: 5 })[k] ?? 6;
        return rank(a.kind) - rank(b.kind) || a.rel.localeCompare(b.rel);
      }),
  );

  // Opening other files of the stack folder.
  let picking = $state(false);
  let tree = $state<StackTreeEntry[] | null>(null);
  let treeError = $state('');
  let newPath = $state('');

  async function openPicker() {
    picking = true;
    newPath = '';
    if (studio.isNew || tree) return;
    try {
      const r = await api.get<{ entries: StackTreeEntry[] }>(`/stacks/${seg(studio.name)}/tree`);
      tree = r.entries;
    } catch (err) {
      treeError = errorMessage(err);
    }
  }

  async function pick(rel: string) {
    picking = false;
    const f = await studio.open(rel);
    studio.select({ kind: 'file', rel: f.rel });
  }

  function createFile(e: Event) {
    e.preventDefault();
    const rel = joinPath(newPath.trim());
    if (!rel || rel.startsWith('..') || rel.startsWith('/')) return;
    picking = false;
    if (!studio.files[rel]) studio.addFile(rel, '', fileKind(rel));
    studio.select({ kind: 'file', rel });
  }
</script>

<nav class="rail" aria-label="Stack editor">
  <div class="group">
    <div class="ghead">
      <span>Services</span>
      <button type="button" class="btn ghost icon small" aria-label="Add a service" title="Add a service" onclick={onadd}><Plus size={15} /></button>
    </div>
    {#each studio.services as name (name)}
      {@const i = info(name)}
      <button type="button" class="item" class:active={sel.kind === 'service' && sel.name === name} onclick={() => studio.select({ kind: 'service', name })}>
        <AppIcon {name} slug={i.slug} icon={i.container?.prefs.icon ?? ''} size={22} />
        <span class="label">{name}</span>
        {#if changedServices.has(name)}<span class="changed" title="Changed"></span>{/if}
        {#if i.container}<StateDot state={stateOf(i.container)} />{/if}
      </button>
    {:else}
      <button type="button" class="item add" onclick={onadd}><Plus size={16} /><span class="label">Add a service</span></button>
    {/each}
  </div>

  <div class="group">
    <div class="ghead"><span>Stack</span></div>
    <button type="button" class="item" class:active={sel.kind === 'stack'} onclick={() => studio.select({ kind: 'stack' })}>
      <Layers size={17} /><span class="label">Overview</span>
      {#if problems.length}<span class="badge {problems.some((p) => p.level === 'error') ? 'bad' : 'warn'}">{problems.length}</span>{/if}
    </button>
    <button type="button" class="item" class:active={sel.kind === 'variables'} onclick={() => studio.select({ kind: 'variables' })}>
      <KeyRound size={17} /><span class="label">Variables</span>
      {#if missingVars}<span class="badge warn">{missingVars}</span>{/if}
      {#if changedFiles.has(studio.envRel)}<span class="changed" title="Changed"></span>{/if}
    </button>
  </div>

  <div class="group">
    <div class="ghead">
      <span>Files</span>
      <button type="button" class="btn ghost icon small" aria-label="Open or create a file" title="Open or create a file" onclick={openPicker}><FilePlus size={15} /></button>
    </div>
    {#each fileList as f (f.rel)}
      <button type="button" class="item file" class:active={sel.kind === 'file' && sel.rel === f.rel} onclick={() => studio.select({ kind: 'file', rel: f.rel })} title={f.abs || f.rel}>
        {#if f.kind === 'compose' || f.kind === 'dockerfile'}<FileCode size={16} />{:else}<FileText size={16} />{/if}
        <span class="label mono">{f.rel}</span>
        {#if studio.yamlErrors[f.rel]}<span class="badge bad">!</span>{/if}
        {#if f.loading}<Spinner size={11} />{/if}
        {#if f.original === null && f.text !== ''}<span class="new">new</span>{:else if changedFiles.has(f.rel)}<span class="changed" title="Changed"></span>{/if}
      </button>
    {/each}
  </div>
</nav>

<Dialog open={picking} title="Open a file" width="560px" onclose={() => (picking = false)}>
  <form class="newfile" onsubmit={createFile}>
    <input class="input mono" bind:value={newPath} placeholder="config/app.conf" aria-label="Path in the stack folder" spellcheck="false" />
    <button type="submit" class="btn" disabled={!newPath.trim()}>Open or create</button>
  </form>
  <p class="faint small">Paths are relative to the stack folder{studio.dir ? ` (${studio.dir})` : ''}.</p>
  {#if !studio.isNew}
    {#if treeError}
      <p class="text-bad">{treeError}</p>
    {:else if !tree}
      <p class="faint"><Spinner size={13} /> Reading the folder…</p>
    {:else}
      <div class="tree">
        {#each tree as e (e.path)}
          {#if e.dir}
            <div class="entry dir" style:--depth={e.path.split('/').length - 1}><Folder size={14} />{e.path.split('/').pop()}</div>
          {:else}
            <button type="button" class="entry" style:--depth={e.path.split('/').length - 1} onclick={() => pick(e.path)} disabled={e.size > 2 * 1024 * 1024}>
              <FileText size={14} /><span class="mono">{e.path.split('/').pop()}</span><span class="faint">{bytes(e.size)}</span>
            </button>
          {/if}
        {/each}
      </div>
    {/if}
  {/if}
</Dialog>

<style>
  .rail {
    display: grid;
    gap: 14px;
    align-content: start;
  }
  .group {
    display: grid;
    gap: 2px;
  }
  .ghead {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 28px;
    padding: 0 4px 0 10px;
    color: var(--text-3);
    font: 600 var(--fs-xs) / 1 var(--font-data);
    letter-spacing: 0.04em;
  }
  .item {
    display: flex;
    align-items: center;
    gap: 9px;
    min-height: 36px;
    padding: 4px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius);
    background: none;
    color: var(--text-2);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .item:hover {
    background: var(--raised);
    color: var(--text);
  }
  .item.active {
    border-color: color-mix(in srgb, var(--accent) 40%, transparent);
    background: var(--accent-soft);
    color: var(--text);
  }
  .item :global(svg) {
    flex: none;
  }
  .item.add {
    border-style: dashed;
    border-color: var(--line-strong);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .file .label {
    font-size: 12.5px;
  }
  .changed {
    flex: none;
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
  }
  .new {
    flex: none;
    color: var(--accent-text);
    font-size: var(--fs-xs);
  }
  .badge {
    flex: none;
    min-width: 18px;
    height: 17px;
    padding: 0 5px;
    border-radius: 9px;
    font: 600 var(--fs-xs) / 17px var(--font-data);
    text-align: center;
  }
  .badge.warn {
    background: color-mix(in srgb, var(--warn) 18%, transparent);
    color: var(--warn);
  }
  .badge.bad {
    background: color-mix(in srgb, var(--bad) 18%, transparent);
    color: var(--bad);
  }
  .newfile {
    display: flex;
    gap: 6px;
  }
  .small {
    margin: 8px 0 12px;
    font-size: var(--fs-sm);
  }
  .tree {
    display: grid;
    max-height: 50vh;
    overflow-y: auto;
  }
  .entry {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 8px 5px calc(8px + var(--depth) * 18px);
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
  }
  .entry.dir {
    color: var(--text-2);
  }
  button.entry {
    cursor: pointer;
  }
  button.entry:hover:not(:disabled) {
    background: var(--raised);
  }
  .entry .faint {
    margin-left: auto;
  }
</style>
