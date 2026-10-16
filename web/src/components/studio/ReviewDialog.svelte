<!-- Every changed file of the stack editor, as a diff, before saving. -->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import Dialog from '../Dialog.svelte';
  import DiffView from '../DiffView.svelte';
  import { useStudio } from '../../lib/compose/studio.svelte';

  let { open, onclose, footer }: { open: boolean; onclose: () => void; footer: Snippet } = $props();
  const studio = useStudio();
</script>

<Dialog {open} title="Changes to {studio.name || 'the new stack'}" width="920px" {onclose} {footer}>
  {#if studio.changes.length}
    {#each studio.changes as f (f.rel)}
      <div class="file">
        <span class="mono path">{f.rel}</span>
        {#if f.original === null}<span class="chip new">new file</span>{/if}
        <button type="button" class="btn small ghost" onclick={() => studio.revert(f.rel)}>{f.original === null ? 'Leave it out' : 'Undo these changes'}</button>
      </div>
      <DiffView before={f.original ?? ''} after={f.text} />
    {/each}
  {:else}
    <p class="muted">Nothing changed.</p>
  {/if}
</Dialog>

<style>
  .file {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 16px 0 6px;
  }
  .file:first-child {
    margin-top: 0;
  }
  .path {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .new {
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
    color: var(--accent-text);
  }
</style>
