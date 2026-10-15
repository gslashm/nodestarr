<!--
  One container drawn as a container: a corrugated block with a status band,
  its name and version, live usage, and markers for updates, failures and
  running tasks.
-->
<script lang="ts">
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import Globe from '@lucide/svelte/icons/globe';
  import { bytes, hasUpdate, isUp, STATE_LABEL, stateColor, stateOf, updateLabel } from '../lib/format';
  import { href } from '../lib/router.svelte';
  import type { ContainerView } from '../lib/types';
  import AppIcon from './AppIcon.svelte';
  import Spinner from './Spinner.svelte';

  let { c }: { c: ContainerView } = $props();

  const st = $derived(stateOf(c));
  const up = $derived(isUp(st));
  const update = $derived(hasUpdate(c.update) && c.policy !== 'ignore');
  const failing = $derived(st === 'unhealthy' || st === 'crashed' || st === 'restarting' || st === 'dead');
  const webDown = $derived(c.uptime?.state === 'down');
  const appProblem = $derived(!!c.app_status?.health.some((h) => h.level === 'error'));

  const tooltip = $derived(
    [
      `${c.display_name}: ${STATE_LABEL[st]}`,
      c.version ? `Version ${c.version}` : '',
      update ? `Update available: ${updateLabel(c.update)}` : '',
      webDown ? 'Web UI is not answering' : '',
      c.stats ? `CPU ${c.stats.cpu.toFixed(1)}%, memory ${bytes(c.stats.mem)}` : '',
    ]
      .filter(Boolean)
      .join('\n'),
  );
</script>

<a class="tile" class:down={!up} class:failing href={href(`/containers/${encodeURIComponent(c.name)}`)} style:--state={stateColor(st)} title={tooltip}>
  <span class="band" aria-hidden="true"></span>
  <span class="head">
    <AppIcon name={c.display_name} slug={c.app} icon={c.prefs.icon} size={20} />
    <span class="name ellipsis">{c.display_name}</span>
    <span class="marks">
      {#if c.busy}<Spinner size={13} label="Task running" />{/if}
      {#if failing || appProblem}<span class="mark bad" title={failing ? STATE_LABEL[st] : 'The app reports a problem'}><CircleAlert size={14} /></span>{/if}
      {#if webDown}<span class="mark bad" title="Web UI is not answering"><Globe size={14} /></span>{/if}
      {#if update}<span class="mark update" title="Update available: {updateLabel(c.update)}"><ArrowUp size={14} strokeWidth={2.6} /></span>{/if}
    </span>
  </span>
  <span class="foot num">
    <span class="ver ellipsis">{up ? (c.version ?? c.image.split(':').pop()) : STATE_LABEL[st]}</span>
    {#if up && c.stats}
      <span class="use">{c.stats.cpu.toFixed(c.stats.cpu < 10 ? 1 : 0)}%</span>
      <span class="use">{bytes(c.stats.mem, 0)}</span>
    {/if}
  </span>
</a>

<style>
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    gap: 8px;
    width: 188px;
    height: 74px;
    padding: 11px 11px 9px 12px;
    border: 1px solid var(--line);
    border-radius: 4px;
    background:
      repeating-linear-gradient(90deg, var(--corrugation) 0 2px, transparent 2px 9px),
      var(--raised);
    color: var(--text);
    text-decoration: none;
    overflow: hidden;
    transition:
      border-color 120ms var(--ease),
      transform 120ms var(--ease);
  }
  .tile:hover {
    border-color: var(--line-strong);
    text-decoration: none;
    transform: translateY(-1px);
  }
  .band {
    position: absolute;
    inset: 0 0 auto 0;
    height: 3px;
    background: var(--state);
  }
  .tile.down {
    background:
      repeating-linear-gradient(90deg, var(--corrugation) 0 2px, transparent 2px 9px),
      color-mix(in srgb, var(--raised) 55%, var(--bg));
    border-style: dashed;
  }
  .tile.down .name,
  .tile.down :global(.mono-icon),
  .tile.down :global(img) {
    opacity: 0.6;
  }
  .tile.failing {
    border-color: color-mix(in srgb, var(--bad) 55%, var(--line));
    border-style: solid;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .name {
    flex: 1;
    font-weight: 600;
    font-size: 14.5px;
  }
  .marks {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .mark {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
  }
  .mark.update {
    background: color-mix(in srgb, var(--update) 20%, transparent);
    color: var(--update);
  }
  .mark.bad {
    color: var(--bad);
  }
  .foot {
    display: flex;
    align-items: baseline;
    gap: 8px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .ver {
    flex: 1;
  }
  .use {
    color: var(--text-3);
  }
  @media (max-width: 760px) {
    .tile {
      width: calc(50% - 4px);
    }
  }
</style>
