<!-- A labelled on/off switch backed by a checkbox. -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    checked = $bindable(false),
    label,
    hint = '',
    disabled = false,
    onchange,
    children,
  }: { checked?: boolean; label: string; hint?: string; disabled?: boolean; onchange?: (v: boolean) => void; children?: Snippet } = $props();
</script>

<label class="toggle" class:disabled>
  <input type="checkbox" role="switch" bind:checked {disabled} onchange={() => onchange?.(checked)} />
  <span class="track" aria-hidden="true"><span class="knob"></span></span>
  <span class="text">
    <span class="label">{label}</span>
    {#if hint}<span class="hint">{hint}</span>{/if}
    {#if children}{@render children()}{/if}
  </span>
</label>

<style>
  .toggle {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    cursor: pointer;
  }
  .toggle.disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }
  input {
    position: absolute;
    opacity: 0;
    width: 1px;
    height: 1px;
  }
  .track {
    flex: none;
    position: relative;
    width: 36px;
    height: 20px;
    margin-top: 1px;
    border-radius: 10px;
    background: var(--line-strong);
    transition: background 140ms var(--ease);
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3);
    transition: transform 160ms var(--ease);
  }
  input:checked + .track {
    background: var(--accent);
  }
  input:checked + .track .knob {
    transform: translateX(16px);
  }
  input:focus-visible + .track {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .text {
    display: grid;
    gap: 2px;
  }
  .label {
    font-weight: 500;
  }
  .hint {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
</style>
