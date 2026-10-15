<!-- Update settings: when to check, what to install on its own, safety nets, and registry logins. -->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import CronInput from '../../components/CronInput.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import SecretInput from '../../components/SecretInput.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import SettingsSection from './SettingsSection.svelte';

  const SCOPES = [
    { value: 'digest', label: 'New builds of the same tag only' },
    { value: 'patch', label: 'Patch versions (1.2.3 → 1.2.4)' },
    { value: 'minor', label: 'Minor versions (1.2 → 1.3)' },
    { value: 'major', label: 'Any version (1 → 2)' },
  ];
</script>

<SettingsSection section="updates" title="Updates" intro="Each container can override the policy in its own settings.">
  {#snippet children(u)}
    <Group title="Checking">
      <Field label="Check for updates" id="u-check" hint="Only image digests and tags are compared; nothing is downloaded.">
        <CronInput id="u-check" kind="check" bind:value={u.check_cron} />
      </Field>
      <Field label="Report new versions" id="u-notify-scope" hint="When the tag is a version (like 4.1.2), how far newer tags may go to count as an update.">
        <select id="u-notify-scope" class="input" bind:value={u.notify_scope}>
          {#each SCOPES as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
        </select>
      </Field>
      <Field label="Pre-releases">
        <Toggle bind:checked={u.ignore_prereleases} label="Ignore pre-release tags" hint="Skips tags like 2.0-beta or 1.5.0-rc1." />
      </Field>
      <Field label="Notify">
        <Toggle bind:checked={u.notify_available} label="Send a notification when updates are found" hint="One message per check, listing what's new." />
      </Field>
    </Group>

    <Group title="Automatic updates">
      <Field label="New containers" id="u-policy" hint="What happens for containers that don't have their own choice.">
        <select id="u-policy" class="input" bind:value={u.default_policy}>
          <option value="notify">Tell me about updates</option>
          <option value="auto">Install updates automatically</option>
          <option value="ignore">Don't check</option>
        </select>
      </Field>
      <Field label="Install on their own" id="u-scope" hint="For containers set to update automatically. Bigger jumps are only reported.">
        <select id="u-scope" class="input" bind:value={u.default_auto_scope}>
          {#each SCOPES as s (s.value)}<option value={s.value}>{s.label}</option>{/each}
        </select>
      </Field>
      <Field label="When" id="u-auto" hint="Automatic updates run at this time, one container after another.">
        <CronInput id="u-auto" bind:value={u.auto_cron} />
      </Field>
      <Field label="Wait for new releases" id="u-age" hint="Hours a release must be out before it's installed automatically. Gives time for broken releases to be pulled.">
        <input id="u-age" class="input num short" type="number" min="0" bind:value={u.min_age_hours} />
      </Field>
    </Group>

    <Group title="Safety">
      <Field label="Backups">
        <Toggle bind:checked={u.backup_before_update} label="Back up before updating" hint="For containers with backups on. Pre-update backups are kept for the days set in Backups." />
      </Field>
      <Field label="Rollback">
        <Toggle bind:checked={u.rollback_on_failure} label="Roll back when the new version doesn't start" hint="The previous image is kept and put back if the container crashes or stays unhealthy." />
      </Field>
      <Field label="Watch for" id="u-settle" hint="Seconds a new container must keep running before the update counts as good.">
        <input id="u-settle" class="input num short" type="number" min="0" bind:value={u.settle_secs} />
      </Field>
      <Field label="Give up after" id="u-verify" hint="Seconds to wait for a health check to pass.">
        <input id="u-verify" class="input num short" type="number" min="10" bind:value={u.verify_timeout_secs} />
      </Field>
      <Field label="Keep old images" id="u-keep" hint="Previous images kept per container, for rolling back later.">
        <input id="u-keep" class="input num short" type="number" min="0" bind:value={u.keep_rollback_images} />
      </Field>
      <Field label="Clean up">
        <Toggle bind:checked={u.prune_after_update} label="Remove unused images after updating" hint="Frees disk space; rollback images are kept." />
      </Field>
      <Field label="Plex">
        <Toggle bind:checked={u.plex_guard} label="Don't update Plex while someone is watching" hint="Automatic updates of Plex wait until no stream is playing." />
      </Field>
    </Group>

    <Group title="Registries" hint="For private images and higher Docker Hub limits.">
      <div class="registries">
        {#each u.registries as r, i (i)}
          <div class="reg">
            <input class="input mono" bind:value={r.registry} placeholder="ghcr.io" aria-label="Registry {i + 1}" spellcheck="false" />
            <input class="input" bind:value={r.username} placeholder="User name" aria-label="User name {i + 1}" autocomplete="off" />
            <SecretInput bind:value={r.password} placeholder="Password or token" label="Password {i + 1}" />
            <button type="button" class="btn ghost icon small" aria-label="Remove {r.registry || 'registry'}" onclick={() => (u.registries = u.registries.filter((_, j) => j !== i))}>
              <X size={15} />
            </button>
          </div>
        {/each}
        <div>
          <button type="button" class="btn small" onclick={() => (u.registries = [...u.registries, { registry: '', username: '', password: '' }])}>
            <Plus size={15} />Add registry login
          </button>
        </div>
      </div>
      <Field label="GitHub token" id="u-gh" hint="Optional. Release notes come from GitHub, which limits anonymous requests to 60 an hour. A token with no scopes is enough.">
        <SecretInput id="u-gh" bind:value={u.github_token} placeholder="ghp_…" label="GitHub token" />
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .short {
    max-width: 140px;
  }
  .registries {
    display: grid;
    gap: 8px;
    padding: 10px 0;
  }
  .reg {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) minmax(0, 1.4fr) 28px;
    gap: 6px;
    align-items: center;
  }
  @media (max-width: 760px) {
    .reg {
      grid-template-columns: 1fr 28px;
    }
  }
</style>
