<!--
  One file of the stack in the editor that suits it: env files as a list
  of variables, Dockerfiles as instruction cards, .dockerignore as
  patterns, everything else (compose files included) as code.
-->
<script lang="ts">
  import FileCode from '@lucide/svelte/icons/file-code';
  import FileText from '@lucide/svelte/icons/file-text';
  import Undo2 from '@lucide/svelte/icons/undo-2';
  import CodeEditor from '../CodeEditor.svelte';
  import Spinner from '../Spinner.svelte';
  import DockerfileEditor from './DockerfileEditor.svelte';
  import IgnoreEditor from './IgnoreEditor.svelte';
  import VariablesEditor from './VariablesEditor.svelte';
  import { joinPath, useStudio } from '../../lib/compose/studio.svelte';
  import { fillHeight } from '../../lib/fill';

  let { rel }: { rel: string } = $props();
  const studio = useStudio();

  const f = $derived(studio.files[rel]);
  const changed = $derived(!!f && (f.original === null ? f.text !== '' : f.text !== f.original));
  const yamlError = $derived(studio.yamlErrors[rel]);

  function language(path: string): string {
    const base = path.slice(path.lastIndexOf('/') + 1).toLowerCase();
    if (/\.ya?ml$/.test(base)) return 'yaml';
    if (/\.json$/.test(base)) return 'json';
    if (/\.(xml|html?|svg)$/.test(base)) return 'xml';
    if (/\.toml$/.test(base)) return 'toml';
    if (/\.(ini|conf|cfg|properties)$/.test(base) || base.endsWith('.env')) return 'ini';
    if (/\.(sh|bash)$/.test(base)) return 'shell';
    if (/\.py$/.test(base)) return 'python';
    if (/\.(js|mjs|ts)$/.test(base)) return 'javascript';
    return 'text';
  }
</script>

{#if !f}
  <p class="faint">This file isn't open.</p>
{:else}
  <div class="file">
    <header>
      {#if f.kind === 'compose' || f.kind === 'dockerfile'}<FileCode size={18} />{:else}<FileText size={18} />{/if}
      <div class="titles">
        <h2 class="mono">{rel}</h2>
        <span class="sub">{f.abs || (studio.dir ? joinPath(studio.dir, rel) : 'In the new stack folder')}{f.original === null && !f.loading ? ', created when you save' : ''}</span>
      </div>
      <span class="grow"></span>
      {#if changed}
        <button type="button" class="btn small ghost" onclick={() => studio.revert(rel)}><Undo2 size={15} />{f.original === null ? 'Discard this file' : 'Undo changes to this file'}</button>
      {/if}
    </header>

    {#if f.loading}
      <p class="faint"><Spinner size={13} /> Loading…</p>
    {:else if f.error}
      <p class="text-bad">{f.error}</p>
    {:else if f.kind === 'env' || f.kind === 'env_file'}
      <VariablesEditor {rel} />
    {:else if f.kind === 'dockerfile'}
      <DockerfileEditor {rel} />
    {:else if f.kind === 'dockerignore'}
      <IgnoreEditor {rel} />
    {:else}
      {#if yamlError}<p class="error">{yamlError}</p>{/if}
      {#if f.kind === 'compose'}
        <p class="faint small">Edit the services in their own forms on the left, or here as YAML. Both change the same file.</p>
      {/if}
      <div class="code" use:fillHeight={{ bottom: 90, min: 360 }}>
        {#key rel}
          <CodeEditor bind:value={() => f.text, (v) => studio.setText(rel, v, `code:${rel}`)} language={language(rel)} height="100%" label="Contents of {rel}" onsave={() => studio.requestSave()} />
        {/key}
      </div>
    {/if}
  </div>
{/if}

<style>
  .file {
    display: grid;
    gap: 12px;
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  header > :global(svg) {
    flex: none;
    color: var(--text-3);
  }
  .titles {
    display: grid;
    min-width: 0;
  }
  h2 {
    font-size: var(--fs-lg);
    overflow-wrap: anywhere;
  }
  .sub {
    color: var(--text-2);
    font-size: var(--fs-sm);
    overflow-wrap: anywhere;
  }
  .small {
    margin: 0;
    font-size: var(--fs-sm);
  }
  .error {
    margin: 0;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--bad) 50%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
    font: 400 12.5px / 1.5 var(--font-code);
    white-space: pre-wrap;
  }
  .code {
    display: flex;
    flex-direction: column;
  }
</style>
