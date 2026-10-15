<!-- Backup settings: where archives go, the schedule, retention, what to skip, and an after-backup hook. -->
<script lang="ts">
  import CronInput from '../../components/CronInput.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import ListEditor from '../../components/ListEditor.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { live } from '../../lib/live.svelte';
  import SettingsSection from './SettingsSection.svelte';

  const running = $derived(live.list.filter((c) => c.state === 'running').map((c) => c.name));
</script>

<SettingsSection section="backups" title="Backups" intro="Each container can override what's included and how it's copied in its own settings.">
  {#snippet children(b)}
    <Group title="Where and when">
      <Field label="Backup folder" id="b-root" hint="Inside Berth's container. Mount your backup disk here, for example /mnt/backup/berth:/backups.">
        <input id="b-root" class="input mono" bind:value={b.root} placeholder="/backups" spellcheck="false" />
      </Field>
      <Field label="Schedule">
        <Toggle bind:checked={b.schedule_enabled} label="Back up on a schedule" hint="Otherwise only manual and pre-update backups are made." />
      </Field>
      {#if b.schedule_enabled}
        <Field label="When" id="b-cron">
          <CronInput id="b-cron" bind:value={b.cron} />
        </Field>
      {/if}
      <Field label="Containers">
        <Toggle bind:checked={b.include_all_by_default} label="Include every container unless it opts out" hint="Off: only containers you switch on are backed up on the schedule." />
      </Field>
    </Group>

    <Group title="Keeping">
      <Field label="Backups per container" id="b-count" hint="The newest ones are kept. 0 keeps all of them.">
        <input id="b-count" class="input num short" type="number" min="0" bind:value={b.retention_count} />
      </Field>
      <Field label="Delete after" id="b-days" hint="Days. 0 never deletes by age.">
        <input id="b-days" class="input num short" type="number" min="0" bind:value={b.retention_days} />
      </Field>
      <Field label="Pre-update backups" id="b-pre" hint="Days to keep the backups taken before updates and restores.">
        <input id="b-pre" class="input num short" type="number" min="1" bind:value={b.pre_update_retention_days} />
      </Field>
    </Group>

    <Group title="How">
      <Field label="Consistency" id="b-strategy" hint="Apps with databases need a pause or a database dump for a consistent copy; automatic picks per app.">
        <select id="b-strategy" class="input" bind:value={b.default_strategy}>
          <option value="auto">Automatic (dump or pause where needed)</option>
          <option value="pause">Pause while copying</option>
          <option value="stop">Stop while copying</option>
          <option value="live">Copy while running</option>
        </select>
      </Field>
      <Field label="Compression">
        <Segmented
          label="Compression"
          bind:value={b.compression}
          options={[
            { value: 'zstd', label: 'zstd (fast, small)' },
            { value: 'gzip', label: 'gzip (compatible)' },
          ]}
        />
      </Field>
      {#if b.compression === 'zstd'}
        <Field label="zstd level" id="b-level" hint="1 is fastest, 19 is smallest. 3 is a good balance.">
          <input id="b-level" class="input num short" type="number" min="1" max="19" bind:value={b.zstd_level} />
        </Field>
      {/if}
      <Field label="Compose files">
        <Toggle bind:checked={b.include_compose} label="Include the stack's compose file and .env" hint="So a backup can rebuild the container on a new machine." />
      </Field>
      <Field label="Media and data folders" hint="Folders under these are treated as media or bulk data and skipped unless a container includes them.">
        <ListEditor bind:items={b.data_roots} placeholder="/mnt" mono addLabel="Add folder" />
      </Field>
      <Field label="Skip files matching" hint="Glob patterns applied inside every backed up folder.">
        <ListEditor bind:items={b.exclude} placeholder="**/*.log" mono addLabel="Add pattern" />
      </Field>
    </Group>

    <Group title="After each backup run" hint="For off-site copies, like rclone to cloud storage.">
      <Field label="Command" id="b-hook" hint="Run in Berth's container. BERTH_BACKUP_ROOT and BERTH_BACKUP_STATUS (ok or failed) are set.">
        <input id="b-hook" class="input mono" bind:value={b.hook.command} placeholder="rclone sync /backups remote:berth" spellcheck="false" />
      </Field>
      <Field label="Or run it in a container" id="b-hook-ct" hint="The name of a container that has the tool, like an rclone container.">
        <input id="b-hook-ct" class="input mono" bind:value={b.hook.exec_container} list="b-running" placeholder="rclone" spellcheck="false" />
        <datalist id="b-running">{#each running as n (n)}<option value={n}></option>{/each}</datalist>
      </Field>
      {#if b.hook.exec_container}
        <Field label="Command in the container" id="b-hook-cmd">
          <input id="b-hook-cmd" class="input mono" bind:value={b.hook.exec_command} placeholder="rclone sync /backups crypt:berth" spellcheck="false" />
        </Field>
      {/if}
      <Field label="Run it">
        <Segmented
          label="When to run the hook"
          bind:value={b.hook.when}
          options={[
            { value: 'success', label: 'After successful runs' },
            { value: 'always', label: 'After every run' },
          ]}
        />
      </Field>
      <Field label="Time limit" id="b-hook-timeout" hint="Seconds before the hook is stopped.">
        <input id="b-hook-timeout" class="input num short" type="number" min="10" bind:value={b.hook.timeout_secs} />
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .short {
    max-width: 140px;
  }
</style>
