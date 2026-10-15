<script lang="ts">
  import Check from '@lucide/svelte/icons/check';
  import Copy from '@lucide/svelte/icons/copy';
  import { ui } from '../lib/ui.svelte';

  let { text, label = 'Copy', small = true, showLabel = false }: { text: string | (() => string); label?: string; small?: boolean; showLabel?: boolean } = $props();

  let done = $state(false);

  async function copy() {
    const value = typeof text === 'function' ? text() : text;
    try {
      await navigator.clipboard.writeText(value);
    } catch {
      // Clipboard API needs a secure context; fall back to a hidden textarea.
      const ta = document.createElement('textarea');
      ta.value = value;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      ta.remove();
      if (!ok) {
        ui.toast('Copying is blocked by the browser here; select the text instead.', 'warning');
        return;
      }
    }
    done = true;
    setTimeout(() => (done = false), 1500);
  }
</script>

<button type="button" class="btn ghost" class:small class:icon={!showLabel} aria-label={label} title={label} onclick={copy}>
  {#if done}<Check size={15} class="text-ok" />{:else}<Copy size={15} />{/if}
  {#if showLabel}{done ? 'Copied' : label}{/if}
</button>
