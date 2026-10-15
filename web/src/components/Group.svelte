<!-- A titled panel that groups related form fields. -->
<script lang="ts">
  import type { Snippet } from 'svelte';

  let { title = '', hint = '', children }: { title?: string; hint?: string; children: Snippet } = $props();
</script>

<section class="panel group">
  {#if title}
    <div class="panel-head">
      <h3>{title}</h3>
      {#if hint}<span class="faint hint">{hint}</span>{/if}
    </div>
  {/if}
  <div class="panel-body">{@render children()}</div>
</section>

<style>
  .group .panel-body {
    padding-top: 4px;
    padding-bottom: 6px;
  }
  .hint {
    font-size: var(--fs-sm);
  }
</style>
