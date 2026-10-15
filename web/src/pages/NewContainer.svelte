<!--
  Add a standalone container: fill in the form or import a docker run
  command. For anything you'll keep, a compose stack is usually better, so
  the form can also be turned into a compose file.
-->
<script lang="ts">
  import FileCode from '@lucide/svelte/icons/file-code';
  import Plus from '@lucide/svelte/icons/plus';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import Wand from '@lucide/svelte/icons/wand-sparkles';
  import Button from '../components/Button.svelte';
  import CopyButton from '../components/CopyButton.svelte';
  import Dialog from '../components/Dialog.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import SpecForm from '../components/SpecForm.svelte';
  import { api } from '../lib/api';
  import { live } from '../lib/live.svelte';
  import { href, router } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import { emptySpec } from '../lib/spec';
  import type { ContainerSpec } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params: _params }: { params: Record<string, string> } = $props();

  let spec = $state<ContainerSpec>(emptySpec());
  /** Bumped when the whole spec is replaced, so the form re-reads it. */
  let formKey = $state(0);
  let runCommand = $state('');
  let warnings = $state<string[]>([]);
  let importing = $state(false);
  let start = $state(true);
  let creating = $state(false);
  let taskId = $state<string | null>(null);
  let compose = $state<string | null>(null);

  async function importRun() {
    const cmd = runCommand.trim();
    if (!cmd) return;
    importing = true;
    try {
      const r = await api.post<{ spec: ContainerSpec; warnings: string[] }>('/containers/parse-run', { command: cmd });
      spec = r.spec;
      warnings = r.warnings;
      formKey++;
      ui.toast('Imported. Check the form, then create the container.', 'success');
    } catch (err) {
      ui.error(err, "Couldn't read that command");
    } finally {
      importing = false;
    }
  }

  async function create() {
    if (creating) return;
    creating = true;
    try {
      const r = await api.post<{ task_id: string }>('/containers', { spec, start });
      taskId = r.task_id;
      ui.openTaskId = r.task_id;
    } catch (err) {
      ui.error(err, "Couldn't create it");
      creating = false;
    }
  }

  // Go to the new container once it exists.
  $effect(() => {
    const t = taskId ? live.tasks[taskId] : undefined;
    if (!t) return;
    if (t.status === 'succeeded') {
      const id = (t.result as { id?: string } | null)?.id;
      const target = spec.name || id;
      taskId = null;
      creating = false;
      ui.openTaskId = null;
      ui.toast(`${spec.name || 'Container'} created`, 'success');
      router.navigate(target ? `/containers/${encodeURIComponent(target)}` : '/containers');
    } else if (t.status === 'failed' || t.status === 'cancelled') {
      taskId = null;
      creating = false;
    }
  });

  async function showCompose() {
    try {
      const r = await api.post<{ yaml: string }>('/containers/to-compose', spec);
      compose = r.yaml;
    } catch (err) {
      ui.error(err);
    }
  }
</script>

<PageHeader title="Add a container" back={{ label: 'Containers', path: '/containers' }} />

<div class="page-body">
  {#if !session.isAdmin}
    <p class="muted">Only admins can add containers.</p>
  {:else}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        void create();
      }}
    >
      <p class="muted intro">
        Standalone containers are fine for trying things out. For apps you'll keep, a <a href={href('/stacks/new')}>compose stack</a> is easier to
        update, back up and move to another machine.
      </p>

      <details class="panel import" open={!spec.image}>
        <summary class="panel-head"><Wand size={16} /><h2>Import a docker run command</h2></summary>
        <div class="panel-body">
          <textarea
            class="input mono"
            rows="4"
            bind:value={runCommand}
            placeholder="docker run -d --name=sonarr -e PUID=1000 -e PGID=1000 -p 8989:8989 -v /opt/appdata/sonarr:/config lscr.io/linuxserver/sonarr:latest"
            spellcheck="false"
          ></textarea>
          <div class="row-end">
            <Button small icon={Wand} busy={importing} onclick={importRun} disabled={!runCommand.trim()}>Fill in the form</Button>
          </div>
          {#if warnings.length}
            <div class="warnings">
              <TriangleAlert size={16} />
              <div>
                <strong>Some options weren't imported:</strong>
                <ul>{#each warnings as w, i (i)}<li>{w}</li>{/each}</ul>
              </div>
            </div>
          {/if}
        </div>
      </details>

      {#key formKey}
        <SpecForm bind:spec />
      {/key}

      <div class="savebar">
        <label class="start"><input type="checkbox" bind:checked={start} /> Start it right away</label>
        <span class="grow"></span>
        <button type="button" class="btn ghost" onclick={showCompose} disabled={!spec.image}><FileCode size={16} />Copy as compose</button>
        <Button type="submit" variant="primary" icon={Plus} busy={creating} disabled={!spec.image.trim()}>Create container</Button>
      </div>
    </form>
  {/if}
</div>

<Dialog open={compose !== null} title="As a compose file" width="720px" onclose={() => (compose = null)}>
  <pre class="yaml">{compose}</pre>
  {#snippet footer()}
    <a class="btn ghost" href={href('/stacks/new')}>Create a stack</a>
    <CopyButton text={compose ?? ''} label="Copy" showLabel small={false} />
  {/snippet}
</Dialog>

<style>
  .form {
    display: grid;
    gap: 16px;
    max-width: 1080px;
  }
  .intro {
    margin: 0;
  }
  .import > summary {
    cursor: pointer;
    list-style: none;
    color: var(--text-2);
  }
  .import > summary::-webkit-details-marker {
    display: none;
  }
  .import:not([open]) > summary {
    border-bottom: 0;
  }
  .import .panel-body {
    display: grid;
    gap: 10px;
  }
  .row-end {
    display: flex;
    justify-content: flex-end;
  }
  .warnings {
    display: flex;
    gap: 10px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--warn) 10%, var(--panel));
  }
  .warnings :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--warn);
  }
  .warnings ul {
    margin: 4px 0 0;
    padding-left: 18px;
  }
  .savebar {
    position: sticky;
    bottom: 12px;
    z-index: 5;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
    padding: 10px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
  .start {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }
  .yaml {
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font-size: 12.5px;
    line-height: 1.5;
    white-space: pre;
    overflow: auto;
  }
</style>
