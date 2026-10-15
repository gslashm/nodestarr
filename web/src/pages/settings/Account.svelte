<!--
  The signed-in user's own account: password, two-step sign-in (TOTP with
  recovery codes), and where they're signed in.
-->
<script lang="ts">
  import { onMount } from 'svelte';
  import { renderSVG } from 'uqr';
  import KeyRound from '@lucide/svelte/icons/key-round';
  import ShieldCheck from '@lucide/svelte/icons/shield-check';
  import Button from '../../components/Button.svelte';
  import CopyButton from '../../components/CopyButton.svelte';
  import Field from '../../components/Field.svelte';
  import Group from '../../components/Group.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { api, errorMessage } from '../../lib/api';
  import { session } from '../../lib/session.svelte';
  import type { SessionInfo } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  const me = $derived(session.me);

  // Password
  let current = $state('');
  let next = $state('');
  let confirm = $state('');
  let pwBusy = $state(false);
  let pwError = $state('');
  const pwValid = $derived(next.length >= 8 && next === confirm && current.length > 0);

  async function changePassword() {
    if (!pwValid) return;
    pwBusy = true;
    pwError = '';
    try {
      await api.post('/auth/password', { current, new: next });
      current = next = confirm = '';
      ui.toast('Password changed. Other sessions were signed out.', 'success');
      void loadSessions();
    } catch (err) {
      pwError = errorMessage(err);
    } finally {
      pwBusy = false;
    }
  }

  // Two-step sign-in
  let setup = $state<{ secret: string; uri: string } | null>(null);
  let code = $state('');
  let recovery = $state<string[] | null>(null);
  let totpBusy = $state(false);
  let totpError = $state('');
  let disablePassword = $state('');

  const qr = $derived(setup ? renderSVG(setup.uri, { pixelSize: 5, whiteColor: '#ffffff', blackColor: '#000000' }) : '');

  async function startSetup() {
    totpError = '';
    try {
      setup = await api.post<{ secret: string; uri: string }>('/auth/2fa/setup');
    } catch (err) {
      totpError = errorMessage(err);
    }
  }

  async function enable() {
    totpBusy = true;
    totpError = '';
    try {
      const r = await api.post<{ recovery_codes: string[] }>('/auth/2fa/enable', { code: code.replace(/\s/g, '') });
      recovery = r.recovery_codes;
      setup = null;
      code = '';
      await session.loadUser();
    } catch (err) {
      totpError = errorMessage(err);
    } finally {
      totpBusy = false;
    }
  }

  async function disable() {
    totpBusy = true;
    totpError = '';
    try {
      await api.post('/auth/2fa/disable', { password: disablePassword });
      disablePassword = '';
      recovery = null;
      await session.loadUser();
      ui.toast('Two-step sign-in turned off', 'success');
    } catch (err) {
      totpError = errorMessage(err);
    } finally {
      totpBusy = false;
    }
  }

  function downloadCodes() {
    if (!recovery) return;
    const blob = new Blob([`Nodestarr recovery codes for ${me?.username}\nEach code works once.\n\n${recovery.join('\n')}\n`], { type: 'text/plain' });
    const a = document.createElement('a');
    a.href = URL.createObjectURL(blob);
    a.download = 'nodestarr-recovery-codes.txt';
    a.click();
    URL.revokeObjectURL(a.href);
  }

  // Sessions
  let sessions = $state<SessionInfo[] | null>(null);

  async function loadSessions() {
    try {
      sessions = await api.get<SessionInfo[]>('/auth/sessions');
    } catch {
      sessions = [];
    }
  }

  onMount(loadSessions);

  async function revoke(s: SessionInfo) {
    try {
      await api.del(`/auth/sessions/${encodeURIComponent(s.id)}`);
      sessions = (sessions ?? []).filter((x) => x.id !== s.id);
    } catch (err) {
      ui.error(err);
    }
  }

  async function revokeOthers() {
    try {
      const r = await api.del<{ ended: number }>('/auth/sessions');
      sessions = (sessions ?? []).filter((x) => x.current);
      ui.toast(r.ended === 1 ? 'Signed out of 1 other session' : `Signed out of ${r.ended} other sessions`, 'success');
    } catch (err) {
      ui.error(err);
    }
  }

  /** "Firefox on Windows" from a user agent string, roughly. */
  function device(ua: string | null): string {
    if (!ua) return 'Unknown device';
    const browser = /Edg\//.test(ua) ? 'Edge' : /Firefox\//.test(ua) ? 'Firefox' : /Chrome\//.test(ua) ? 'Chrome' : /Safari\//.test(ua) ? 'Safari' : /curl|python|Go-http/i.test(ua) ? 'Script' : 'Browser';
    const os = /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Windows/.test(ua) ? 'Windows' : /Mac OS X/.test(ua) ? 'macOS' : /Linux/.test(ua) ? 'Linux' : '';
    return os ? `${browser} on ${os}` : browser;
  }

  /** How the current request is authenticated, for sign-ins without a Nodestarr session. */
  const NO_SESSION: Record<string, string> = {
    api_key: "You're using an API key, which has no password or sessions.",
    external: 'Your reverse proxy signs you in, so there is no Nodestarr password or session to manage here.',
    local_bypass: "You're on a local network that Nodestarr lets in without signing in (see Sign-in and security), so there's no password or session to manage.",
  };
  const ROLE_TEXT = (role: string) => (role === 'admin' ? 'admin' : 'viewer, read-only');
