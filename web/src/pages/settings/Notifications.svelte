<!--
  Notification channels (Discord, email, Telegram, ntfy, Gotify, Pushover,
  Slack, webhooks), which events each receives, quiet hours and repeats.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import Pencil from '@lucide/svelte/icons/pencil';
  import Send from '@lucide/svelte/icons/send';
  import Trash2 from '@lucide/svelte/icons/trash-2';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import Menu, { type MenuEntry } from '../../components/Menu.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api } from '../../lib/api';
  import type { NotificationProvider } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';
  import ProviderEditor from './ProviderEditor.svelte';
  import { PROVIDER_LABEL, newProvider, type ProviderKind } from './providers';
  import SettingsSection from './SettingsSection.svelte';

  interface Status {
    last_ok: number | null;
    last_error: string | null;
    last_error_at: number | null;
  }

  let kinds = $state<{ kind: string; label: string }[]>([]);
  let status = $state<Record<string, Status>>({});
  /** The channel open in the editor, and where it goes back in the list (-1: new). */
  let editing = $state<{ provider: NotificationProvider; index: number } | null>(null);
  let list: NotificationProvider[] | null = null;

  async function loadKinds() {
    try {
      const r = await api.get<{ kinds: { kind: string; label: string }[]; status: Record<string, Status> }>('/notifications/kinds');
      kinds = r.kinds;
      status = r.status;
    } catch {
      // The editor still works without labels.
    }
  }

  onMount(loadKinds);

  function addMenu(providers: NotificationProvider[]): MenuEntry[] {
    return (Object.keys(PROVIDER_LABEL) as ProviderKind[]).map((k) => ({
      label: PROVIDER_LABEL[k],
      run: () => {
        list = providers;
        editing = { provider: newProvider(k), index: -1 };
      },
    }));
  }

  function apply(p: NotificationProvider) {
    if (!editing || !list) return;
    if (editing.index < 0) list.push(p);
    else list[editing.index] = p;
    editing = null;
  }

  async function test(p: NotificationProvider) {
    try {
      await api.post('/settings/test/notification', p);
      ui.toast(`Test sent to ${p.name}`, 'success');
    } catch (err) {
      ui.error(err, `${p.name} failed`);
    } finally {
      void loadKinds();
    }
  }

  function summary(p: NotificationProvider): string {
    const what = p.events.length === 0 ? 'Everything' : p.events.length === 1 ? 'One kind of event' : `${p.events.length} kinds of events`;
    const sev = p.min_severity === 'warning' ? ', warnings and up' : p.min_severity === 'error' ? ', problems only' : '';
    return what + sev;
  }
</script>

<SettingsSection section="notifications" title="Notifications" intro="Where Berth sends updates, crashes, backups and alerts. Changes apply once you save.">
  {#snippet children(n)}
    <Group title="Channels">
      {#if n.providers.length === 0}
        <p class="muted">No channels yet. Add one to hear about updates, failures and alerts.</p>
      {:else}
        <ul class="providers">
          {#each n.providers as p, i (p.id)}
            {@const st = status[p.id]}
            <li class:off={!p.enabled}>
              <span class="kind">{PROVIDER_LABEL[p.config.kind]}</span>
              <div class="what">
                <span class="name">{p.name}</span>
                <span class="faint small">
                  {p.enabled ? summary(p) : 'Off'}
                  {#if st?.last_error && (st.last_error_at ?? 0) > (st.last_ok ?? 0)}
                    <span class="text-bad">; last attempt failed <TimeAgo ts={st.last_error_at} />: {st.last_error}</span>
                  {:else if st?.last_ok}
                    ; last sent <TimeAgo ts={st.last_ok} />
                  {/if}
                </span>
              </div>
              <button type="button" class="btn small ghost" onclick={() => test(p)} title="Send a test message"><Send size={14} />Test</button>
              <button
                type="button"
                class="btn small ghost icon"
                aria-label="Edit {p.name}"
                title="Edit"
                onclick={() => {
                  list = n.providers;
                  editing = { provider: p, index: i };
                }}><Pencil size={15} /></button
              >
              <button type="button" class="btn small ghost icon" aria-label="Remove {p.name}" title="Remove" onclick={() => n.providers.splice(i, 1)}><Trash2 size={15} /></button>
            </li>
          {/each}
        </ul>
      {/if}
      <div class="add">
        <Menu items={addMenu(n.providers)} align="left" label="Add a channel">
          {#snippet trigger()}<span class="btn small">Add a channel</span>{/snippet}
        </Menu>
      </div>
    </Group>

    <Group title="Quiet hours" hint="Times in Berth's time zone.">
      <Field label="Quiet hours">
        <Toggle bind:checked={n.quiet_hours.enabled} label="Hold notifications at night" hint="Held messages are sent together when quiet hours end." />
      </Field>
      {#if n.quiet_hours.enabled}
        <Field label="From" id="q-start">
          <div class="times">
            <input id="q-start" class="input" type="time" bind:value={n.quiet_hours.start} />
            <span class="muted">to</span>
            <input class="input" type="time" bind:value={n.quiet_hours.end} aria-label="Until" />
          </div>
        </Field>
        <Field label="Problems">
          <Toggle bind:checked={n.quiet_hours.allow_errors} label="Send problems right away anyway" hint="Crashes, failed updates and down checks." />
        </Field>
      {/if}
    </Group>

    <Group title="Repeats">
      <Field label="Combine repeats for" id="q-dedup" hint="Minutes during which the same message about the same container is sent only once.">
        <input id="q-dedup" class="input num short" type="number" min="0" bind:value={n.dedup_minutes} />
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<ProviderEditor provider={editing?.provider ?? null} {kinds} onapply={apply} onclose={() => (editing = null)} />

<style>
  .providers {
    display: grid;
    margin: 0;
    padding: 6px 0;
    list-style: none;
  }
  .providers li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }
  .providers li:last-child {
    border-bottom: 0;
  }
  .providers li.off .name {
    color: var(--text-3);
  }
  .kind {
    flex: none;
    width: 82px;
    color: var(--text-2);
    font: 600 var(--fs-sm) / 1 var(--font-data);
  }
  .what {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .name {
    font-weight: 500;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .add {
    padding: 8px 0;
  }
  .times {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .times .input {
    width: 130px;
  }
  .short {
    max-width: 140px;
  }
</style>
