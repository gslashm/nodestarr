<!-- Renders the confirmation requested through ui.ask()/ui.confirm(). -->
<script lang="ts">
  import { ui } from '../lib/ui.svelte';
  import Dialog from './Dialog.svelte';

  let typed = $state('');
  let options = $state<Record<string, boolean>>({});

  $effect(() => {
    const c = ui.confirmState;
    if (c) {
      typed = '';
      options = Object.fromEntries((c.options ?? []).map((o) => [o.key, !!o.checked]));
    }
  });

  const c = $derived(ui.confirmState);
  const canConfirm = $derived(!c?.typeToConfirm || typed.trim() === c.typeToConfirm);

  function settle(ok: boolean) {
    ui.settleConfirm({ ok, options: { ...options } });
  }
</script>

<Dialog open={!!c} title={c?.title ?? ''} width="480px" onclose={() => settle(false)}>
  {#if c}
    <form
      id="confirm-form"
      onsubmit={(e) => {
        e.preventDefault();
        if (canConfirm) settle(true);
      }}
    >
      {#if c.message}<p class="message">{c.message}</p>{/if}
      {#each c.options ?? [] as o (o.key)}
        <label class="option" class:danger={o.danger}>
          <input type="checkbox" bind:checked={options[o.key]} />
          <span>{o.label}</span>
        </label>
      {/each}
      {#if c.typeToConfirm}
        <label class="type">
          <span>Type <strong>{c.typeToConfirm}</strong> to confirm</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input class="input" bind:value={typed} autocomplete="off" spellcheck="false" autofocus />
        </label>
      {/if}
    </form>
  {/if}
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={() => settle(false)}>{c?.cancelLabel ?? 'Cancel'}</button>
    <button form="confirm-form" type="submit" class="btn {c?.danger ? 'danger solid' : 'primary'}" disabled={!canConfirm}>
      {c?.confirmLabel ?? 'Confirm'}
    </button>
  {/snippet}
</Dialog>

<style>
  .message {
    color: var(--text-2);
    white-space: pre-line;
  }
  .option {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
    cursor: pointer;
  }
  .option.danger span {
    color: var(--bad);
  }
  .type {
    display: grid;
    gap: 6px;
    margin-top: 14px;
    color: var(--text-2);
  }
</style>
