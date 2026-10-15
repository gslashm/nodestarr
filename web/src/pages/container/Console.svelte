<!--
  A shell inside the container (interactive terminal), or a single command
  run with its output captured.
-->
<script lang="ts">
  import CornerDownLeft from '@lucide/svelte/icons/corner-down-left';
  import Play from '@lucide/svelte/icons/play';
  import SquareTerminal from '@lucide/svelte/icons/square-terminal';
  import Button from '../../components/Button.svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import EmptyState from '../../components/EmptyState.svelte';
  import Segmented from '../../components/Segmented.svelte';
  import Terminal from '../../components/Terminal.svelte';
  import { runAction } from '../../lib/actions';
  import { api, errorMessage, seg } from '../../lib/api';
  import { fillHeight } from '../../lib/fill';
  import { isUp, stateOf } from '../../lib/format';
  import { session } from '../../lib/session.svelte';
  import type { ContainerDetail, ContainerView } from '../../lib/types';

  let { c, detail }: { c: ContainerView; detail: ContainerDetail | null; reload: () => void } = $props();

  type Mode = 'shell' | 'command';
  let mode = $state<Mode>('shell');
  let shell = $state('');
  let user = $state('');
  /** Bumped to start a fresh terminal session with the chosen shell and user. */
  let termKey = $state(0);

  let command = $state('');
  let running = $state(false);
  let result = $state<{ command: string; exit_code: number | null; stdout: string; stderr: string } | null>(null);
  let runError = $state('');
  let history = $state<string[]>([]);

  const up = $derived(isUp(stateOf(c)));
  const shells = $derived(session.settings?.docker.console_shells ?? ['/bin/bash', '/bin/sh']);

  async function run() {
    const cmd = command.trim();
    if (!cmd || running) return;
    running = true;
    runError = '';
    try {
      const out = await api.post<{ exit_code: number | null; stdout: string; stderr: string }>(`/containers/${seg(c.id)}/exec`, {
        command: cmd,
        user: user || null,
      });
      result = { command: cmd, ...out };
      history = [cmd, ...history.filter((h) => h !== cmd)].slice(0, 10);
    } catch (err) {
      runError = errorMessage(err);
    } finally {
      running = false;
    }
  }
</script>

{#if !session.isAdmin}
  <EmptyState icon={SquareTerminal} title="Only admins can open a console">A console gives full control of the container.</EmptyState>
{:else if !up}
  <EmptyState icon={SquareTerminal} title="{c.display_name} isn't running">
    A console needs a running container.
    {#snippet actions()}
      {#if stateOf(c) === 'paused'}
        <Button variant="primary" icon={Play} onclick={() => runAction(c, 'unpause')}>Resume</Button>
      {:else}
        <Button variant="primary" icon={Play} onclick={() => runAction(c, 'start')}>Start</Button>
      {/if}
    {/snippet}
  </EmptyState>
{:else}
  <div class="console">
    <div class="toolbar">
      <Segmented
        label="Console mode"
        bind:value={mode}
        options={[
          { value: 'shell', label: 'Shell' },
          { value: 'command', label: 'Run a command' },
        ]}
      />
      {#if mode === 'shell'}
        <label class="opt">
          <span>Shell</span>
          <select class="input" bind:value={shell} onchange={() => termKey++}>
            <option value="">Automatic</option>
            {#each shells as s (s)}<option value={s}>{s}</option>{/each}
          </select>
        </label>
      {/if}
      <label class="opt">
        <span>User</span>
        <input
          class="input mono"
          placeholder={detail?.user || 'default'}
          bind:value={user}
          onchange={() => {
            if (mode === 'shell') termKey++;
          }}
          spellcheck="false"
          autocomplete="off"
        />
      </label>
    </div>

    {#if mode === 'shell'}
      <div class="term" use:fillHeight={{ bottom: 18, min: 320 }}>
        {#key termKey}
          <Terminal container={c.id} {shell} {user} />
        {/key}
      </div>
    {:else}
      <form
        class="cmd"
        onsubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <span class="prompt mono">$</span>
        <input
          class="input mono"
          bind:value={command}
          placeholder="ls -la /config"
          aria-label="Command"
          list="cmd-history"
          spellcheck="false"
          autocomplete="off"
        />
        <datalist id="cmd-history">
          {#each history as h (h)}<option value={h}></option>{/each}
        </datalist>
        <Button variant="primary" type="submit" icon={CornerDownLeft} busy={running} disabled={!command.trim()}>Run</Button>
      </form>
      <p class="faint hint">Runs with <code>sh -c</code> and stops after 2 minutes. Use the shell for anything interactive.</p>
      {#if runError}<p class="text-bad">{runError}</p>{/if}
      {#if result}
        <section class="panel out">
          <div class="panel-head">
            <h3 class="mono ellipsis">$ {result.command}</h3>
            <span class="chip {result.exit_code === 0 ? 'ok' : 'bad'}">exit {result.exit_code ?? '?'}</span>
            <CopyButton text={() => [result?.stdout, result?.stderr].filter(Boolean).join('\n')} label="Copy output" />
          </div>
          {#if result.stdout}<pre class="stdout">{result.stdout}</pre>{/if}
          {#if result.stderr}<pre class="stderr">{result.stderr}</pre>{/if}
          {#if !result.stdout && !result.stderr}<p class="faint none">No output.</p>{/if}
        </section>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .console {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .toolbar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 16px;
  }
  .opt {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .opt .input {
    width: 150px;
    min-height: 32px;
    padding-top: 4px;
    padding-bottom: 4px;
  }
  .term {
    display: flex;
    flex-direction: column;
  }
  .term :global(.console) {
    flex: 1;
  }
  .cmd {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .prompt {
    color: var(--accent-text);
    font-size: 15px;
  }
  .hint {
    margin: -4px 0 0;
    font-size: var(--fs-sm);
  }
  .out pre {
    max-height: 60vh;
    overflow: auto;
    padding: 12px 16px;
    font-size: 12.5px;
    line-height: 1.5;
  }
  .stderr {
    border-top: 1px solid var(--line);
    color: color-mix(in srgb, var(--bad) 70%, var(--text));
  }
  .none {
    margin: 0;
    padding: 12px 16px;
  }
  .chip.ok {
    color: var(--ok);
  }
  .chip.bad {
    color: var(--bad);
  }
</style>
