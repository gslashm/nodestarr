<!--
  Earlier versions of a stack's files (kept on every save from Nodestarr and on
  tag switches), compared with the current file, and restorable.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import History from '@lucide/svelte/icons/history';
  import Button from '../../components/Button.svelte';
  import DiffView from '../../components/DiffView.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import { api, errorMessage, qs, seg } from '../../lib/api';
  import { actorLabel, bytes, dateTime } from '../../lib/format';
  import type { FileVersion, StackDetail, StackFile } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { name }: { name: string; detail: StackDetail; reload: () => Promise<void> } = $props();

  let files = $state<StackFile[] | null>(null);
  let file = $state('');
  let versions = $state<FileVersion[] | null>(null);
  let picked = $state<{ v: FileVersion; content: string } | null>(null);
  let error = $state('');
  let restoring = $state(false);

  const current = $derived(files?.find((f) => f.path === file) ?? null);

  async function loadFiles() {
    try {
      const r = await api.get<{ files: StackFile[] }>(`/stacks/${seg(name)}/files`);
      files = r.files;
      if (!file || !r.files.some((f) => f.path === file)) file = r.files[0]?.path ?? '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  async function loadVersions(path: string) {
    versions = null;
    picked = null;
    if (!path) return;
    try {
      const v = await api.get<FileVersion[]>(`/stacks/${seg(name)}/history${qs({ path })}`);
      if (path === file) versions = v;
    } catch (err) {
      error = errorMessage(err);
    }
  }

  onMount(loadFiles);

  $effect(() => {
    void loadVersions(file);
  });

  async function pick(v: FileVersion) {
    try {
      const r = await api.get<{ content: string }>(`/files/versions/${v.id}`);
      picked = { v, content: r.content };
    } catch (err) {
      ui.error(err);
    }
  }

  async function restore(deploy: boolean) {
    if (!picked || !current) return;
    const ok = await ui.confirm({
      title: `Restore ${current.name}?`,
      message: `It goes back to the version from ${dateTime(picked.v.ts)}. The current content is kept as a version too.${
        deploy ? ' The stack is deployed right after.' : ''
      }`,
      confirmLabel: deploy ? 'Restore and deploy' : 'Restore',
    });
    if (!ok) return;
    restoring = true;
    try {
      const r = await api.put<{ task_id: string | null }>(`/stacks/${seg(name)}/files`, {
        files: [{ path: current.path, content: picked.content }],
        deploy,
      });
      if (r.task_id) ui.taskStarted(r.task_id, `Deploying ${name}`);
      else ui.toast(`${current.name} restored`, 'success');
      await loadFiles();
      await loadVersions(file);
    } catch (err) {
      ui.error(err, "Couldn't restore it");
    } finally {
      restoring = false;
    }
  }
</script>

{#if error}
  <p class="text-bad">{error}</p>
{:else if !files}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else}
  <div class="history">
    <aside class="panel list">
      <div class="panel-head">
        <select class="input" bind:value={file} aria-label="File">
          {#each files as f (f.path)}<option value={f.path}>{f.name}</option>{/each}
        </select>
      </div>
      {#if !versions}
        <p class="pad faint"><Spinner size={13} /></p>
      {:else if versions.length === 0}
        <p class="pad muted">No earlier versions of this file yet.</p>
      {:else}
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
      {/if}
    </aside>
    <section class="panel">
      <div class="panel-head">
        <h2>{picked ? `Changes since ${dateTime(picked.v.ts)}` : 'Compare'}</h2>
        {#if picked}
          <Button small onclick={() => restore(false)} busy={restoring}>Restore</Button>
          <Button small variant="primary" onclick={() => restore(true)} busy={restoring}>Restore and deploy</Button>
        {/if}
      </div>
      <div class="panel-body">
        {#if picked && current}
          <DiffView before={picked.content} after={current.content} empty="This version is the same as the current file." />
        {:else}
          <EmptyState icon={History} title="Pick a version">It's compared with the current file, and can be put back.</EmptyState>
        {/if}
      </div>
    </section>
  </div>
{/if}

<style>
  .history {
    display: grid;
    grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
    gap: 16px;
    align-items: start;
  }
  .list .input {
    min-height: 32px;
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .pad {
    margin: 0;
    padding: 14px 16px;
  }
  ul {
    display: grid;
    gap: 2px;
    max-height: calc(100vh - 260px);
    margin: 0;
    padding: 6px;
    overflow-y: auto;
    list-style: none;
  }
  li button {
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
  li button:hover {
    background: var(--raised);
  }
  li button.active {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .small {
    font-size: var(--fs-sm);
  }
  @media (max-width: 860px) {
    .history {
      grid-template-columns: 1fr;
    }
  }
</style>
