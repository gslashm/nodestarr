<!--
  Modal dialog on the native <dialog> element (focus trap, Escape and the
  top layer for free). Set `open` to show it; `onclose` fires on Escape,
  backdrop click or the close button.
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import X from '@lucide/svelte/icons/x';

  interface Props {
    open: boolean;
    title: string;
    width?: string;
    /** Side panel instead of a centered dialog. */
    drawer?: boolean;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
    actions?: Snippet;
  }

  let { open, title, width = '560px', drawer = false, onclose, children, footer, actions }: Props = $props();

  let el: HTMLDialogElement | undefined = $state();

  $effect(() => {
    if (!el) return;
    if (open && !el.open) el.showModal();
    else if (!open && el.open) el.close();
  });

  function onBackdrop(e: MouseEvent) {
    if (e.target === el) onclose();
  }
</script>

<dialog
  bind:this={el}
  class:drawer
  style:--w={width}
  aria-label={title}
  oncancel={(e) => {
    e.preventDefault();
    onclose();
  }}
  onclick={onBackdrop}
>
  {#if open}
    <div class="frame">
      <header>
        <h2>{title}</h2>
        {#if actions}{@render actions()}{/if}
        <button type="button" class="btn ghost icon small" aria-label="Close" onclick={onclose}><X size={17} /></button>
      </header>
      <div class="body">{@render children()}</div>
      {#if footer}<footer>{@render footer()}</footer>{/if}
    </div>
  {/if}
</dialog>

<style>
  dialog {
    width: min(var(--w), calc(100vw - 24px));
    max-height: min(86vh, 900px);
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-lg);
    background: var(--panel);
    color: var(--text);
    box-shadow: var(--shadow);
  }
  dialog::backdrop {
    background: var(--overlay);
  }
  dialog[open] {
    animation: pop 140ms var(--ease);
  }
  dialog.drawer {
    margin: 0 0 0 auto;
    height: 100vh;
    max-height: 100vh;
    width: min(var(--w), 100vw);
    border-radius: 0;
    border-width: 0 0 0 1px;
  }
  dialog.drawer[open] {
    animation: slide 180ms var(--ease);
  }
  .frame {
    display: flex;
    flex-direction: column;
    max-height: inherit;
    height: 100%;
  }
  header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 14px 14px 12px 20px;
    border-bottom: 1px solid var(--line);
  }
  h2 {
    flex: 1;
    min-width: 0;
    font-size: var(--fs-lg);
    overflow-wrap: anywhere;
  }
  .body {
    flex: 1;
    min-height: 0;
    padding: 18px 20px;
    overflow: auto;
  }
  footer {
    display: flex;
    justify-content: flex-end;
    flex-wrap: wrap;
    gap: 8px;
    padding: 12px 20px;
    border-top: 1px solid var(--line);
  }
  @keyframes pop {
    from {
      transform: scale(0.97);
      opacity: 0;
    }
  }
  @keyframes slide {
    from {
      transform: translateX(24px);
      opacity: 0;
    }
  }
</style>
