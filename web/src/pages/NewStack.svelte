<!--
  Create a compose stack: a name, a compose file (typed, pasted, or
  converted from a docker run command) and an optional .env file. The files
  go into a new folder in the first stacks directory.
-->
<script lang="ts">
  import Rocket from '@lucide/svelte/icons/rocket';
  import Save from '@lucide/svelte/icons/save';
  import Wand from '@lucide/svelte/icons/wand-sparkles';
  import Button from '../components/Button.svelte';
  import CodeEditor from '../components/CodeEditor.svelte';
  import Field from '../components/Field.svelte';
  import PageHeader from '../components/PageHeader.svelte';
  import { api, errorMessage } from '../lib/api';
  import { href, router } from '../lib/router.svelte';
  import { session } from '../lib/session.svelte';
  import type { ContainerSpec } from '../lib/types';
  import { ui } from '../lib/ui.svelte';

  let { params: _params }: { params: Record<string, string> } = $props();

  const TEMPLATE = `services:
  app:
    image: nginx:latest
    restart: unless-stopped
    ports:
      - "8080:80"
    volumes:
      - ./data:/usr/share/nginx/html
    environment:
      TZ: \${TZ:-UTC}
`;

  let name = $state('');
  let compose = $state(TEMPLATE);
  let env = $state('');
  let showEnv = $state(false);
  let runCommand = $state('');
  let converting = $state(false);
  let creating = $state(false);
  let error = $state('');

  const stacksDir = $derived(session.settings?.docker.stacks_dirs[0] ?? '');
  const nameOk = $derived(/^[a-z0-9][a-z0-9_-]*$/.test(name));

  async function convert() {
    const cmd = runCommand.trim();
    if (!cmd) return;
    converting = true;
    try {
      const parsed = await api.post<{ spec: ContainerSpec; warnings: string[] }>('/containers/parse-run', { command: cmd });
      const r = await api.post<{ yaml: string }>('/containers/to-compose', parsed.spec);
      compose = r.yaml;
      if (!name && parsed.spec.name) name = parsed.spec.name.toLowerCase().replace(/[^a-z0-9_-]/g, '-');
      if (parsed.warnings.length) ui.toast(`Converted, but some options were left out: ${parsed.warnings.join('; ')}`, 'warning');
      else ui.toast('Converted to a compose file', 'success');
    } catch (err) {
      ui.error(err, "Couldn't convert it");
    } finally {
      converting = false;
    }
  }

  async function create(deploy: boolean) {
    if (!nameOk || creating) return;
    creating = true;
    error = '';
    try {
      const r = await api.post<{ task_id: string | null }>('/stacks', { name, compose, env: env.trim() ? env : null, deploy });
      if (r.task_id) ui.taskStarted(r.task_id, `Deploying ${name}`);
      else ui.toast(`Stack ${name} created`, 'success');
      router.navigate(`/stacks/${encodeURIComponent(name)}`);
    } catch (err) {
      error = errorMessage(err);
    } finally {
      creating = false;
    }
  }
</script>

<PageHeader title="Create a stack" back={{ label: 'Stacks', path: '/stacks' }} />

<div class="page-body">
  {#if !session.isAdmin}
    <p class="muted">Only admins can create stacks.</p>
  {:else}
    <form
      class="form"
      onsubmit={(e) => {
        e.preventDefault();
        void create(true);
      }}
    >
      <section class="panel">
        <div class="panel-body">
          <Field
            label="Name"
            id="st-name"
            hint={stacksDir ? `Files go in ${stacksDir}/${name || 'name'}. Lowercase letters, digits, - and _.` : 'Set a stacks folder in Settings > Docker first.'}
            error={name && !nameOk ? 'Use lowercase letters, digits, - and _, starting with a letter or digit.' : ''}
          >
            <input id="st-name" class="input mono" bind:value={name} placeholder="media" autocomplete="off" spellcheck="false" required />
          </Field>
        </div>
      </section>

      <details class="panel convert">
        <summary class="panel-head"><Wand size={16} /><h2>Start from a docker run command</h2></summary>
        <div class="panel-body">
          <textarea class="input mono" rows="4" bind:value={runCommand} placeholder="docker run -d --name=sonarr -p 8989:8989 -v /opt/appdata/sonarr:/config lscr.io/linuxserver/sonarr:latest" spellcheck="false"></textarea>
          <div class="row-end">
            <Button small icon={Wand} busy={converting} onclick={convert} disabled={!runCommand.trim()}>Convert to compose</Button>
          </div>
        </div>
      </details>

      <section class="panel">
        <div class="panel-head">
          <h2 class="mono">compose.yaml</h2>
          <button type="button" class="btn small ghost" onclick={() => (showEnv = !showEnv)}>{showEnv ? 'Hide' : 'Add'} .env file</button>
        </div>
        <div class="panel-body">
          <CodeEditor bind:value={compose} language="yaml" height="420px" label="compose.yaml" onsave={() => create(false)} />
        </div>
      </section>

      {#if showEnv}
        <section class="panel">
          <div class="panel-head"><h2 class="mono">.env</h2><span class="faint small">Variables used as $&#123;NAME&#125; in the compose file. Keep secrets here.</span></div>
          <div class="panel-body">
            <CodeEditor bind:value={env} language="ini" height="200px" label=".env" />
          </div>
        </section>
      {/if}

      {#if error}<pre class="error">{error}</pre>{/if}

      <div class="actions">
        <a class="btn ghost" href={href('/stacks')}>Cancel</a>
        <Button icon={Save} busy={creating} onclick={() => create(false)} disabled={!nameOk || !stacksDir}>Create only</Button>
        <Button type="submit" variant="primary" icon={Rocket} busy={creating} disabled={!nameOk || !stacksDir}>Create and deploy</Button>
      </div>
    </form>
  {/if}
</div>

<style>
  .form {
    display: grid;
    gap: 16px;
    max-width: 1000px;
  }
  .form .panel-body {
    padding-top: 6px;
  }
  .convert > summary {
    cursor: pointer;
    list-style: none;
    color: var(--text-2);
  }
  .convert > summary::-webkit-details-marker {
    display: none;
  }
  .convert:not([open]) > summary {
    border-bottom: 0;
  }
  .convert .panel-body {
    display: grid;
    gap: 10px;
    padding-top: 12px;
  }
  .row-end {
    display: flex;
    justify-content: flex-end;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .error {
    padding: 10px 12px;
    border: 1px solid color-mix(in srgb, var(--bad) 50%, var(--line));
    border-radius: var(--radius);
    background: color-mix(in srgb, var(--bad) 8%, var(--panel));
    font-size: 12.5px;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
  }
</style>
