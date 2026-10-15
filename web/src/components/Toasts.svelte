<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import Info from '@lucide/svelte/icons/info';
  import TriangleAlert from '@lucide/svelte/icons/triangle-alert';
  import X from '@lucide/svelte/icons/x';
  import { ui } from '../lib/ui.svelte';

  const icons = { info: Info, success: CircleCheck, warning: TriangleAlert, error: CircleAlert };
</script>

<div class="toasts" aria-live="polite">
  {#each ui.toasts as t (t.id)}
    {@const Icon = icons[t.level]}
    <div class="toast {t.level}" role={t.level === 'error' ? 'alert' : 'status'}>
      <Icon size={18} aria-hidden="true" />
      <span class="msg">{t.message}</span>
      {#if t.action}
        <button type="button"
          class="btn small ghost action"
          onclick={() => {
            t.action?.run();
            ui.dismiss(t.id);
          }}>{t.action.label}</button
        >
      {/if}
      <button type="button" class="close" aria-label="Dismiss" onclick={() => ui.dismiss(t.id)}><X size={15} /></button>
    </div>
  {/each}
</div>

<style>
  .toasts {
    position: fixed;
    right: 16px;
    bottom: 16px;
    z-index: 60;
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: min(420px, calc(100vw - 32px));
  }
  .toast {
    --c: var(--info);
    display: flex;
    align-items: flex-start;
    gap: 10px;
    padding: 11px 10px 11px 14px;
    border: 1px solid var(--line-strong);
    border-left: 3px solid var(--c);
    border-radius: var(--radius);
    background: var(--panel);
    box-shadow: var(--shadow);
    animation: rise 160ms var(--ease);
  }
  .toast :global(svg) {
    flex: none;
    margin-top: 1px;
    color: var(--c);
  }
  .success {
    --c: var(--ok);
  }
  .warning {
    --c: var(--warn);
  }
  .error {
    --c: var(--bad);
  }
  .msg {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .action {
    margin: -4px 0;
    color: var(--accent-text);
  }
  .close {
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text-3);
    cursor: pointer;
  }
  .close:hover {
    color: var(--text);
    background: var(--raised);
  }
  .close :global(svg) {
    color: inherit;
    margin: 0;
  }
  @keyframes rise {
    from {
      transform: translateY(8px);
      opacity: 0;
    }
  }
  @media (max-width: 760px) {
    .toasts {
      right: 12px;
      left: 12px;
      bottom: 12px;
      width: auto;
    }
  }
</style>
