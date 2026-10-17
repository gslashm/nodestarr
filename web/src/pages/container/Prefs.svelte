<!--
  Per-container settings: name, icon, links and notes; update policy;
  what and how to back up; the uptime check; and the app integration.
  Empty or "Default" fields follow the global settings.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import AppIcon from '../../components/AppIcon.svelte';
  import AutoInstallEditor from '../../components/AutoInstallEditor.svelte';
  import Button from '../../components/Button.svelte';
  import Field from '../../components/Field.svelte';
  import ListEditor from '../../components/ListEditor.svelte';
  import SecretInput from '../../components/SecretInput.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage, seg } from '../../lib/api';
  import { shortDigest } from '../../lib/format';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { BackupPlan, ContainerDetail, ContainerPrefs, ContainerView, UpdatePolicy } from '../../lib/types';
  import { rulesSummary } from '../../lib/updates';
  import { ui } from '../../lib/ui.svelte';

  let { c, reload }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  let form = $state<ContainerPrefs | null>(null);
  let saved = $state('');
  let loadError = $state('');
  let saving = $state(false);
  let plan = $state<BackupPlan | null>(null);
  let testing = $state(false);
  let testResult = $state<{ ok: boolean; text: string } | null>(null);

  const admin = $derived(session.isAdmin);
  const s = $derived(session.settings);
  const dirty = $derived(!!form && JSON.stringify(form) !== saved);

  const POLICY: Record<UpdatePolicy, string> = { notify: 'Tell me', auto: 'Install automatically', ignore: "Don't check" };
  const STRATEGY: Record<string, string> = {
    auto: 'Automatic (pause apps with databases)',
    pause: 'Pause while copying',
    stop: 'Stop while copying',
    live: 'Copy while running',
  };

  async function load() {
    try {
      const [p, bp] = await Promise.all([
        api.get<ContainerPrefs>(`/containers/${seg(c.id)}/prefs`),
        api.get<{ plan: BackupPlan }>(`/containers/${seg(c.id)}/backup-plan`).catch(() => null),
      ]);
      form = p;
      saved = JSON.stringify(p);
      plan = bp?.plan ?? null;
    } catch (err) {
      loadError = errorMessage(err);
    }
  }

  onMount(load);

  async function save() {
    if (!form || saving) return;
    saving = true;
    try {
      const p = await api.put<ContainerPrefs>(`/containers/${seg(c.id)}/prefs`, form);
      form = p;
      saved = JSON.stringify(p);
      ui.toast('Settings saved', 'success');
      reload();
      void api
        .get<{ plan: BackupPlan }>(`/containers/${seg(c.id)}/backup-plan`)
        .then((r) => (plan = r.plan))
        .catch(() => {});
    } catch (err) {
      ui.error(err, "Couldn't save");
    } finally {
      saving = false;
    }
  }

  function discard() {
    if (saved) form = JSON.parse(saved);
  }

  type MountChoice = 'auto' | 'include' | 'exclude';

  function mountChoice(dest: string): MountChoice {
    const m = form?.backup.mounts.find((x) => x.destination === dest);
    return m ? (m.include ? 'include' : 'exclude') : 'auto';
  }

  function setMount(dest: string, choice: MountChoice) {
    if (!form) return;
    const rest = form.backup.mounts.filter((x) => x.destination !== dest);
    form.backup.mounts = choice === 'auto' ? rest : [...rest, { destination: dest, include: choice === 'include' }];
  }

  async function testUptime() {
    if (!form) return;
    const url = form.uptime.url || c.links[0]?.url?.replaceAll('{host}', s?.general.public_host || location.hostname);
    if (!url) {
      testResult = { ok: false, text: 'Enter an address to check.' };
      return;
    }
    testing = true;
    testResult = null;
    try {
      const r = await api.post<{ ok: boolean; status: number | null; latency_ms: number; error: string | null }>('/uptime/test', {
        url,
        expect_status: form.uptime.expect_status || s?.uptime.expect_status || '',
        keyword: form.uptime.keyword,
      });
      testResult = r.ok
        ? { ok: true, text: `Answered${r.status ? ` with ${r.status}` : ''} in ${r.latency_ms} ms` }
        : { ok: false, text: r.error ?? `Got ${r.status ?? 'no answer'}` };
    } catch (err) {
      testResult = { ok: false, text: errorMessage(err) };
    } finally {
      testing = false;
    }
  }

  function numberOrNull(v: string): number | null {
    const n = Number(v);
    return v.trim() === '' || !Number.isFinite(n) ? null : n;
  }
