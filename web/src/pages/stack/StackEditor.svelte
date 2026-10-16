<!--
  The stack editor tab: every service as a form, the stack's variables,
  and all its files (compose, .env, env files, Dockerfiles), saved and
  deployed together. Every save keeps the previous version under History.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Spinner from '../../components/Spinner.svelte';
  import StudioView from '../../components/studio/Studio.svelte';
  import { Studio } from '../../lib/compose/studio.svelte';
  import { live } from '../../lib/live.svelte';
  import type { StackDetail } from '../../lib/types';

  let { name }: { name: string; detail: StackDetail; reload: () => Promise<void> } = $props();

  // svelte-ignore state_referenced_locally
  const studio = new Studio(name);
  let conflict = $state(false);

  onMount(() => void studio.load());

  // Someone (or an update) changed the files: take the new ones, or warn.
  let seen = live.stacksVersion;
  $effect(() => {
    const v = live.stacksVersion;
    if (v === seen) return;
    seen = v;
    studio
      .checkServer()
      .then((r) => (conflict = r === 'conflict'))
      .catch(() => {});
  });

  async function reloadFromServer() {
    conflict = false;
    const selection = studio.selection;
    await studio.load();
    studio.selection = selection;
  }
</script>

{#if studio.loadError}
  <p class="text-bad">{studio.loadError}</p>
{:else if !studio.loaded}
  <p class="faint"><Spinner size={13} /> Loading the stack's files…</p>
{:else}
  {#if conflict}
    <div class="conflict">
      <span>The files changed on the server since you opened them. Saving now would replace those changes.</span>
      <button type="button" class="btn small" onclick={reloadFromServer}><RefreshCw size={14} />Load them (drops your edits)</button>
      <button type="button" class="btn small ghost" onclick={() => (conflict = false)}>Keep editing</button>
    </div>
  {/if}
  <StudioView {studio} mode="stack" />
{/if}

<style>
  .conflict {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    margin-bottom: 14px;
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--warn) 50%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--warn) 9%, var(--panel));
  }
  .conflict span {
    flex: 1;
    min-width: 240px;
  }
</style>
