<!-- Export all settings and per-container preferences to a file, or import them on another Berth. -->
<script lang="ts">
  import Download from '@lucide/svelte/icons/download';
  import Upload from '@lucide/svelte/icons/upload';
  import Button from '../../components/Button.svelte';
  import Group from '../../components/Group.svelte';
  import Toggle from '../../components/Toggle.svelte';
  import { api, apiUrl, errorMessage, qs } from '../../lib/api';
  import { dateTime, plural } from '../../lib/format';
  import { session } from '../../lib/session.svelte';
  import { ui } from '../../lib/ui.svelte';

  interface ExportDoc {
    berth_export: number;
    version?: string;
    exported_at?: number;
    includes_secrets?: boolean;
    settings?: Record<string, unknown>;
    container_prefs?: Record<string, unknown>;
  }

  let withSecrets = $state(false);
  let doc = $state<ExportDoc | null>(null);
  let fileName = $state('');
  let readError = $state('');
  let importSettings = $state(true);
  let importPrefs = $state(true);
  let importing = $state(false);

  async function pick(e: Event & { currentTarget: HTMLInputElement }) {
    const file = e.currentTarget.files?.[0];
    doc = null;
    readError = '';
    if (!file) return;
    fileName = file.name;
    try {
      const parsed = JSON.parse(await file.text()) as ExportDoc;
      if (parsed.berth_export !== 1) throw new Error("This isn't a Berth settings export.");
      doc = parsed;
    } catch (err) {
      readError = err instanceof SyntaxError ? "The file isn't valid JSON." : errorMessage(err);
    }
  }

  async function runImport() {
    if (!doc) return;
    const ok = await ui.confirm({
      title: 'Import these settings?',
      message: 'The chosen parts replace what Berth has now. Passwords the file leaves out keep their current values.',
      confirmLabel: 'Import',
      danger: true,
    });
    if (!ok) return;
    importing = true;
    try {
      const r = await api.post<{ sections: string[]; container_prefs: number }>(`/settings/import${qs({ settings: importSettings ? 1 : 0, prefs: importPrefs ? 1 : 0 })}`, doc);
      await session.loadSettings();
      ui.toast(`Imported ${plural(r.sections.length, 'settings section')} and ${plural(r.container_prefs, 'container preference')}`, 'success');
      doc = null;
      fileName = '';
    } catch (err) {
      ui.error(err, 'Import failed');
    } finally {
      importing = false;
    }
  }
</script>

<div class="io">
  <header>
    <h2>Import and export</h2>
    <p class="muted">Move Berth's setup to a new machine or keep a copy. Container data is in backups, not here.</p>
  </header>

  {#if !session.isAdmin}
    <p class="muted">Only admins can export or import settings.</p>
  {:else}
  <Group title="Export">
    <div class="block">
      <Toggle bind:checked={withSecrets} label="Include passwords and tokens" hint="Then keep the file as safe as the passwords themselves." />
      <a class="btn" href={apiUrl(`/settings/export${qs({ secrets: withSecrets ? 1 : 0 })}`)} download><Download size={16} />Download settings</a>
    </div>
  </Group>

    <Group title="Import">
      <div class="block">
        <label class="file">
          <input type="file" accept="application/json,.json" onchange={pick} />
          <span class="btn"><Upload size={16} />Choose a file</span>
          <span class="muted small">{fileName}</span>
        </label>
        {#if readError}<p class="text-bad">{readError}</p>{/if}
        {#if doc}
          <p class="muted small">
            From Berth {doc.version ?? '?'}{doc.exported_at ? `, exported ${dateTime(doc.exported_at)}` : ''}; {doc.includes_secrets ? 'includes passwords' : 'without passwords'}.
          </p>
          <Toggle bind:checked={importSettings} label="Settings ({plural(Object.keys(doc.settings ?? {}).length, 'section')})" />
          <Toggle bind:checked={importPrefs} label="Container preferences ({plural(Object.keys(doc.container_prefs ?? {}).length, 'container')})" />
          <div>
            <Button variant="primary" busy={importing} disabled={!importSettings && !importPrefs} onclick={runImport}>Import</Button>
          </div>
        {/if}
      </div>
    </Group>
  {/if}
</div>

<style>
  .io {
    display: grid;
    gap: 16px;
    max-width: 900px;
  }
  header p {
    margin: 4px 0 0;
  }
  .block {
    display: grid;
    gap: 12px;
    justify-items: start;
    padding: 10px 0;
  }
  .file {
    display: flex;
    align-items: center;
    gap: 10px;
    cursor: pointer;
  }
  .file input {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
  }
  .file:focus-within .btn {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }
  .small {
    font-size: var(--fs-sm);
  }
</style>
