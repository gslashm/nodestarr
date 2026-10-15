<!-- All containers as tiles: one row per stack, standalone containers last. -->
<script lang="ts">
  import { href } from '../lib/router.svelte';
  import { isUp, stateOf } from '../lib/format';
  import type { ContainerView } from '../lib/types';
  import ContainerTile from './ContainerTile.svelte';

  let { containers }: { containers: ContainerView[] } = $props();

  const groups = $derived.by(() => {
    const map = new Map<string, ContainerView[]>();
    for (const c of containers) {
      const key = c.stack ?? '';
      if (!map.has(key)) map.set(key, []);
      map.get(key)!.push(c);
    }
    return [...map.entries()]
      .sort(([a], [b]) => (a === '' ? 1 : b === '' ? -1 : a.localeCompare(b)))
      .map(([stack, items]) => ({ stack, items, running: items.filter((c) => isUp(stateOf(c))).length }));
  });
</script>

<div class="grid">
  {#each groups as group (group.stack)}
    <section class="group" aria-label={group.stack || 'Standalone containers'}>
      <div class="label">
        {#if group.stack}
          <a class="stack" href={href(`/stacks/${encodeURIComponent(group.stack)}`)}>{group.stack}</a>
        {:else}
          <span class="stack plain">Standalone</span>
        {/if}
        <span class="count num" class:partial={group.running < group.items.length}>{group.running} of {group.items.length} up</span>
      </div>
      <div class="tiles">
        {#each group.items as c (c.id)}
          <ContainerTile {c} />
        {/each}
      </div>
    </section>
  {/each}
</div>

<style>
  .grid {
    display: flex;
    flex-direction: column;
  }
  .group {
    display: grid;
    grid-template-columns: 148px 1fr;
    gap: 16px;
    padding: 14px 0;
    border-top: 1px solid var(--line);
  }
  .group:first-child {
    border-top: 0;
    padding-top: 4px;
  }
  .label {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding-top: 6px;
    min-width: 0;
  }
  .stack {
    color: var(--text);
    font: 600 var(--fs-md) / 1.2 var(--font-data);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .stack.plain {
    color: var(--text-2);
  }
  .count {
    color: var(--text-3);
    font-size: var(--fs-sm);
  }
  .count.partial {
    color: var(--warn);
  }
  .tiles {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  @media (max-width: 760px) {
    .group {
      grid-template-columns: 1fr;
      gap: 8px;
    }
    .label {
      flex-direction: row;
      align-items: baseline;
      gap: 10px;
      padding-top: 0;
    }
  }
</style>
