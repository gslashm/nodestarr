<!-- Starts a vzdump backup of one Proxmox guest. -->
<script lang="ts">
  import Archive from '@lucide/svelte/icons/archive';
  import Button from '../../components/Button.svelte';
  import Field from '../../components/Field.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api } from '../../lib/api';
  import { bytes } from '../../lib/format';
  import { session } from '../../lib/session.svelte';
  import type { PveGuest, PveStorage } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let { guest, storages, ondone }: { guest: PveGuest; storages: PveStorage[]; ondone: () => void } = $props();

  const targets = $derived(storages.filter((s) => s.active && s.content.includes('backup')));
  let storage = $state('');
  let mode = $state<'snapshot' | 'suspend' | 'stop'>('snapshot');
  let notes = $state('{{guestname}}, by Nodestarr');
  let protect = $state(false);
  let busy = $state(false);

  $effect(() => {
    if (!storage) {
      const preferred = session.settings?.proxmox.backup_storage;
      storage = targets.find((s) => s.storage === preferred)?.storage ?? targets[0]?.storage ?? '';
    }
  });

  const MODES = {
    snapshot: 'Snapshot: no downtime (the default)',
    suspend: 'Suspend: pauses the guest briefly for a consistent copy',
    stop: 'Stop: shuts the guest down while copying; most consistent',
  };

  async function start() {
    if (!storage || busy) return;
    if (mode === 'stop' && guest.is_self) {
      const ok = await ui.confirm({
        title: 'Stop the guest that runs Nodestarr?',
        message: 'Nodestarr goes offline while the backup runs and comes back when the guest starts again.',
        confirmLabel: 'Back up',
        danger: true,
      });
      if (!ok) return;
    }
    busy = true;
    try {
      const r = await api.post<{ task_id: string }>(`/proxmox/guests/${guest.vmid}/backup`, { storage, mode, notes, protected: protect });
      ui.taskStarted(r.task_id, `Backing up ${guest.name} to ${storage}`);
      ondone();
    } catch (err) {
      ui.error(err, "Couldn't start the backup");
    } finally {
      busy = false;
    }
  }
</script>

{#if targets.length === 0}
  <p class="muted">No active storage on this node accepts backups. Enable the "VZDump backup file" content type on a storage in Proxmox first.</p>
{:else}
  <form
    onsubmit={(e) => {
      e.preventDefault();
      void start();
    }}
  >
    <Field label="Storage" id="pb-storage">
      <select id="pb-storage" class="input" bind:value={storage}>
        {#each targets as s (s.storage)}
          <option value={s.storage}>{s.storage} ({bytes(s.total - s.used)} free)</option>
        {/each}
      </select>
    </Field>
    <Field label="Mode" id="pb-mode">
      <select id="pb-mode" class="input" bind:value={mode}>
        {#each Object.entries(MODES) as [value, label] (value)}<option {value}>{label}</option>{/each}
      </select>
    </Field>
    <Field label="Notes" id="pb-notes" hint={'Shown in Proxmox. {{guestname}}, {{vmid}} and {{node}} are filled in.'}>
      <input id="pb-notes" class="input" bind:value={notes} />
    </Field>
    <Field label="Protection">
      <Toggle bind:checked={protect} label="Protect this backup" hint="Proxmox won't prune or delete it until you remove the protection." />
    </Field>
    <div class="actions">
      <Button type="submit" variant="primary" icon={Archive} {busy} disabled={!storage}>Back up {guest.name}</Button>
    </div>
  </form>
{/if}

<style>
  .actions {
    display: flex;
    justify-content: flex-end;
    padding-top: 8px;
  }
</style>
