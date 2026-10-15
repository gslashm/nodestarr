<!--
  Password/token field for settings. The server never sends stored secrets
  back; it sends a placeholder instead, which this field shows as "saved".
  Sending the placeholder back keeps the stored value; typing replaces it.
-->
<script lang="ts">
  import Eye from '@lucide/svelte/icons/eye';
  import EyeOff from '@lucide/svelte/icons/eye-off';
  import { session } from '../lib/session.svelte';

  let {
    value = $bindable(''),
    id = undefined,
    placeholder = '',
    label = 'Secret',
  }: { value: string; id?: string; placeholder?: string; label?: string } = $props();

  const marker = $derived(session.secretPlaceholder || '__berth_secret__');
  const saved = $derived(value === marker);
  let wasSaved = $state(false);
  let visible = $state(false);

  $effect(() => {
    if (saved) wasSaved = true;
  });
</script>

<div class="secret">
  <input
    {id}
    class="input mono"
    type={visible && !saved ? 'text' : 'password'}
    value={saved ? '' : value}
    placeholder={saved ? 'Saved (hidden); type to replace' : placeholder}
    aria-label={label}
    autocomplete="new-password"
    spellcheck="false"
    oninput={(e) => (value = e.currentTarget.value)}
  />
  {#if !saved && value}
    <button type="button" class="btn ghost icon small" onclick={() => (visible = !visible)} aria-label={visible ? 'Hide' : 'Show'} title={visible ? 'Hide' : 'Show'}>
      {#if visible}<EyeOff size={15} />{:else}<Eye size={15} />{/if}
    </button>
  {/if}
  {#if saved}
    <button type="button" class="btn ghost small" onclick={() => (value = '')}>Remove</button>
  {:else if wasSaved}
    <button type="button" class="btn ghost small" onclick={() => (value = marker)}>Keep saved</button>
  {/if}
</div>

<style>
  .secret {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .secret .input {
    flex: 1;
    min-width: 0;
  }
</style>
