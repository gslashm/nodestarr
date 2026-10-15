<!-- Uptime checks: automatic checks of container web UIs, and your own addresses to watch. -->
<script lang="ts">
  import Plus from '@lucide/svelte/icons/plus';
  import X from '@lucide/svelte/icons/x';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, errorMessage } from '../../lib/api';
  import type { CustomCheck } from '../../lib/types';
  import SettingsSection from './SettingsSection.svelte';

  let results = $state<Record<string, { ok: boolean; text: string }>>({});

  function newCheck(): CustomCheck {
    const bytes = crypto.getRandomValues(new Uint8Array(5));
    return { id: Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join(''), name: '', url: '', expect_status: '', keyword: '', enabled: true };
  }

  async function test(c: CustomCheck) {
    results[c.id] = { ok: true, text: 'Checking…' };
    try {
      const r = await api.post<{ ok: boolean; status: number | null; latency_ms: number; error: string | null }>('/uptime/test', {
        url: c.url,
        expect_status: c.expect_status,
        keyword: c.keyword,
      });
      results[c.id] = r.ok ? { ok: true, text: `Healthy in ${r.latency_ms} ms` } : { ok: false, text: r.error ?? `Status ${r.status}` };
    } catch (err) {
      results[c.id] = { ok: false, text: errorMessage(err) };
    }
  }
</script>

<SettingsSection section="uptime" title="Uptime" intro="Nodestarr checks addresses from inside your network and tells you when they stop answering.">
  {#snippet children(u)}
    <Group>
      <Field label="Uptime checks">
        <Toggle bind:checked={u.enabled} label="Check addresses regularly" />
      </Field>
      <Field label="Containers">
        <Toggle bind:checked={u.auto_checks} label="Check every container's web UI on its own" hint="Uses each container's web address; containers can opt out in their settings." />
      </Field>
      <Field label="Check every" id="up-int" hint="Seconds.">
        <input id="up-int" class="input num short" type="number" min="10" bind:value={u.interval_secs} />
      </Field>
      <Field label="Time limit" id="up-to" hint="Seconds to wait for an answer.">
        <input id="up-to" class="input num short" type="number" min="1" bind:value={u.timeout_secs} />
      </Field>
      <Field label="Down after" id="up-fail" hint="Failed checks in a row before an address counts as down (and you're notified).">
        <input id="up-fail" class="input num short" type="number" min="1" bind:value={u.failures_before_down} />
      </Field>
      <Field label="Healthy status codes" id="up-status" hint="Like 200-399, or 200-399,401 for apps that ask for a login.">
        <input id="up-status" class="input mono short-wide" bind:value={u.expect_status} />
      </Field>
      <Field label="Keep results" id="up-ret" hint="Days.">
        <input id="up-ret" class="input num short" type="number" min="1" bind:value={u.retention_days} />
      </Field>
    </Group>

    <Group title="Your own checks" hint="Websites, routers, NAS, anything with an address.">
      <div class="checks">
        {#each u.custom as c, i (c.id)}
          <div class="check">
            <div class="row1">
              <input class="input" bind:value={c.name} placeholder="Name" aria-label="Name" />
              <input class="input mono url" bind:value={c.url} placeholder="https://example.com or tcp://192.168.1.1:53" aria-label="Address" spellcheck="false" />
            </div>
            <div class="row2">
              <input class="input mono" bind:value={c.expect_status} placeholder="Status codes (default)" aria-label="Healthy status codes" />
              <input class="input" bind:value={c.keyword} placeholder="Must contain (optional)" aria-label="Text the page must contain" />
              <label class="on"><input type="checkbox" bind:checked={c.enabled} /> On</label>
              <button type="button" class="btn small ghost" onclick={() => test(c)} disabled={!c.url}>Test</button>
              <button type="button" class="btn small ghost icon" aria-label="Remove {c.name || 'check'}" onclick={() => u.custom.splice(i, 1)}><X size={15} /></button>
            </div>
            {#if results[c.id]}<p class="small {results[c.id].ok ? 'text-ok' : 'text-bad'}">{results[c.id].text}</p>{/if}
          </div>
        {/each}
        <div>
          <button type="button" class="btn small" onclick={() => u.custom.push(newCheck())}><Plus size={15} />Add a check</button>
        </div>
      </div>
    </Group>
  {/snippet}
</SettingsSection>

<style>
  .short {
    max-width: 140px;
  }
  .short-wide {
    max-width: 240px;
  }
  .checks {
    display: grid;
    gap: 10px;
    padding: 10px 0;
  }
  .check {
    display: grid;
    gap: 6px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--line);
  }
  .row1,
  .row2 {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
  }
  .row1 .input {
    flex: 1 1 160px;
  }
  .row1 .url {
    flex: 3 1 280px;
  }
  .row2 .input {
    flex: 1 1 160px;
  }
  .on {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: var(--fs-sm);
  }
  .small {
    margin: 0;
    font-size: var(--fs-sm);
  }
</style>
