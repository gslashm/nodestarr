<!--
  Editor for one notification channel: its connection details (per kind),
  which events it receives, and the minimum severity.
-->
<script lang="ts">
  import Button from '../../components/Button.svelte';
  import Dialog from '../../components/Dialog.svelte';
  import Field from '../../components/Field.svelte';
  import KeyValueEditor from '../../components/KeyValueEditor.svelte';
  import ListEditor from '../../components/ListEditor.svelte';
  import SecretInput from '../../components/SecretInput.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { session } from '../../lib/session.svelte';
  import type { KeyValue, NotificationProvider } from '../../lib/types';
  import { PROVIDER_LABEL } from './providers';

  interface Props {
    provider: NotificationProvider | null;
    kinds: { kind: string; label: string }[];
    onapply: (p: NotificationProvider) => void;
    onclose: () => void;
  }

  let { provider, kinds, onapply, onclose }: Props = $props();

  let p = $state<NotificationProvider | null>(null);
  let headers = $state<KeyValue[]>([]);
  let onlySome = $state(false);
  let testing = $state(false);
  let testResult = $state<{ ok: boolean; text: string } | null>(null);

  $effect(() => {
    if (provider) {
      p = JSON.parse(JSON.stringify(provider));
      onlySome = provider.events.length > 0;
      headers = provider.config.kind === 'webhook' ? Object.entries(provider.config.headers).map(([key, value]) => ({ key, value })) : [];
      testResult = null;
    } else p = null;
  });

  // Event kinds grouped by category ("update", "backup", ...), without the test message.
  const groups = $derived.by(() => {
    const out = new Map<string, { kind: string; label: string }[]>();
    for (const k of kinds.filter((k) => k.kind !== 'test')) {
      const cat = k.kind.split('.')[0];
      out.set(cat, [...(out.get(cat) ?? []), k]);
    }
    return [...out.entries()];
  });

  function toggleEvent(kind: string, on: boolean) {
    if (!p) return;
    p.events = on ? [...new Set([...p.events, kind])] : p.events.filter((e) => e !== kind);
  }

  /** The provider as it would be saved (header rows folded back into a map). */
  function result(): NotificationProvider | null {
    if (!p) return null;
    const out: NotificationProvider = JSON.parse(JSON.stringify(p));
    if (!onlySome) out.events = [];
    if (out.config.kind === 'webhook') {
      out.config.headers = Object.fromEntries(headers.filter((h) => h.key.trim()).map((h) => [h.key.trim(), h.value]));
    }
    return out;
  }

  async function test() {
    const r = result();
    if (!r) return;
    testing = true;
    testResult = null;
    try {
      await api.post('/settings/test/notification', r);
      testResult = { ok: true, text: 'Sent. Check that it arrived.' };
    } catch (err) {
      testResult = { ok: false, text: errorMessage(err) };
    } finally {
      testing = false;
    }
  }

  function apply() {
    const r = result();
    if (r) onapply(r);
  }

  const secretKeep = $derived(session.secretPlaceholder);
</script>

