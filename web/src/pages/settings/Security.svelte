<!-- How people sign in: forms or a reverse proxy, local-network access, sessions and metrics. -->
<script lang="ts">
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import ListEditor from '../../components/ListEditor.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import SettingsSection from './SettingsSection.svelte';
</script>

<SettingsSection section="security" title="Sign-in and security" intro="Changes here can lock you out; Nodestarr checks the values before saving.">
  {#snippet children(s)}
    <Group title="Sign-in">
      <Field label="Sign in with">
        <Segmented
          label="Sign-in method"
          bind:value={s.auth_mode}
          options={[
            { value: 'forms', label: 'Nodestarr accounts' },
            { value: 'external', label: 'Reverse proxy (SSO)' },
          ]}
        />
      </Field>
      {#if s.auth_mode === 'external'}
        <p class="warn">
          Nodestarr trusts the user name your proxy (Authelia, Authentik, oauth2-proxy…) sends in a header, but only from the proxy addresses below. Make sure the proxy is the only way to reach Nodestarr.
        </p>
        <Field label="User header" id="s-hdr" hint="The header your proxy sets to the signed-in user.">
          <input id="s-hdr" class="input mono" bind:value={s.external_header} placeholder="Remote-User" spellcheck="false" />
        </Field>
        <Field label="Trusted proxies" hint="Addresses or ranges (CIDR) of your reverse proxy. Required.">
          <ListEditor bind:items={s.trusted_proxies} placeholder="172.18.0.0/16" mono addLabel="Add" />
        </Field>
      {:else}
        <Field label="Trusted proxies" hint="Only needed to see real client addresses behind a reverse proxy (X-Forwarded-For).">
          <ListEditor bind:items={s.trusted_proxies} placeholder="172.18.0.0/16" mono addLabel="Add" />
        </Field>
      {/if}
    </Group>

    <Group title="Local network">
      <Field label="Skip sign-in at home">
        <Toggle bind:checked={s.bypass_local} label="No sign-in from the local networks below" hint="Convenient, but anyone on those networks gets full control." />
      </Field>
      <Field label="Local networks" hint="Ranges that count as local, like 192.168.1.0/24.">
        <ListEditor bind:items={s.local_networks} placeholder="192.168.1.0/24" mono addLabel="Add" />
      </Field>
    </Group>

    <Group title="Sessions">
      <Field label="Signed in for" id="s-hours" hint="Hours a sign-in lasts without “Remember me”.">
        <input id="s-hours" class="input num short" type="number" min="1" bind:value={s.session_hours} />
      </Field>
      <Field label="Remember me for" id="s-days" hint="Days a remembered sign-in lasts.">
        <input id="s-days" class="input num short" type="number" min="1" bind:value={s.remember_days} />
      </Field>
    </Group>

    <Group title="API and metrics">
      <Field label="API keys in links">
        <Toggle bind:checked={s.allow_query_api_key} label="Accept an API key in the address (?apikey=…)" hint="Some tools can't send headers. Keys in addresses can end up in logs, so leave this off if you can." />
      </Field>
      <Field label="Prometheus">
        <Toggle bind:checked={s.metrics_public} label="Serve /metrics without a key" hint="For Prometheus on a trusted network. Otherwise scrape it with an API key." />
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .warn {
    margin: 4px 0 8px;
    padding: 10px 12px;
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--warn) 10%, var(--panel));
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .short {
    max-width: 140px;
  }
</style>
