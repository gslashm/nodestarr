<!--
  Interactive shell in a container (xterm.js over a WebSocket, loaded on
  first use). The server picks the first shell that exists unless one is
  given.
-->
<script lang="ts">
  import { onDestroy } from 'svelte';
  import type { Terminal as XTerm } from '@xterm/xterm';
  import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
  import { qs, seg, wsUrl } from '../lib/api';
  import Spinner from './Spinner.svelte';

  interface Props {
    container: string;
    shell?: string;
    user?: string;
  }

  let { container, shell = '', user = '' }: Props = $props();

  let host: HTMLDivElement | undefined = $state();
  let term: XTerm | null = null;
  let fit: { fit: () => void } | null = null;
  let socket: WebSocket | null = null;
  let status = $state<'loading' | 'connecting' | 'open' | 'closed' | 'error'>('loading');
  let message = $state('');
  let usedShell = $state('');
  let ro: ResizeObserver | null = null;

  function css(name: string): string {
    return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  }

  async function init() {
    const [{ Terminal }, { FitAddon }, { WebLinksAddon }] = await Promise.all([
      import('@xterm/xterm'),
      import('@xterm/addon-fit'),
      import('@xterm/addon-web-links'),
      import('@xterm/xterm/css/xterm.css'),
    ]);
    if (!host) return;
    term = new Terminal({
      cursorBlink: true,
      fontFamily: '"IBM Plex Mono", ui-monospace, monospace',
      fontSize: 13,
      lineHeight: 1.25,
      scrollback: 5000,
      allowProposedApi: false,
      theme: {
        background: css('--sunken'),
        foreground: css('--text'),
        cursor: css('--accent'),
        selectionBackground: 'rgba(242, 194, 48, 0.3)',
      },
    });
    const fitAddon = new FitAddon();
    fit = fitAddon;
    term.loadAddon(fitAddon);
    term.loadAddon(new WebLinksAddon());
    term.open(host);
    fitAddon.fit();
    term.onData((data) => send({ type: 'input', data }));
    term.onResize(({ cols, rows }) => send({ type: 'resize', cols, rows }));
    ro = new ResizeObserver(() => fit?.fit());
    ro.observe(host);
    connect();
  }

  function send(msg: unknown) {
    if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(msg));
  }

  function connect() {
    if (!term) return;
    socket?.close();
    status = 'connecting';
    message = '';
    term.reset();
    const url = wsUrl(`/containers/${seg(container)}/console${qs({ shell, user, cols: term.cols, rows: term.rows })}`);
    const ws = new WebSocket(url);
    ws.binaryType = 'arraybuffer';
    socket = ws;
    ws.onmessage = (e) => {
      if (e.data instanceof ArrayBuffer) {
        term?.write(new Uint8Array(e.data));
        return;
      }
      try {
        const msg = JSON.parse(e.data as string) as { type: string; shell?: string; message?: string; code?: number | null };
        if (msg.type === 'ready') {
          status = 'open';
          usedShell = msg.shell ?? '';
          term?.focus();
        } else if (msg.type === 'error') {
          status = 'error';
          message = msg.message ?? 'The console could not start.';
        } else if (msg.type === 'exit') {
          status = 'closed';
          message = `The shell exited${msg.code !== null && msg.code !== undefined ? ` with code ${msg.code}` : ''}.`;
        }
      } catch {
        term?.write(e.data as string);
      }
    };
    ws.onclose = (e) => {
      if (socket !== ws) return;
      if (status === 'connecting') {
        status = 'error';
        message = e.code === 1006 ? 'Could not connect. The container may have stopped, or you may need an admin account.' : `Connection closed (${e.code}).`;
      } else if (status === 'open') {
        status = 'closed';
        message = 'The connection closed.';
      }
    };
  }

  $effect(() => {
    if (host && !term) void init();
  });

  onDestroy(() => {
    ro?.disconnect();
    socket?.close();
    term?.dispose();
  });
</script>

<div class="console">
  <div class="status">
    {#if status === 'loading' || status === 'connecting'}
      <Spinner size={13} /> <span>Connecting…</span>
    {:else if status === 'open'}
      <span class="dot ok"></span> <span>Connected{usedShell ? ` (${usedShell})` : ''}</span>
    {:else}
      <span class="dot {status === 'error' ? 'bad' : ''}"></span> <span>{message}</span>
      <button type="button" class="btn small" onclick={connect}><RotateCcw size={14} />Reconnect</button>
    {/if}
  </div>
  <div class="term" bind:this={host}></div>
</div>

<style>
  .console {
    display: flex;
    flex-direction: column;
    gap: 8px;
    height: 100%;
  }
  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 28px;
    color: var(--text-2);
    font-size: var(--fs-sm);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--idle);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.bad {
    background: var(--bad);
  }
  .term {
    flex: 1;
    min-height: 320px;
    padding: 8px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    overflow: hidden;
  }
  .term :global(.xterm) {
    height: 100%;
  }
  .term :global(.xterm-viewport) {
    background: transparent !important;
  }
</style>