<Dialog open={!!p} title={p ? `${PROVIDER_LABEL[p.config.kind]}: ${p.name || 'new channel'}` : ''} width="680px" {onclose}>
  {#if p}
    {@const c = p.config}
    <Field label="Name" id="np-name" hint="Shown in Berth; also used as the sender name where the service supports it.">
      <input id="np-name" class="input" bind:value={p.name} />
    </Field>
    <Field label="On">
      <Toggle bind:checked={p.enabled} label="Send notifications to this channel" />
    </Field>

    {#if c.kind === 'discord'}
      <Field label="Webhook URL" id="np-url" hint="Server settings > Integrations > Webhooks > New webhook > Copy webhook URL.">
        <SecretInput id="np-url" bind:value={c.webhook_url} placeholder="https://discord.com/api/webhooks/…" label="Webhook URL" />
      </Field>
      <Field label="Bot name" id="np-user" hint="Optional; overrides the webhook's name.">
        <input id="np-user" class="input" bind:value={c.username} placeholder="Berth" />
      </Field>
      <Field label="Mention" id="np-mention" hint={'Optional, added to problems: @here, @everyone, <@user id> or <@&role id>.'}>
        <input id="np-mention" class="input mono" bind:value={c.mention} placeholder="@here" />
      </Field>
    {:else if c.kind === 'slack'}
      <Field label="Webhook URL" id="np-url" hint="An incoming webhook from a Slack app.">
        <SecretInput id="np-url" bind:value={c.webhook_url} placeholder="https://hooks.slack.com/services/…" label="Webhook URL" />
      </Field>
    {:else if c.kind === 'email'}
      <Field label="SMTP server" id="np-host">
        <input id="np-host" class="input mono" bind:value={c.host} placeholder="smtp.gmail.com" />
      </Field>
      <Field label="Port and security" id="np-port">
        <div class="inline">
          <input id="np-port" class="input num port" type="number" min="1" max="65535" bind:value={c.port} />
          <select class="input" bind:value={c.security} aria-label="Security">
            <option value="starttls">STARTTLS (usually port 587)</option>
            <option value="tls">TLS (usually port 465)</option>
            <option value="none">None (local relays only)</option>
          </select>
        </div>
      </Field>
      <Field label="User name" id="np-smtp-user">
        <input id="np-smtp-user" class="input" bind:value={c.username} autocomplete="off" />
      </Field>
      <Field label="Password" id="np-smtp-pass" hint="For Gmail and similar, an app password.">
        <SecretInput id="np-smtp-pass" bind:value={c.password} label="SMTP password" />
      </Field>
      <Field label="From" id="np-from">
        <input id="np-from" class="input" bind:value={c.from} placeholder="Berth <berth@example.com>" />
      </Field>
      <Field label="To">
        <ListEditor bind:items={c.to} placeholder="you@example.com" addLabel="Add recipient" />
      </Field>
    {:else if c.kind === 'telegram'}
      <Field label="Bot token" id="np-bot" hint="From @BotFather.">
        <SecretInput id="np-bot" bind:value={c.bot_token} placeholder="123456:ABC…" label="Bot token" />
      </Field>
      <Field label="Chat ID" id="np-chat" hint="Your user ID, or a group's ID (negative). Send the bot a message first.">
        <input id="np-chat" class="input mono" bind:value={c.chat_id} />
      </Field>
      <Field label="Topic ID" id="np-topic" hint="Optional, for a topic in a group with topics.">
        <input id="np-topic" class="input mono" bind:value={c.topic_id} />
      </Field>
    {:else if c.kind === 'ntfy'}
      <Field label="Server" id="np-server">
        <input id="np-server" class="input mono" bind:value={c.server} placeholder="https://ntfy.sh" />
      </Field>
      <Field label="Topic" id="np-ntopic" hint="On the public server, pick something hard to guess.">
        <input id="np-ntopic" class="input mono" bind:value={c.topic} />
      </Field>
      <Field label="Access token" id="np-ntoken" hint="Optional.">
        <SecretInput id="np-ntoken" bind:value={c.token} label="Access token" />
      </Field>
      <Field label="Or user name and password" id="np-nuser">
        <div class="inline">
          <input id="np-nuser" class="input" bind:value={c.username} placeholder="User name" autocomplete="off" />
          <SecretInput bind:value={c.password} placeholder="Password" label="ntfy password" />
        </div>
      </Field>
    {:else if c.kind === 'gotify'}
      <Field label="Server" id="np-gserver">
        <input id="np-gserver" class="input mono" bind:value={c.server} placeholder="https://gotify.example.com" />
      </Field>
      <Field label="App token" id="np-gtoken">
        <SecretInput id="np-gtoken" bind:value={c.app_token} label="App token" />
      </Field>
    {:else if c.kind === 'pushover'}
      <Field label="User key" id="np-pkey">
        <SecretInput id="np-pkey" bind:value={c.user_key} label="User key" />
      </Field>
      <Field label="App token" id="np-ptoken">
        <SecretInput id="np-ptoken" bind:value={c.app_token} label="App token" />
      </Field>
      <Field label="Device" id="np-pdev" hint="Optional; empty sends to all your devices.">
        <input id="np-pdev" class="input" bind:value={c.device} />
      </Field>
    {:else if c.kind === 'webhook'}
      <Field label="URL" id="np-wurl">
        <input id="np-wurl" class="input mono" bind:value={c.url} placeholder="https://example.com/hook" />
      </Field>
      <Field label="Method" id="np-method">
        <select id="np-method" class="input short" bind:value={c.method}>
          {#each ['POST', 'PUT', 'PATCH', 'GET'] as m (m)}<option value={m}>{m}</option>{/each}
        </select>
      </Field>
      <Field label="Headers" hint="Values are stored like passwords.">
        <KeyValueEditor bind:items={headers} keyLabel="Header" keyPlaceholder="Authorization" addLabel="Add header" textMode={false} />
        {#if headers.some((h) => h.value === secretKeep)}<span class="faint small">Saved values are hidden; leave them as they are to keep them.</span>{/if}
      </Field>
      <Field
        label="Body"
        id="np-body"
        wide
        hint={'Empty sends JSON with kind, severity, title, message, fields, target, target_name, url, timestamp and instance. Or write a template using those names, like {"text": "{{ title }}: {{ message }}"}.'}
      >
        <textarea id="np-body" class="input mono" rows="5" bind:value={c.body_template} spellcheck="false"></textarea>
      </Field>
    {/if}

    <Field label="Send">
      <div class="events">
        <label><input type="radio" name="np-events" checked={!onlySome} onchange={() => (onlySome = false)} /> Everything</label>
        <label><input type="radio" name="np-events" checked={onlySome} onchange={() => (onlySome = true)} /> Only these events</label>
        {#if onlySome}
          <div class="groups">
            {#each groups as [cat, items] (cat)}
              <div class="group">
                {#each items as k (k.kind)}
                  <label class="ev"><input type="checkbox" checked={p.events.includes(k.kind) || p.events.includes(cat)} onchange={(e) => toggleEvent(k.kind, e.currentTarget.checked)} /> {k.label}</label>
                {/each}
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </Field>
    <Field label="At least" id="np-sev" hint="Skips anything less important.">
      <select id="np-sev" class="input short" bind:value={p.min_severity}>
        <option value="info">Information</option>
        <option value="warning">Warnings</option>
        <option value="error">Problems only</option>
      </select>
    </Field>
  {/if}
  {#snippet footer()}
    <Button variant="ghost" busy={testing} onclick={test}>Send a test</Button>
    {#if testResult}<span class="result {testResult.ok ? 'text-ok' : 'text-bad'}" role="status" title={testResult.text}>{testResult.text}</span>{/if}
    <span class="grow"></span>
    <button type="button" class="btn ghost" onclick={onclose}>Cancel</button>
    <button type="button" class="btn primary" onclick={apply}>Done</button>
  {/snippet}
</Dialog>

<style>
  .inline {
    display: flex;
    gap: 8px;
  }
  .port {
    max-width: 110px;
  }
  .short {
    max-width: 200px;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .events {
    display: grid;
    gap: 6px;
    padding-top: 6px;
  }
  .events label {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
  }
  .groups {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: 10px 16px;
    margin-top: 6px;
    padding: 10px 12px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
  }
  .group {
    display: grid;
    gap: 4px;
  }
  .ev {
    font-size: var(--fs-sm);
  }
  .result {
    align-self: center;
    min-width: 0;
    overflow: hidden;
    font-size: var(--fs-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
