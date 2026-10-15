<!-- Monitoring: resource history, watched disks, alert rules and how long history is kept. -->
<script lang="ts">
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import ListEditor from '../../components/ListEditor.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { alertKind } from '../../lib/alerts';
  import { session } from '../../lib/session.svelte';
  import SettingsSection from './SettingsSection.svelte';
</script>

<SettingsSection section="monitoring" title="Monitoring" intro="Berth samples resources cheaply: often while someone is looking, rarely otherwise.">
  {#snippet children(m)}
    <Group title="History">
      <Field label="Resource history">
        <Toggle bind:checked={m.history_enabled} label="Record CPU, memory, network and disk history" hint="Kept in Berth's database; a few megabytes for a typical server." />
      </Field>
      <Field label="Sample every" id="m-sample" hint="Seconds between history samples.">
        <input id="m-sample" class="input num short" type="number" min="10" bind:value={m.sample_secs} />
      </Field>
      <Field label="Live refresh" id="m-live" hint="Seconds between live updates while a page with numbers is open.">
        <input id="m-live" class="input num short" type="number" min="1" bind:value={m.live_secs} />
      </Field>
      <Field label="Keep detailed history" id="m-raw" hint="Hours of full-detail samples; older data is averaged into 15-minute points.">
        <input id="m-raw" class="input num short" type="number" min="1" bind:value={m.raw_retention_hours} />
      </Field>
      <Field label="Keep averaged history" id="m-rollup" hint="Days.">
        <input id="m-rollup" class="input num short" type="number" min="1" bind:value={m.rollup_retention_days} />
      </Field>
      <Field label="Watched disks" hint="Paths whose free space is shown and alerted on. The backup folder and Berth's data are always watched.">
        <ListEditor bind:items={m.disk_paths} placeholder="/mnt/storage" mono addLabel="Add path" />
      </Field>
    </Group>

    <Group title="Alerts" hint="Alerts go to notification channels that receive resource alerts.">
      <ul class="rules">
        {#each m.rules as r (r.id)}
          {@const k = alertKind(r.kind)}
          <li class:off={!r.enabled}>
            <div class="rule-head">
              <Toggle bind:checked={r.enabled} label={k.label} />
              <select class="input sev" bind:value={r.severity} aria-label="Severity of {k.label}" disabled={!r.enabled}>
                <option value="info">Information</option>
                <option value="warning">Warning</option>
                <option value="error">Problem</option>
              </select>
            </div>
            <p class="small muted">{k.describe(r)}</p>
            {#if r.enabled && (k.threshold || k.usesDuration)}
              <div class="params">
                {#if k.threshold}
                  <label>
                    {k.threshold.label}
                    <input class="input num" type="number" min={k.threshold.min} max={k.threshold.max} step={k.threshold.step ?? 1} bind:value={r.threshold} />
                    {k.threshold.unit}
                  </label>
                {/if}
                {#if k.usesDuration}
                  <label>
                    {r.kind === 'restart_loop' ? 'Within' : 'For at least'}
                    <input
                      class="input num"
                      type="number"
                      min="0"
                      value={Math.round(r.duration_secs / 60)}
                      oninput={(e) => (r.duration_secs = Math.max(0, Number(e.currentTarget.value) || 0) * 60)}
                    />
                    minutes
                  </label>
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      {#if session.defaults}
        <button type="button" class="btn small ghost reset" onclick={() => (m.rules = JSON.parse(JSON.stringify(session.defaults!.alert_rules)))}>Reset rules to defaults</button>
      {/if}
    </Group>

    <Group title="Logs">
      <Field label="Keep activity" id="m-act" hint="Days of the activity log.">
        <input id="m-act" class="input num short" type="number" min="1" bind:value={m.activity_retention_days} />
      </Field>
      <Field label="Keep task logs" id="m-task" hint="Days of finished tasks and their logs.">
        <input id="m-task" class="input num short" type="number" min="1" bind:value={m.task_retention_days} />
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .short {
    max-width: 140px;
  }
  .rules {
    display: grid;
    margin: 0;
    padding: 6px 0;
    list-style: none;
  }
  .rules li {
    display: grid;
    gap: 6px;
    padding: 12px 0;
    border-bottom: 1px solid var(--line);
  }
  .rules li:last-child {
    border-bottom: 0;
  }
  .rule-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
  }
  .sev {
    width: auto;
    min-height: 30px;
    padding-top: 3px;
    padding-bottom: 3px;
  }
  .small {
    margin: 0 0 0 48px;
    font-size: var(--fs-sm);
  }
  .params {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 20px;
    margin-left: 48px;
  }
  .params label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .params .input {
    width: 80px;
    min-height: 30px;
    padding: 3px 8px;
  }
  .reset {
    margin: 4px 0 8px;
  }
  @media (max-width: 760px) {
    .small,
    .params {
      margin-left: 0;
    }
  }
</style>
