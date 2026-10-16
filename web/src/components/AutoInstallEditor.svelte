<!--
  Which updates install automatically, by type: the version changes, the
  two things release notes can say, and whether tags may change. The chips
  match the ones on the Updates page.
-->
<script lang="ts">
  import ShieldAlert from '@lucide/svelte/icons/shield-alert';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { TYPE } from '../lib/updates';
  import type { AutoInstall } from '../lib/types';

  let { value = $bindable(), disabled = false }: { value: AutoInstall; disabled?: boolean } = $props();

  const SIZES = [
    { key: 'rebuild', what: 'The same version in a new image (base image and packaging fixes)' },
    { key: 'patch', what: '1.2.3 → 1.2.4' },
    { key: 'minor', what: '1.2 → 1.3' },
    { key: 'major', what: '1.x → 2.0' },
    { key: 'unknown', what: "New builds whose image doesn't say which version it is" },
  ] as const;
</script>

<div class="rules">
  <fieldset>
    <legend>Version changes</legend>
    {#each SIZES as s (s.key)}
      <label class="row">
        <input type="checkbox" bind:checked={value[s.key]} {disabled} />
        <span class="chip t-{s.key}">{TYPE[s.key].label}</span>
        <span class="what">{s.what}</span>
      </label>
    {/each}
  </fieldset>
  <fieldset>
    <legend>What the release notes say</legend>
    <label class="row">
      <input type="checkbox" bind:checked={value.security} {disabled} />
      <span class="chip t-security"><ShieldAlert size={12} />Security</span>
      <span class="what">Install security fixes even when their version change isn't ticked above (never major versions)</span>
    </label>
    <label class="row">
      <input type="checkbox" bind:checked={value.breaking} {disabled} />
      <span class="chip t-breaking"><TriangleAlert size={12} />Breaking</span>
      <span class="what">Install updates whose release notes mention breaking changes or steps to take</span>
    </label>
  </fieldset>
  <fieldset>
    <legend>Tags</legend>
    <label class="row wide">
      <input type="checkbox" bind:checked={value.switch_tags} {disabled} />
      <span class="what">
        <span class="name">Switch to newer tags</span>
        A container on a version tag like 16.4 moves to a newer one like 16.5 when that version's type is ticked. Off: only new builds of the tag it runs are installed.
      </span>
    </label>
  </fieldset>
</div>

<style>
  .rules {
    display: grid;
    gap: 14px;
  }
  fieldset {
    display: grid;
    gap: 2px;
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  legend {
    margin-bottom: 4px;
    padding: 0;
    color: var(--text-2);
    font-size: var(--fs-sm);
    font-weight: 500;
  }
  .row {
    display: grid;
    grid-template-columns: 16px 92px minmax(0, 1fr);
    gap: 10px;
    align-items: center;
    min-height: 32px;
    padding: 4px 0;
    cursor: pointer;
  }
  .row.wide {
    grid-template-columns: 16px minmax(0, 1fr);
    align-items: start;
  }
  .row.wide input {
    margin-top: 2px;
  }
  .row .chip {
    justify-self: start;
  }
  .what {
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .name {
    display: block;
    color: var(--text);
    font-size: var(--fs-md);
    font-weight: 500;
  }
  @media (max-width: 760px) {
    .row {
      grid-template-columns: 16px minmax(0, 1fr);
    }
    .row .what {
      grid-column: 2;
    }
  }
</style>
