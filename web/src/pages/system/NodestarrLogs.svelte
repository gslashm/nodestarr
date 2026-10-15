<!-- Nodestarr's own recent log (kept in memory), for troubleshooting Nodestarr itself. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage, qs } from '../../lib/api';
  import { fillHeight } from '../../lib/fill';

  interface Entry {
    ts: number;
    level: string;
    target: string;
    message: string;
  }

  let level = $state('info');
  let entries = $state<Entry[] | null>(null);
  let error = $state('');
  let follow = $state(true);
  let filter = $state('');
  let box: HTMLDivElement | undefined = $state();

  async function load() {
    try {
      entries = (await api.get<{ entries: Entry[] }>(`/system/logs${qs({ limit: 1000, level })}`)).entries;
      error = '';
      if (follow) requestAnimationFrame(() => box && (box.scrollTop = box.scrollHeight));
    } catch (err) {
      error = errorMessage(err);
    }
  }

  $effect(() => {
    void level;
    void load();
  });

  onMount(() => {
    const t = setInterval(() => {
      if (follow && !document.hidden) void load();
    }, 5000);
    return () => clearInterval(t);
  });

  const shown = $derived((entries ?? []).filter((e) => !filter || `${e.target} ${e.message}`.toLowerCase().includes(filter.toLowerCase())));

  function stamp(ms: number): string {
    return new Date(ms).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
  }

  const asText = () => shown.map((e) => `${new Date(e.ts).toISOString()} ${e.level.toUpperCase()} ${e.target}: ${e.message}`).join('\n');
</script>

<div class="logs">
  <div class="bar">
    <select class="input compact" bind:value={level} aria-label="Level">
      <option value="error">Errors</option>
      <option value="warn">Warnings and errors</option>
      <option value="info">Information and up</option>
      <option value="debug">Everything (debug)</option>
    </select>
    <input class="input compact filter" type="search" placeholder="Filter" bind:value={filter} aria-label="Filter" />
    <Toggle bind:checked={follow} label="Follow" />
    <span class="grow"></span>
    <CopyButton text={asText} label="Copy the log" showLabel />
  </div>
  {#if error}
    <p class="text-bad">{error}</p>
  {:else if !entries}
    <p class="faint"><Spinner size={13} /> Loading…</p>
  {:else}
    <div class="box" bind:this={box} use:fillHeight={{ bottom: 18, min: 300 }}>
      {#each shown as e, i (i)}
        <div class="line lvl-{e.level.toLowerCase()}">
          <span class="ts">{stamp(e.ts)}</span>
          <span class="lvl">{e.level}</span>
          <span class="tgt">{e.target.replace(/^nodestarr::/, '')}</span>
          <span class="msg">{e.message}</span>
        </div>
      {:else}
        <p class="faint empty">Nothing at this level.</p>
      {/each}
    </div>
  {/if}
</div>

<style>
  .logs {
    display: grid;
    gap: 10px;
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }
  .compact {
    width: auto;
    min-height: 32px;
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .filter {
    width: 220px;
  }
  .box {
    overflow: auto;
    padding: 8px 0;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font: 400 12.5px / 1.55 var(--font-code);
  }
  .line {
    display: grid;
    grid-template-columns: max-content 44px minmax(90px, max-content) minmax(0, 1fr);
    gap: 10px;
    padding: 0 12px;
  }
  .ts,
  .tgt {
    color: var(--text-3);
  }
  .lvl {
    text-transform: uppercase;
    color: var(--text-3);
  }
  .lvl-warn .lvl,
  .lvl-warn .msg {
    color: var(--warn);
  }
  .lvl-error .lvl,
  .lvl-error .msg {
    color: var(--bad);
  }
  .msg {
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .empty {
    padding: 8px 12px;
  }
</style>
