<!-- All containers as a yard: one row ("bay") per stack, standalone last. -->
<script lang="ts">
  import { href } from '../lib/router.svelte';
  import { isUp, stateOf } from '../lib/format';
  import type { ContainerView } from '../lib/types';
  import YardTile from './YardTile.svelte';

  let { containers }: { containers: ContainerView[] } = $props();

  const bays = $derived.by(() => {
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

<div class="yard">
  {#each bays as bay (bay.stack)}
    <section class="bay" aria-label={bay.stack || 'Standalone containers'}>
      <div class="label">
        {#if bay.stack}
          <a class="stack" href={href(`/stacks/${encodeURIComponent(bay.stack)}`)}>{bay.stack}</a>
        {:else}
          <span class="stack plain">Standalone</span>
        {/if}
        <span class="count num" class:partial={bay.running < bay.items.length}>{bay.running} of {bay.items.length} up</span>
      </div>
      <div class="tiles">
        {#each bay.items as c (c.id)}
          <YardTile {c} />
        {/each}
      </div>
    </section>
  {/each}
</div>

<style>
  .yard {
    display: flex;
    flex-direction: column;
  }
  .bay {
    display: grid;
    grid-template-columns: 148px 1fr;
    gap: 16px;
    padding: 14px 0;
    border-top: 1px solid var(--line);
  }
  .bay:first-child {
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
    .bay {
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
