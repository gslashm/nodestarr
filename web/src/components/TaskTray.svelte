<!-- Running tasks with progress; click one to follow its log. -->
<script lang="ts">
  import { live } from '../lib/live.svelte';
  import { href } from '../lib/router.svelte';
  import { ui } from '../lib/ui.svelte';
  import Spinner from './Spinner.svelte';
</script>

{#if live.activeTasks.length}
  <div class="tray" aria-label="Running tasks">
    {#each live.activeTasks.slice(0, 4) as t (t.id)}
      <button type="button" class="task" onclick={() => (ui.openTaskId = t.id)} title={t.message ?? t.title}>
        <Spinner size={13} />
        <span class="text">
          <span class="title ellipsis">{t.title}</span>
          {#if t.message}<span class="msg ellipsis">{t.message}</span>{/if}
          {#if t.progress !== null}
            <span class="bar"><span style:width="{Math.round((t.progress ?? 0) * 100)}%"></span></span>
          {/if}
        </span>
      </button>
    {/each}
    {#if live.activeTasks.length > 4}
      <a class="more" href={href("/activity/tasks")}>{live.activeTasks.length - 4} more running</a>
    {/if}
  </div>
{/if}

<style>
  .tray {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .task {
    display: flex;
    gap: 9px;
    align-items: flex-start;
    width: 100%;
    padding: 8px 10px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--raised);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .task :global(.spinner) {
    margin-top: 2px;
    color: var(--accent-text);
  }
  .task:hover {
    border-color: var(--line-strong);
  }
  .text {
    display: grid;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }
  .title {
    font-size: var(--fs-sm);
    font-weight: 600;
  }
  .msg {
    color: var(--text-2);
    font-size: var(--fs-xs);
  }
  .bar {
    height: 3px;
    margin-top: 3px;
    border-radius: 2px;
    background: var(--line);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 300ms var(--ease);
  }
  .more {
    padding: 0 10px;
    font-size: var(--fs-sm);
  }
</style>