</script>

<div class="account">
  <header>
    <h2>Your account</h2>
    {#if me}
      <p class="muted">
        {#if me.via === 'api_key'}Using the API key <strong>{me.username}</strong> ({ROLE_TEXT(me.role)}).
        {:else if me.via === 'local_bypass'}Let in from the local network as <strong>{me.username}</strong> ({ROLE_TEXT(me.role)}).
        {:else}Signed in as <strong>{me.username}</strong> ({ROLE_TEXT(me.role)}){me.via === 'external' ? ' by your reverse proxy' : ''}.{/if}
      </p>
    {/if}
  </header>

  {#if me?.can_change_password}
    <Group title="Password">
      <form
        onsubmit={(e) => {
          e.preventDefault();
          void changePassword();
        }}
      >
        <Field label="Current password" id="a-cur">
          <input id="a-cur" class="input" type="password" bind:value={current} autocomplete="current-password" />
        </Field>
        <Field label="New password" id="a-new" hint="At least 8 characters. A passphrase of a few words is easy to remember and hard to guess.">
          <input id="a-new" class="input" type="password" bind:value={next} autocomplete="new-password" />
        </Field>
        <Field label="Repeat it" id="a-rep" error={confirm && next !== confirm ? "The passwords don't match." : ''}>
          <input id="a-rep" class="input" type="password" bind:value={confirm} autocomplete="new-password" />
        </Field>
        {#if pwError}<p class="text-bad">{pwError}</p>{/if}
        <div class="row-end"><Button type="submit" icon={KeyRound} busy={pwBusy} disabled={!pwValid}>Change password</Button></div>
      </form>
    </Group>
  {:else if me}
    <Group title="Password"><p class="muted">{NO_SESSION[me.via] ?? "There's no Nodestarr password to change for this sign-in."}</p></Group>
  {/if}

  {#if me?.can_change_password}
    <Group title="Two-step sign-in" hint="A code from an authenticator app on your phone, on top of your password.">
      {#if recovery}
        <div class="recovery">
          <p><strong>Two-step sign-in is on.</strong> Save these recovery codes somewhere safe. Each one signs you in once if you lose your phone. They won't be shown again.</p>
          <pre class="codes">{recovery.join('\n')}</pre>
          <div class="row">
            <CopyButton text={recovery.join('\n')} label="Copy codes" showLabel small={false} />
            <button type="button" class="btn ghost" onclick={downloadCodes}>Download</button>
            <button type="button" class="btn primary" onclick={() => (recovery = null)}>I saved them</button>
          </div>
        </div>
      {:else if me.totp_enabled}
        <p class="on"><ShieldCheck size={18} /> On for your account.</p>
        <form
          class="row"
          onsubmit={(e) => {
            e.preventDefault();
            void disable();
          }}
        >
          <input class="input pw" type="password" placeholder="Your password" bind:value={disablePassword} autocomplete="current-password" aria-label="Password" />
          <Button type="submit" variant="danger" busy={totpBusy} disabled={!disablePassword}>Turn off</Button>
        </form>
      {:else if setup}
        <div class="setup">
          <div class="qr" aria-label="QR code for your authenticator app">{@html qr}</div>
          <div class="steps">
            <p>1. Scan this with an authenticator app (like Aegis, 2FAS, Google Authenticator or 1Password).</p>
            <p class="small muted">Can't scan? Enter this key instead: <code class="secret">{setup.secret}</code> <CopyButton text={setup.secret} label="Copy key" /></p>
            <p>2. Enter the 6-digit code it shows.</p>
            <form
              class="row"
              onsubmit={(e) => {
                e.preventDefault();
                void enable();
              }}
            >
              <input class="input code" inputmode="numeric" autocomplete="one-time-code" maxlength="8" bind:value={code} placeholder="123456" aria-label="Code" />
              <Button type="submit" variant="primary" busy={totpBusy} disabled={code.replace(/\s/g, '').length < 6}>Turn on</Button>
              <button type="button" class="btn ghost" onclick={() => (setup = null)}>Cancel</button>
            </form>
          </div>
        </div>
      {:else}
        <p class="muted">Off. Turn it on to protect your account even if your password leaks.</p>
        <Button icon={ShieldCheck} onclick={startSetup}>Set up two-step sign-in</Button>
      {/if}
      {#if totpError}<p class="text-bad">{totpError}</p>{/if}
    </Group>
  {/if}

  <Group title="Where you're signed in">
    {#if !sessions}
      <p class="faint">Loading…</p>
    {:else if sessions.length === 0}
      <p class="muted">{NO_SESSION[me?.via ?? ''] ?? 'No other sign-ins.'}</p>
    {:else}
      <ul class="sessions">
        {#each sessions as s (s.id)}
          <li>
            <div class="what">
              <span>{device(s.user_agent)} {#if s.current}<span class="chip here">This browser</span>{/if}</span>
              <span class="faint small">{s.ip ?? 'unknown address'}, active <TimeAgo ts={s.last_seen_at} />, signed in <TimeAgo ts={s.created_at} /></span>
            </div>
            {#if !s.current}<button type="button" class="btn small ghost" onclick={() => revoke(s)}>Sign out</button>{/if}
          </li>
        {/each}
      </ul>
      {#if sessions.some((x) => !x.current)}
        <div class="row-end"><Button small variant="ghost" onclick={revokeOthers}>Sign out everywhere else</Button></div>
      {/if}
    {/if}
  </Group>
</div>

<style>
  .account {
    display: grid;
    gap: 16px;
    max-width: 900px;
  }
  header p {
    margin: 4px 0 0;
  }
  .row-end {
    display: flex;
    justify-content: flex-end;
    padding: 4px 0 8px;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }
  .on {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--ok);
  }
  .pw {
    max-width: 260px;
  }
  .setup {
    display: flex;
    flex-wrap: wrap;
    gap: 20px;
    padding: 8px 0;
  }
  .qr {
    flex: none;
    width: 190px;
    height: 190px;
    padding: 8px;
    border-radius: var(--radius);
    background: #fff;
  }
  .qr :global(svg) {
    width: 100%;
    height: 100%;
  }
  .steps {
    flex: 1;
    min-width: 240px;
  }
  .steps p {
    margin-bottom: 10px;
  }
  .secret {
    word-break: break-all;
  }
  .code {
    max-width: 140px;
    font-family: var(--font-code);
    letter-spacing: 0.15em;
  }
  .codes {
    padding: 12px 14px;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    background: var(--sunken);
    font-size: 14px;
    line-height: 1.8;
    columns: 2;
    max-width: 360px;
  }
  .recovery {
    display: grid;
    gap: 10px;
    padding: 6px 0;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .sessions {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .sessions li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }
  .sessions li:last-child {
    border-bottom: 0;
  }
  .what {
    flex: 1;
    display: grid;
    gap: 2px;
  }
  .here {
    margin-left: 6px;
    color: var(--ok);
  }
</style>
