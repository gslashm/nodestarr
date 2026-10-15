<!-- General settings: name, addresses, time zone, and how the interface looks. -->
<script lang="ts">
  import { onMount } from 'svelte';
  import AppIcon from '../../components/AppIcon.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api } from '../../lib/api';
  import { applyAccent, applyDensity } from '../../lib/theme';
  import SettingsSection from './SettingsSection.svelte';

  let zones = $state<string[]>([]);

  onMount(() => {
    api
      .get<{ timezones: string[] }>('/system/timezones')
      .then((r) => (zones = r.timezones))
      .catch(() => {});
  });

  // Nodestarr magenta first; the others are alternatives people often like.
  const ACCENTS = ['#e46cff', '#f2c230', '#2fd3c0', '#4fb3ff', '#b8e04a', '#dfe4f5'];
  const browserZone = Intl.DateTimeFormat().resolvedOptions().timeZone;
</script>

<SettingsSection section="general" title="General" intro="How this Nodestarr is named and reached, and how it looks for everyone.">
  {#snippet children(g)}
    <Group title="This instance">
      <Field label="Name" id="g-name" hint="Shown in the sidebar, the browser tab and notifications.">
        <input id="g-name" class="input" bind:value={g.instance_name} />
      </Field>
      <Field label="Host address" id="g-host" hint={'The address of this Docker host as you open it, used for container links ({host}). Empty uses the address in your browser.'}>
        <input id="g-host" class="input mono" bind:value={g.public_host} placeholder="192.168.1.50" spellcheck="false" />
      </Field>
      <Field label="Nodestarr's address" id="g-ext" hint="The full address of Nodestarr, used for links in notifications, like http://192.168.1.50:7979.">
        <input id="g-ext" class="input mono" bind:value={g.external_url} placeholder="http://192.168.1.50:7979" spellcheck="false" />
      </Field>
      <Field label="URL path" id="g-base" hint="When Nodestarr sits behind a reverse proxy under a path, like /nodestarr. Takes effect after a restart.">
        <input id="g-base" class="input mono" bind:value={g.url_base} placeholder="(served at the root)" spellcheck="false" />
      </Field>
      <Field label="Time zone" id="g-tz" hint="For schedules and times in notifications. Empty uses the TZ variable, then UTC.">
        <input id="g-tz" class="input" bind:value={g.timezone} list="g-zones" placeholder={browserZone} spellcheck="false" />
        <datalist id="g-zones">{#each zones as z (z)}<option value={z}></option>{/each}</datalist>
      </Field>
      <Field label="Nodestarr updates">
        <Toggle bind:checked={g.check_nodestarr_updates} label="Tell me when a new Nodestarr version is out" hint="Checks the Nodestarr image once a day." />
      </Field>
    </Group>

    <Group title="Look" hint="Each browser can still pick light or dark from the account menu.">
      <Field label="Theme">
        <Segmented
          label="Theme"
          bind:value={g.ui.theme}
          options={[
            { value: 'dark', label: 'Dark' },
            { value: 'light', label: 'Light' },
            { value: 'system', label: 'Match the device' },
          ]}
        />
      </Field>
      <Field label="Accent color" id="g-accent">
        <div class="accents">
          {#each ACCENTS as a (a)}
            <button
              type="button"
              class="swatch"
              class:sel={g.ui.accent.toLowerCase() === a}
              style:background={a}
              aria-label="Use {a}"
              onclick={() => {
                g.ui.accent = a;
                applyAccent(a);
              }}
            ></button>
          {/each}
          <input
            id="g-accent"
            type="color"
            bind:value={g.ui.accent}
            oninput={(e) => applyAccent(e.currentTarget.value)}
            aria-label="Custom accent color"
          />
        </div>
      </Field>
      <Field label="Density">
        <Segmented
          label="Density"
          bind:value={g.ui.density}
          onchange={(v) => applyDensity(v)}
          options={[
            { value: 'comfortable', label: 'Comfortable' },
            { value: 'compact', label: 'Compact' },
          ]}
        />
      </Field>
      <Field label="Clock">
        <Segmented
          label="Clock"
          bind:value={g.ui.time_format}
          options={[
            { value: '24h', label: '24-hour' },
            { value: '12h', label: '12-hour' },
          ]}
        />
      </Field>
      <Field label="Containers list">
        <Toggle bind:checked={g.ui.group_by_stack} label="Group containers by stack" />
      </Field>
      <Field
        label="App icons"
        id="g-icons"
        hint={'Nodestarr draws a monogram for each app. To use real icons, give the address of an icon set you trust; {name} becomes the app (sonarr, radarr…). Nodestarr never downloads icons on its own.'}
      >
        <input id="g-icons" class="input mono" bind:value={g.ui.icon_url_template} placeholder={'https://icons.example.com/png/{name}.png'} spellcheck="false" />
        <div class="preview">
          {#each ['sonarr', 'radarr', 'plex', 'immich'] as slug (slug)}
            <AppIcon name={slug} {slug} size={28} />
          {/each}
          <span class="faint small">Preview (after saving)</span>
        </div>
      </Field>
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .accents {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .swatch {
    width: 28px;
    height: 28px;
    border: 2px solid transparent;
    border-radius: 50%;
    cursor: pointer;
  }
  .swatch.sel {
    border-color: var(--text);
    box-shadow: 0 0 0 2px var(--bg) inset;
  }
  input[type='color'] {
    width: 40px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-sm);
    background: none;
    cursor: pointer;
  }
  .preview {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .small {
    font-size: var(--fs-sm);
  }
</style>
