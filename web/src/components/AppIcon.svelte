<!--
  An app's icon: the configured icon URL (if any), else a colored monogram.
  Falls back to the monogram if the image fails to load.
-->
<script lang="ts">
  import { iconUrl, monogramColor, monogramText } from '../lib/monogram';
  import { session } from '../lib/session.svelte';

  let {
    name,
    slug = null,
    icon = '',
    size = 32,
  }: { name: string; slug?: string | null; icon?: string; size?: number } = $props();

  let failed = $state(false);
  const url = $derived(iconUrl(session.settings?.general.ui.icon_url_template ?? '', slug, icon));
  const key = $derived(slug ?? name);
</script>

{#if url && !failed}
  <img class="icon" src={url} alt="" width={size} height={size} style:--s="{size}px" onerror={() => (failed = true)} />
{:else}
  <span class="mono-icon" style:--c={monogramColor(key)} style:--s="{size}px" aria-hidden="true">
    {monogramText(name)}
  </span>
{/if}

<style>
  .icon,
  .mono-icon {
    flex: none;
    width: var(--s);
    height: var(--s);
    border-radius: calc(var(--s) * 0.22);
  }
  .icon {
    object-fit: contain;
  }
  .mono-icon {
    display: inline-grid;
    place-items: center;
    background:
      repeating-linear-gradient(90deg, rgba(255, 255, 255, 0.07) 0 1px, transparent 1px calc(var(--s) / 7)),
      var(--c);
    color: #fff;
    font: 700 calc(var(--s) * 0.42) / 1 var(--font-data);
    letter-spacing: 0.01em;
    text-shadow: 0 1px 1px rgba(0, 0, 0, 0.25);
  }
</style>
