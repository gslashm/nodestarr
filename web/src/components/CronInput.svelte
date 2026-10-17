<!-- A schedule (cron) field with presets and a plain-language reading. -->
<script lang="ts">
  import { cronPresets, describeCron } from '../lib/cron';

  let {
    value = $bindable(''),
    id = undefined,
    kind = 'daily',
  }: { value: string; id?: string; kind?: 'check' | 'daily' | 'test' } = $props();

  const reading = $derived(describeCron(value));
  const presets = $derived(cronPresets(kind));
</script>

<div class="cron">
  <input {id} class="input mono" bind:value spellcheck="false" autocomplete="off" />
  <div class="meta">
    <span class="reading" class:unknown={!reading}>{reading ?? 'Custom schedule (minute hour day month weekday)'}</span>
    <span class="presets">
      {#each presets as p (p.value)}
        <button type="button" class="preset" class:active={p.value === value.trim()} onclick={() => (value = p.value)}>{p.label}</button>
      {/each}
    </span>
  </div>
</div>

<style>
  .cron {
    display: grid;
    gap: 6px;
  }
  .meta {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 12px;
  }
  .reading {
    color: var(--accent-text);
    font-size: var(--fs-sm);
  }
  .reading.unknown {
    color: var(--text-3);
  }
  .presets {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }
  .preset {
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--line);
    border-radius: 12px;
    background: none;
    color: var(--text-2);
    font: 500 var(--fs-xs) / 1 var(--font-data);
    cursor: pointer;
  }
  .preset:hover {
    border-color: var(--line-strong);
    color: var(--text);
  }
  .preset.active {
    border-color: var(--accent);
    color: var(--accent-text);
  }
</style>
