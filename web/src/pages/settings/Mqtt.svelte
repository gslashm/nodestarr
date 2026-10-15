<!--
  Home Assistant over MQTT: each container appears as a device with its
  state, version and update status (with MQTT discovery), and can be
  controlled from Home Assistant if you allow it.
-->
<script lang="ts">
  import PlugZap from '@lucide/svelte/icons/plug-zap';
  import Button from '../../components/Button.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import SecretInput from '../../components/SecretInput.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { session } from '../../lib/session.svelte';
  import type { Settings } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';
  import SettingsSection from './SettingsSection.svelte';

  let testing = $state(false);
  let result = $state<{ ok: boolean; text: string } | null>(null);

  async function test(m: Settings['mqtt']) {
    testing = true;
    result = null;
    try {
      await api.post('/settings/test/mqtt', m);
      result = { ok: true, text: 'Connected to the broker.' };
    } catch (err) {
      result = { ok: false, text: errorMessage(err) };
    } finally {
      testing = false;
    }
  }

  async function reconnect() {
    try {
      await api.post('/system/mqtt/reconnect');
      ui.toast('Reconnecting to the broker and republishing discovery');
    } catch (err) {
      ui.error(err);
    }
  }

  const TOPIC_LABEL: Record<string, string> = {
    availability: 'Online status',
    state: 'Container state',
    update: 'Update status',
    commands: 'Commands',
    berth: 'Berth commands',
  };
</script>

<SettingsSection section="mqtt" title="Home Assistant" intro="Berth publishes to your MQTT broker; Home Assistant's MQTT integration picks the devices up on its own.">
  {#snippet actions(m)}
    <Button icon={PlugZap} busy={testing} onclick={() => test(m)} disabled={!m.host}>Test connection</Button>
    {#if session.settings?.mqtt.enabled}<Button variant="ghost" onclick={reconnect}>Reconnect</Button>{/if}
  {/snippet}
  {#snippet children(m)}
    <Group>
      <Field label="MQTT">
        <Toggle bind:checked={m.enabled} label="Publish to an MQTT broker" />
      </Field>
      <Field label="Broker" id="mq-host">
        <div class="inline">
          <input id="mq-host" class="input mono" bind:value={m.host} placeholder="192.168.1.60" spellcheck="false" />
          <input class="input num port" type="number" min="1" max="65535" bind:value={m.port} aria-label="Port" />
        </div>
      </Field>
      <Field label="Encryption">
        <Toggle bind:checked={m.tls} label="Use TLS" hint="Usually on port 8883." />
      </Field>
      <Field label="User name" id="mq-user">
        <input id="mq-user" class="input" bind:value={m.username} autocomplete="off" />
      </Field>
      <Field label="Password" id="mq-pass">
        <SecretInput id="mq-pass" bind:value={m.password} label="MQTT password" />
      </Field>
      {#if result}<p class={result.ok ? 'text-ok' : 'text-bad'}>{result.text}</p>{/if}
    </Group>

    <Group title="What Home Assistant gets">
      <Field label="Resources">
        <Toggle bind:checked={m.expose_stats} label="CPU and memory sensors for each container" hint="Updated at the history sampling interval." />
      </Field>
      <Field label="Control">
        <Toggle bind:checked={m.expose_controls} label="Buttons to start, stop, restart and update containers" hint="Anyone who can use your Home Assistant can then control containers." />
      </Field>
      <Field label="Discovery prefix" id="mq-disc" hint="Home Assistant's default is homeassistant.">
        <input id="mq-disc" class="input mono short" bind:value={m.discovery_prefix} spellcheck="false" />
      </Field>
      <Field label="Base topic" id="mq-base">
        <input id="mq-base" class="input mono short" bind:value={m.base_topic} spellcheck="false" />
      </Field>
      <Field label="Client ID" id="mq-client" hint="Must be unique among the clients of your broker.">
        <input id="mq-client" class="input mono short" bind:value={m.client_id} spellcheck="false" />
      </Field>
      {#if Object.keys(session.mqttTopics).length}
        <Field label="Topics" hint="With the current saved settings.">
          <dl class="topics">
            {#each Object.entries(session.mqttTopics) as [k, v] (k)}
              <dt>{TOPIC_LABEL[k] ?? k}</dt>
              <dd class="mono">{v}</dd>
            {/each}
          </dl>
        </Field>
      {/if}
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .inline {
    display: flex;
    gap: 8px;
  }
  .port {
    max-width: 110px;
  }
  .short {
    max-width: 260px;
  }
  .topics {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 4px 14px;
    margin: 6px 0 0;
    font-size: var(--fs-sm);
  }
  .topics dt {
    color: var(--text-2);
  }
  .topics dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