</script>

{#if loadError}
  <p class="text-bad">{loadError}</p>
{:else if !form}
  <p class="faint"><Spinner size={13} /> Loading…</p>
{:else}
  <form
    class="prefs"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <fieldset disabled={!admin}>
      <section class="panel">
        <div class="panel-head"><h2>General</h2></div>
        <div class="panel-body">
          <Field label="Name" id="p-name" hint="How it's shown in Nodestarr. Leave empty for the automatic name.">
            <input id="p-name" class="input" bind:value={form.display_name} placeholder={c.display_name} />
          </Field>
          <Field label="Icon" id="p-icon" hint="An image URL, or the name of an app icon if an icon source is set in Settings > General.">
            <div class="with-preview">
              <input id="p-icon" class="input" bind:value={form.icon} placeholder="https://… or sonarr" />
              <AppIcon name={form.display_name || c.display_name} slug={c.app} icon={form.icon} size={34} />
            </div>
          </Field>
          <Field label="Web address" id="p-web" hint={'Where "Open" goes. {host} is replaced with the address you use for Nodestarr.'}>
            <input id="p-web" class="input mono" bind:value={form.web_url} placeholder={c.links[0]?.url ?? 'http://{host}:8080'} />
          </Field>
          <Field label="Tags" hint="For filtering and grouping.">
            <ListEditor bind:items={form.tags} placeholder="media" addLabel="Add tag" />
          </Field>
          <Field label="Notes" id="p-notes" hint="Shown on the overview.">
            <textarea id="p-notes" class="input" rows="3" bind:value={form.notes}></textarea>
          </Field>
          <Field label="Visibility">
            <div class="toggles">
              <Toggle bind:checked={form.hidden} label="Hide from lists" hint="It's still monitored; show hidden containers from the Containers page." />
              <Toggle bind:checked={form.alerts_muted} label="Mute alerts" hint="No resource or crash alerts for this container." />
            </div>
          </Field>
        </div>
      </section>

      <section class="panel">
        <div class="panel-head"><h2>Updates</h2></div>
        <div class="panel-body">
          <Field label="When there's an update" id="p-policy">
            <select id="p-policy" class="input" bind:value={form.update.policy}>
              <option value={null}>Default ({POLICY[s?.updates.default_policy ?? 'notify']})</option>
              <option value="notify">{POLICY.notify}</option>
              <option value="auto">{POLICY.auto}</option>
              <option value="ignore">{POLICY.ignore}</option>
            </select>
          </Field>
          {#if (form.update.policy ?? s?.updates.default_policy) === 'auto'}
            <Field label="Install on their own" hint="Updates of other types wait for you on the Updates page.">
              <Toggle
                checked={form.update.auto_install === null}
                label="Use the default"
                hint={s ? rulesSummary(s.updates.auto_install) : ''}
                onchange={(useDefault) => form && (form.update.auto_install = useDefault ? null : { ...(s?.updates.auto_install ?? c.auto_install) })}
              />
              {#if form.update.auto_install}
                <div class="own-rules"><AutoInstallEditor bind:value={form.update.auto_install} /></div>
              {/if}
            </Field>
            <Field label="Wait before installing" id="p-age" hint="Hours a release must be out before it's installed automatically. Empty uses the default.">
              <input
                id="p-age"
                class="input num short"
                type="number"
                min="0"
                value={form.update.min_age_hours ?? ''}
                placeholder={String(s?.updates.min_age_hours ?? 0)}
                oninput={(e) => form && (form.update.min_age_hours = numberOrNull(e.currentTarget.value))}
              />
            </Field>
          {/if}
          <Field label="Hold">
            <Toggle bind:checked={form.update.held} label="Hold automatic updates" hint="Nothing is installed on its own until you release the hold. You can still update by hand." />
          </Field>
          {#if form.update.skipped_digest}
            <Field label="Skipped build">
              <div class="row-inline">
                <span class="mono">{shortDigest(form.update.skipped_digest)}</span>
                <button type="button" class="btn small ghost" onclick={() => form && (form.update.skipped_digest = '')}>Offer it again</button>
              </div>
            </Field>
          {/if}
          {#if form.update.rolled_back_digest}
            <Field label="Rolled back build" hint="Automatic updates don't install it again. You can still update to it by hand.">
              <div class="row-inline">
                <span class="mono">{shortDigest(form.update.rolled_back_digest)}</span>
                <button type="button" class="btn small ghost" onclick={() => form && (form.update.rolled_back_digest = '')}>Let automatic updates install it</button>
              </div>
            </Field>
          {/if}
        </div>
      </section>

      <section class="panel">
        <div class="panel-head">
          <h2>Backups</h2>
          <a class="faint small" href={href(`/containers/${encodeURIComponent(c.name)}/backups`)}>See backups</a>
        </div>
        <div class="panel-body">
          <Field label="Scheduled backups" id="p-bk">
            <select id="p-bk" class="input" bind:value={form.backup.enabled}>
              <option value={null}>Default ({s?.backups.include_all_by_default ? 'included' : 'not included'})</option>
              <option value={true}>Include in scheduled backups</option>
              <option value={false}>Leave out of scheduled backups</option>
            </select>
          </Field>
          <Field label="Consistency" id="p-strategy" hint="Databases need the app paused or stopped for a consistent copy.">
            <select id="p-strategy" class="input" bind:value={form.backup.strategy}>
              <option value={null}>Default ({STRATEGY[s?.backups.default_strategy ?? 'auto']})</option>
              {#each Object.entries(STRATEGY) as [k, label] (k)}<option value={k}>{label}</option>{/each}
            </select>
          </Field>
          {#if plan?.native || form.backup.use_native !== null}
            <Field label="App's own backup" id="p-native" hint="Some apps can make a backup themselves through their API. It's usually the safest choice.">
              <select id="p-native" class="input" bind:value={form.backup.use_native}>
                <option value={null}>Automatic</option>
                <option value={true}>Use the app's backup</option>
                <option value={false}>Copy the files instead</option>
              </select>
            </Field>
          {/if}
          <Field label="Keep" id="p-keep" hint="How many backups of this container to keep. Empty uses the default.">
            <input
              id="p-keep"
              class="input num short"
              type="number"
              min="1"
              value={form.backup.retention_count ?? ''}
              placeholder={String(s?.backups.retention_count ?? 7)}
              oninput={(e) => form && (form.backup.retention_count = numberOrNull(e.currentTarget.value))}
            />
          </Field>
          {#if plan?.mounts.length}
            <Field label="Folders and volumes" hint="Automatic picks config and skips media, system paths and database files that are dumped instead.">
              <ul class="mounts">
                {#each plan.mounts as m (m.destination)}
                  {@const choice = mountChoice(m.destination)}
                  <li>
                    <div class="what">
                      <span class="mono">{m.destination}</span>
                      <span class="faint small">{choice === 'auto' ? `Automatic: ${m.include ? 'included' : 'skipped'}, ${m.reason}` : m.source}</span>
                    </div>
                    <Segmented
                      label="Back up {m.destination}"
                      value={choice}
                      onchange={(v) => setMount(m.destination, v)}
                      options={[
                        { value: 'auto', label: 'Auto' },
                        { value: 'include', label: 'Include' },
                        { value: 'exclude', label: 'Skip' },
                      ]}
                    />
                  </li>
                {/each}
              </ul>
            </Field>
          {/if}
          <Field label="Skip files matching" hint="Glob patterns relative to each folder, like cache/** or *.log. Added to the global list.">
            <ListEditor bind:items={form.backup.exclude} placeholder="logs/**" mono addLabel="Add pattern" />
          </Field>
        </div>
      </section>

      <section class="panel">
        <div class="panel-head"><h2>Uptime check</h2></div>
        <div class="panel-body">
          <Field label="Check the web UI" id="p-up">
            <select id="p-up" class="input" bind:value={form.uptime.enabled}>
              <option value={null}>Automatic ({s?.uptime.auto_checks ? 'on when it has a web UI' : 'off'})</option>
              <option value={true}>On</option>
              <option value={false}>Off</option>
            </select>
          </Field>
          <Field label="Address" id="p-up-url" hint="Empty uses its web address.">
            <input id="p-up-url" class="input mono" bind:value={form.uptime.url} placeholder={c.links[0]?.url ?? 'http://{host}:8080/'} />
          </Field>
          <Field label="Healthy status codes" id="p-up-status" hint="Like 200-399 or 200,401. Empty uses the default.">
            <input id="p-up-status" class="input mono short" bind:value={form.uptime.expect_status} placeholder={s?.uptime.expect_status ?? '200-399'} />
          </Field>
          <Field label="Page must contain" id="p-up-kw" hint="Optional text the page has to include.">
            <input id="p-up-kw" class="input" bind:value={form.uptime.keyword} />
          </Field>
          <div class="test">
            <Button small onclick={testUptime} busy={testing}>Test now</Button>
            {#if testResult}<span class={testResult.ok ? 'text-ok' : 'text-bad'}>{testResult.text}</span>{/if}
          </div>
        </div>
      </section>

      <section class="panel">
        <div class="panel-head"><h2>App integration</h2></div>
        <div class="panel-body">
          <p class="muted intro">
            {#if c.app_name}
              Nodestarr talks to {c.app_name}'s API for its version, health warnings and activity.
            {:else}
              Nodestarr doesn't recognize this app, so there's nothing to connect to unless you set it up below.
            {/if}
          </p>
          <Field label="Integration" id="p-int">
            <select id="p-int" class="input" bind:value={form.integration.enabled}>
              <option value={null}>Automatic</option>
              <option value={true}>On</option>
              <option value={false}>Off</option>
            </select>
          </Field>
          <Field label="API address" id="p-int-url" hint="Empty finds it on its own (Docker network, then published ports).">
            <input id="p-int-url" class="input mono" bind:value={form.integration.base_url} placeholder="http://sonarr:8989" />
          </Field>
          <Field label="API key" id="p-int-key" hint="Usually read from the app's config file automatically.">
            <SecretInput id="p-int-key" bind:value={form.integration.api_key} label="API key" />
          </Field>
          <Field label="Username" id="p-int-user">
            <input id="p-int-user" class="input" bind:value={form.integration.username} autocomplete="off" />
          </Field>
          <Field label="Password" id="p-int-pass">
            <SecretInput id="p-int-pass" bind:value={form.integration.password} label="Password" />
          </Field>
        </div>
      </section>
    </fieldset>

    {#if admin && dirty}
      <div class="savebar">
        <span>You have unsaved changes.</span>
        <span class="grow"></span>
        <button type="button" class="btn ghost" onclick={discard}>Discard</button>
        <Button type="submit" variant="primary" busy={saving}>Save</Button>
      </div>
    {/if}
  </form>
{/if}

<style>
  .prefs fieldset {
    display: grid;
    gap: 16px;
    max-width: 980px;
    margin: 0;
    padding: 0;
    border: 0;
    min-width: 0;
  }
  .prefs .panel-body {
    padding-top: 4px;
    padding-bottom: 8px;
  }
  .with-preview {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .toggles {
    display: grid;
    gap: 14px;
    padding-top: 6px;
  }
  .short {
    max-width: 160px;
  }
  .row-inline {
    display: flex;
    align-items: center;
    gap: 10px;
    padding-top: 6px;
  }
  .own-rules {
    margin-top: 6px;
    padding: 10px 0 2px 14px;
    border-left: 2px solid var(--line-strong);
  }
  .small {
    font-size: var(--fs-sm);
  }
  .mounts {
    display: grid;
    gap: 12px;
    margin: 0;
    padding: 6px 0 0;
    list-style: none;
  }
  .mounts li {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 12px;
  }
  .what {
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .test {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 0 8px;
  }
  .intro {
    margin: 10px 0 0;
  }
  .savebar {
    position: sticky;
    bottom: 12px;
    z-index: 5;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: 980px;
    margin-top: 16px;
    padding: 10px 14px;
    border: 1px solid var(--accent);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
</style>
