<!-- Docker and Compose: where stacks live, the compose command, and container defaults. -->
<script lang="ts">
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import ListEditor from '../../components/ListEditor.svelte';
  import SettingsSection from './SettingsSection.svelte';
</script>

<SettingsSection section="docker" title="Docker" intro="Stacks found outside these folders still show up and work; these are where new stacks go and where Berth looks for undeployed ones.">
  {#snippet children(d)}
    <Group title="Stacks">
      <Field label="Stacks folders" hint="Absolute paths. Each subfolder with a compose file is a stack. New stacks are created in the first one. Berth must see these folders at the same path.">
        <ListEditor bind:items={d.stacks_dirs} placeholder="/opt/stacks" mono addLabel="Add folder" />
      </Field>
      <Field label="Compose command" id="d-compose" hint="Empty finds it on its own (docker compose, then docker-compose).">
        <input id="d-compose" class="input mono" bind:value={d.compose_command} placeholder="docker compose" spellcheck="false" />
      </Field>
    </Group>

    <Group title="Containers">
      <Field label="Stop timeout" id="d-stop" hint="Seconds to wait for a clean shutdown before Docker kills the container.">
        <input id="d-stop" class="input num short" type="number" min="1" bind:value={d.stop_timeout_secs} />
      </Field>
      <Field label="Hide containers" hint="Names or patterns (* and ?) of containers to leave out of lists, like *-db or watchtower.">
        <ListEditor bind:items={d.hidden_containers} placeholder="*-redis" mono addLabel="Add pattern" />
      </Field>
      <Field label="Console shells" hint="Tried in order when opening a console.">
        <ListEditor bind:items={d.console_shells} placeholder="/bin/bash" mono addLabel="Add shell" />
      </Field>
      <Field label="Full refresh every" id="d-resync" hint="Seconds between complete re-reads of Docker's state. Changes arrive instantly through Docker events; this is a safety net.">
        <input id="d-resync" class="input num short" type="number" min="30" bind:value={d.resync_secs} />
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .short {
    max-width: 140px;
  }
</style>
