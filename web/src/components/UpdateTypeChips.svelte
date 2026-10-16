<!--
  An update's type as chips: its size (major, minor, patch, rebuild,
  unknown) and, from its release notes, security fixes and breaking
  changes. Hovering a chip says why it's there.
-->
<script lang="ts">
  import ShieldAlert from '@lucide/svelte/icons/shield-alert';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import { TYPE } from '../lib/updates';
  import type { Change } from '../lib/types';

  let { change }: { change: Change | null } = $props();

  function evidence(kind: 'security' | 'breaking'): string {
    const f = change?.notes?.findings.find((x) => x.kind === kind);
    return f ? `${TYPE[kind].hint}: “${f.text}” (${f.release})` : TYPE[kind].hint;
  }

  const sizeTitle = $derived(
    change?.estimated ? `${TYPE[change.bump].hint}. Estimated from the tag: the image doesn't say which version it is, so this is the most it can be.` : change ? TYPE[change.bump].hint : '',
  );
</script>

{#if change}
  <span class="types">
    <span class="chip t-{change.bump}" class:estimated={change.estimated} title={sizeTitle}>{change.estimated && change.bump !== 'rebuild' ? '≤ ' : ''}{TYPE[change.bump].label}</span>
    {#if change.security}<span class="chip t-security" title={evidence('security')}><ShieldAlert size={12} />Security</span>{/if}
    {#if change.breaking}<span class="chip t-breaking" title={evidence('breaking')}><TriangleAlert size={12} />Breaking</span>{/if}
    {#if change.prerelease}<span class="chip" title="The new version is a pre-release (alpha, beta, release candidate)">Pre-release</span>{/if}
  </span>
{/if}

<style>
  .types {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px;
    vertical-align: middle;
  }
</style>
