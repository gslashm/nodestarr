<!--
  Compose editor for a stack: its compose files and .env side by side in
  tabs, with validation (docker compose config), a review of the changes,
  and save or save-and-deploy. Every save keeps the previous version.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import FileDiff from '@lucide/svelte/icons/file-diff';
  import Rocket from '@lucide/svelte/icons/rocket';
  import Save from '@lucide/svelte/icons/save';
  import Button from '../../components/Button.svelte';
  import CodeEditor from '../../components/CodeEditor.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import DiffView from '../../components/DiffView.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage, seg } from '../../lib/api';
  import { fillHeight } from '../../lib/fill';
  import type { StackDetail, StackFile } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { name, reload }: { name: string; detail: StackDetail; reload: () => Promise<void> } = $props();

  let files = $state<StackFile[] | null>(null);
  let edits = $state<Record<string, string>>({});
  let active = $state('');
  let loadError = $state('');
  let validation = $state<{ ok: boolean; message?: string } | null>(null);
  let validating = $state(false);
  let saving = $state(false);
  let diffs = $state<{ path: string; diff: string }[] | null>(null);

  const changed = $derived((files ?? []).filter((f) => edits[f.path] !== undefined && edits[f.path] !== f.content));
  const current = $derived(files?.find((f) => f.path === active) ?? null);

  async function load() {
    try {
      const r = await api.get<{ files: StackFile[]; dir: string }>(`/stacks/${seg(name)}/files`);
      files = r.files;
      edits = Object.fromEntries(r.files.map((f) => [f.path, f.content]));
      if (!r.files.some((f) => f.path === active)) active = r.files[0]?.path ?? '';
      loadError = '';
    } catch (err) {
      loadError = errorMessage(err);
    }
  }

  onMount(load);

  function payload() {
    return changed.map((f) => ({ path: f.path, content: edits[f.path] }));
  }

  // Validation results go stale as soon as anything changes.
  $effect(() => {
    void JSON.stringify(edits);
    validation = null;
  });

  async function validate() {
    validating = true;
    try {
      validation = await api.post<{ ok: boolean; message?: string }>(`/stacks/${seg(name)}/validate`, { files: payload() });
    } catch (err) {
      validation = { ok: false, message: errorMessage(err) };
    } finally {
      validating = false;
    }
  }

  async function review() {
    try {
      const r = await api.post<{ files: { path: string; diff: string }[] }>(`/stacks/${seg(name)}/diff`, { files: payload() });
      diffs = r.files;
    } catch (err) {
      ui.error(err);
    }
  }

  async function save(deploy: boolean) {
    if (saving || !changed.length) return;
    saving = true;
    try {
      const r = await api.put<{ task_id: string | null }>(`/stacks/${seg(name)}/files`, { files: payload(), deploy });
      diffs = null;
      await load();
      void reload();
      if (r.task_id) ui.taskStarted(r.task_id, `Deploying ${name}`);
      else ui.toast('Saved. Deploy the stack to apply the changes.', 'success');
    } catch (err) {
      // Invalid files come back as a 400 with compose's message.
      validation = { ok: false, message: errorMessage(err) };
      ui.toast("Not saved: the compose files aren't valid", 'error');
    } finally {
      saving = false;
    }
  }

  function revertActive() {
    if (current) edits[current.path] = current.content;
  }

  function onKey(e: KeyboardEvent) {
    if ((e.metaKey || e.ctrlKey) && e.key === 's') {
      e.preventDefault();
      void save(false);
    }
  }

  function onBeforeUnload(e: BeforeUnloadEvent) {
    if (changed.length) e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} onbeforeunload={onBeforeUnload} />

{#if loadError}
  <p class="text-bad">{loadError}</p>
{:else if !files}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else}
  <div class="compose">
    <div class="bar">
      <div class="file-tabs" role="tablist" aria-label="Files">
        {#each files as f (f.path)}
          <button type="button" role="tab" aria-selected={f.path === active} class:active={f.path === active} onclick={() => (active = f.path)} title={f.path}>
            <span class="mono">{f.name}</span>
            {#if edits[f.path] !== f.content}<span class="dot" aria-label="changed"></span>{/if}
            {#if !f.exists}<span class="faint new">new</span>{/if}
          </button>
        {/each}
      </div>
      <span class="grow"></span>
      {#if current && edits[current.path] !== current.content}
        <button type="button" class="btn small ghost" onclick={revertActive}>Undo changes to {current.name}</button>
      {/if}
      <Button small variant="ghost" icon={CircleCheck} busy={validating} onclick={validate}>Validate</Button>
      <Button small variant="ghost" icon={FileDiff} onclick={review} disabled={!changed.length}>Review changes</Button>
      <Button small icon={Save} busy={saving} onclick={() => save(false)} disabled={!changed.length}>Save</Button>
      <Button small variant="primary" icon={Rocket} busy={saving} onclick={() => save(true)} disabled={!changed.length}>Save and deploy</Button>
    </div>

    {#if validation}
      <div class="validation" class:ok={validation.ok}>
        {#if validation.ok}
          <CircleCheck size={16} /> The compose files are valid.
        {:else}
          <pre>{validation.message}</pre>
        {/if}
      </div>
    {/if}

    {#if current}
      {#if !current.exists}
        <p class="faint small">{current.name} doesn't exist yet. Anything you write here is saved as a new file.</p>
      {/if}
      <div class="editor" use:fillHeight={{ bottom: 44, min: 360 }}>
        {#key current.path}
          <CodeEditor
            bind:value={edits[current.path]}
            language={current.kind === 'env' ? 'ini' : 'yaml'}
            height="100%"
            label="Contents of {current.name}"
            onsave={() => save(false)}
          />
        {/key}
      </div>
      <p class="faint small hint">Ctrl+S saves. Saving checks the files with <code>docker compose config</code> first, and keeps the previous version under History.</p>
    {/if}
  </div>
{/if}

<Dialog open={diffs !== null} title="Changes to {name}" width="900px" onclose={() => (diffs = null)}>
  {#if diffs?.length}
    {#each diffs as d (d.path)}
      <p class="mono small path">{d.path}</p>
      <DiffView unified={d.diff} />
    {/each}
  {:else}
    <p class="muted">Nothing changed.</p>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (diffs = null)}>Keep editing</button>
    <Button icon={Save} busy={saving} onclick={() => save(false)} disabled={!changed.length}>Save</Button>
    <Button variant="primary" icon={Rocket} busy={saving} onclick={() => save(true)} disabled={!changed.length}>Save and deploy</Button>
  {/snippet}
</Dialog>

<style>
  .compose {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .file-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .file-tabs button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--panel);
    color: var(--text-2);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .file-tabs button.active {
    border-color: var(--accent);
    background: var(--accent-soft);
    color: var(--text);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  .new {
    font-size: var(--fs-xs);
  }
  .validation {
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--bad) 50%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
  }
  .validation pre {
    color: var(--text);
    font-size: 12.5px;
  }
  .validation.ok {
    display: flex;
    align-items: center;
    gap: 8px;
    border-color: color-mix(in srgb, var(--ok) 50%, var(--line));
    background: color-mix(in srgb, var(--ok) 8%, var(--panel));
    color: var(--ok);
  }
  .editor {
    display: flex;
    flex-direction: column;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .hint {
    margin: 0;
  }
  .path {
    margin: 12px 0 6px;
  }
  .path:first-child {
    margin-top: 0;
  }
</style>
