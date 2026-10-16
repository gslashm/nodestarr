<!--
  Settings the forms don't cover (ulimits, secrets, extends, x- keys...) or
  use in ways they can't show, as YAML. Nothing in the file is hidden: any
  setting can be read, changed or added here.
-->
<script lang="ts">
  import Pencil from '@lucide/svelte/icons/pencil';
  import Plus from '@lucide/svelte/icons/plus';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import CodeEditor from '../../CodeEditor.svelte';
  import Dialog from '../../Dialog.svelte';
  import { rawKeys, SERVICE_ORDER } from '../../../lib/compose/service';
  import { useStudio } from '../../../lib/compose/studio.svelte';
  import { pairOf, pairText, parsePairText, setKey, type YAMLMap, type YDoc } from '../../../lib/compose/yamldoc';
  import { ui } from '../../../lib/ui.svelte';

  let { name, doc, svc }: { name: string; doc: YDoc; svc: YAMLMap } = $props();
  const studio = useStudio();

  const keys = $derived(rawKeys(doc, svc));
  let editing = $state<{ key: string; text: string; isNew: boolean } | null>(null);
  let error = $state('');

  function open(key: string) {
    error = '';
    editing = { key, text: pairText(doc, svc, key), isNew: false };
  }

  function openNew() {
    error = '';
    editing = { key: '', text: 'ulimits:\n  nofile:\n    soft: 65536\n    hard: 65536\n', isNew: true };
  }

  function save() {
    if (!editing) return;
    const key = editing.isNew ? (editing.text.match(/^([A-Za-z_][\w.-]*)\s*:/)?.[1] ?? '') : editing.key;
    if (!key) {
      error = 'Start with the name of the setting, like "ulimits:".';
      return;
    }
    if (editing.isNew && pairOf(svc, key)) {
      error = `${name} already has ${key}; edit that one instead.`;
      return;
    }
    const r = parsePairText(editing.text, key);
    if ('error' in r) {
      error = r.error;
      return;
    }
    try {
      studio.editService(
        name,
        (d, s) => {
          const p = pairOf(s, key);
          if (p) p.value = r.node as never;
          else setKey(d, s, key, r.node, SERVICE_ORDER);
        },
        `svc:${name}:raw:${key}`,
      );
      editing = null;
    } catch (err) {
      error = err instanceof Error ? err.message : String(err);
    }
  }

  function remove(key: string) {
    try {
      studio.editService(name, (d, s) => setKey(d, s, key, undefined), `svc:${name}:raw:${key}`);
    } catch (err) {
      ui.error(err);
    }
  }
</script>

<div class="other">
  {#if keys.length}
    <p class="muted intro">Settings the forms don't show, as written in the file.</p>
    {#each keys as key (key)}
      <div class="item">
        <div class="head">
          <span class="mono key">{key}</span>
          <span class="grow"></span>
          <button type="button" class="btn small ghost" onclick={() => open(key)}><Pencil size={14} />Edit</button>
          <button type="button" class="btn ghost icon small" aria-label="Remove {key}" onclick={() => remove(key)}><Trash2 size={14} /></button>
        </div>
        <pre class="yaml">{pairText(doc, svc, key)}</pre>
      </div>
    {/each}
  {:else}
    <p class="muted intro">Everything {name} sets is covered by the other tabs.</p>
  {/if}
  <div>
    <button type="button" class="btn small" onclick={openNew}><Plus size={15} />Add a setting as YAML</button>
    <a class="faint small" href="https://docs.docker.com/reference/compose-file/services/" target="_blank" rel="noopener">All service settings</a>
  </div>
</div>

<Dialog open={editing !== null} title={editing?.isNew ? `Add a setting to ${name}` : `Edit ${editing?.key ?? ''}`} width="640px" onclose={() => (editing = null)}>
  {#if editing}
    <CodeEditor bind:value={editing.text} language="yaml" height="260px" label="Setting as YAML" onsave={save} />
    {#if error}<p class="text-bad err">{error}</p>{:else}<p class="faint small hint">Written into the file under {name} when you apply it.</p>{/if}
  {/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => (editing = null)}>Cancel</button>
    <button type="button" class="btn primary" onclick={save}>Apply</button>
  {/snippet}
</Dialog>

<style>
  .other {
    display: grid;
    gap: 10px;
  }
  .intro {
    margin: 0;
  }
  .item {
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 8px 6px 12px;
    border-bottom: 1px solid var(--line);
  }
  .key {
    font-weight: 600;
  }
  .yaml {
    margin: 0;
    padding: 10px 12px;
    font-size: 12.5px;
    line-height: 1.5;
    white-space: pre;
    overflow-x: auto;
  }
  .small {
    margin-left: 10px;
    font-size: var(--fs-sm);
  }
  .err,
  .hint {
    margin: 8px 0 0;
  }
</style>
