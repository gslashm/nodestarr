<!-- Creates or edits a self-test schedule: what kind of test, when, and on which disks. -->
<script lang="ts">
  import CronInput from '../../../components/CronInput.svelte';
  import Dialog from '../../../components/Dialog.svelte';
  import Field from '../../../components/Field.svelte';
  import Segmented from '../../../components/Segmented.svelte';
  import Toggle from '../../../components/Toggle.svelte';
  import { MEDIA, SCHEDULE_TARGETS, TEST_KIND, diskSize, testLength } from '../../../lib/disks';
  import { plural } from '../../../lib/format';
  import type { DiskView, TestSchedule } from '../../../lib/types';

  let {
    open,
    schedule,
    disks,
    nodes,
    onclose,
    onsave,
  }: {
    open: boolean;
    schedule: TestSchedule | null;
    disks: DiskView[];
    nodes: string[];
    onclose: () => void;
    onsave: (s: TestSchedule) => Promise<void> | void;
  } = $props();

  function blank(): TestSchedule {
    return { id: `s-${Math.random().toString(36).slice(2, 10)}`, name: '', enabled: true, kind: 'short', cron: '0 3 * * 0', disks: 'all', ids: [], nodes: [] };
  }

  let form = $state<TestSchedule>(blank());
  let saving = $state(false);
  let error = $state('');

  $effect(() => {
    if (open) {
      const { next_run: _n, covers: _c, ...rest } = schedule ?? blank();
      form = JSON.parse(JSON.stringify(rest));
      error = '';
    }
  });

  const covered = $derived(
    disks.filter((d) => {
      if (form.nodes.length && !form.nodes.includes(d.node)) return false;
      switch (form.disks) {
        case 'hdd':
          return d.media === 'hdd';
        case 'ssd':
          return d.media === 'ssd' || d.media === 'nvme';
        case 'nvme':
          return d.media === 'nvme';
        case 'list':
          return form.ids.includes(d.id);
        default:
          return true;
      }
    }),
  );
  const unsupported = $derived(covered.filter((d) => (form.kind === 'conveyance' ? !d.tests.conveyance : form.kind === 'long' ? !d.tests.long : !d.tests.short)));
  const longest = $derived(
    Math.max(0, ...covered.map((d) => (form.kind === 'long' ? d.tests.long_minutes : form.kind === 'conveyance' ? d.tests.conveyance_minutes : d.tests.short_minutes) ?? 0)),
  );

  function toggleId(id: string, on: boolean) {
    form.ids = on ? [...form.ids, id] : form.ids.filter((x) => x !== id);
  }

  function toggleNode(n: string, on: boolean) {
    form.nodes = on ? [...form.nodes, n] : form.nodes.filter((x) => x !== n);
  }

  async function save() {
    if (!form.name.trim()) form.name = `${TEST_KIND[form.kind]} test`;
    if (form.disks === 'list' && !form.ids.length) {
      error = 'Choose at least one disk.';
      return;
    }
    saving = true;
    try {
      await onsave({ ...form, name: form.name.trim() });
    } finally {
      saving = false;
    }
  }
</script>

<Dialog {open} title={schedule ? `Edit “${schedule.name}”` : 'New self-test schedule'} width="620px" {onclose}>
  <div class="form">
    <Field label="Name" id="ts-name">
      <input id="ts-name" class="input" bind:value={form.name} placeholder="{TEST_KIND[form.kind]} test" />
    </Field>
    <Field label="Test" hint={form.kind === 'short' ? 'A couple of minutes: checks the electronics, the heads and a sample of the surface.' : form.kind === 'long' ? 'Reads the whole surface; hours on big hard disks. Finds bad sectors before your data does.' : 'A few minutes: checks for damage from shipping (ATA disks only).'}>
      <Segmented
        label="Test type"
        bind:value={form.kind}
        options={[
          { value: 'short', label: 'Short' },
          { value: 'long', label: 'Long' },
          { value: 'conveyance', label: 'Conveyance' },
        ]}
      />
    </Field>
    <Field label="When" id="ts-cron" hint="In Nodestarr's time zone. Tests that are still waiting when the next one is due are not queued twice.">
      <CronInput id="ts-cron" bind:value={form.cron} kind="test" />
    </Field>
    <Field label="Disks">
      <Segmented
        label="Disks"
        bind:value={form.disks}
        options={(['all', 'hdd', 'ssd', 'nvme', 'list'] as const).map((v) => ({ value: v, label: SCHEDULE_TARGETS[v] }))}
      />
    </Field>
    {#if form.disks === 'list'}
      <div class="pick">
        {#each disks as d (d.id)}
          <label class="pick-row">
            <input type="checkbox" checked={form.ids.includes(d.id)} onchange={(e) => toggleId(d.id, e.currentTarget.checked)} />
            <span class="mono">{d.devpath.replace('/dev/', '')}</span>
            <span class="chip">{MEDIA[d.media]}</span>
            <span class="grow ellipsis small">{d.model ?? ''} <span class="faint">{diskSize(d.size)}{d.label.bay ? ` · ${d.label.bay}` : ''}{nodes.length > 1 ? ` · ${d.node}` : ''}</span></span>
          </label>
        {/each}
      </div>
    {/if}
    {#if nodes.length > 1}
      <Field label="Nodes" hint="None checked: all nodes.">
        <div class="nodes">
          {#each nodes as n (n)}
            <label class="pick-row"><input type="checkbox" checked={form.nodes.includes(n)} onchange={(e) => toggleNode(n, e.currentTarget.checked)} /> {n}</label>
          {/each}
        </div>
      </Field>
    {/if}
    <Field label="Schedule">
      <Toggle bind:checked={form.enabled} label="Enabled" />
    </Field>
    <p class="summary small">
      Covers {plural(covered.length, 'disk')} right now{#if longest} · the longest takes about {testLength(longest)}{/if}.
      {#if unsupported.length}<span class="text-warn">{plural(unsupported.length, 'disk')} can't run this test and will be skipped.</span>{/if}
      Disks in a pool that is resilvering or scrubbing wait until it finishes.
    </p>
    {#if error}<p class="text-bad small">{error}</p>{/if}
  </div>
  {#snippet footer()}
    <button type="button" class="btn ghost" onclick={onclose}>Cancel</button>
    <button type="button" class="btn primary" disabled={saving} onclick={save}>{schedule ? 'Save' : 'Add schedule'}</button>
  {/snippet}
</Dialog>

<style>
  .form {
    display: grid;
    gap: 4px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .pick {
    display: grid;
    gap: 2px;
    max-height: 260px;
    margin: 0 0 8px;
    padding: 6px;
    overflow-y: auto;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
  }
  .pick-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    padding: 0 6px;
    border-radius: var(--radius-sm);
    cursor: pointer;
  }
  .pick-row:hover {
    background: var(--raised);
  }
  .nodes {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
  }
  .summary {
    margin: 6px 0 0;
    color: var(--text-2);
  }
</style>
