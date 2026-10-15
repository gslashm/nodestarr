<!--
  Sign-in, the two-factor step, and first-run setup (creating the first
  admin with the setup code printed in Nodestarr's log).
-->
<script lang="ts">
  import { api, errorMessage } from '../lib/api';
  import { session } from '../lib/session.svelte';
  import Logo from '../components/Logo.svelte';
  import Spinner from '../components/Spinner.svelte';

  const setup = $derived(!!session.status?.setup_required);
  const mfa = $derived(!!session.status?.mfa_pending);

  let username = $state('');
  let password = $state('');
  let confirm = $state('');
  let code = $state('');
  let remember = $state(true);
  let busy = $state(false);
  let error = $state('');

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    error = '';
    if (setup && password !== confirm) {
      error = "The passwords don't match.";
      return;
    }
    busy = true;
    try {
      if (setup) {
        await api.post('/auth/setup', { username, password, code }, { quiet401: true });
      } else if (mfa) {
        await api.post('/auth/mfa', { code: code.replace(/\s/g, '') }, { quiet401: true });
      } else {
        const r = await api.post<{ mfa_required: boolean }>('/auth/login', { username, password, remember }, { quiet401: true });
        if (r.mfa_required) {
          code = '';
          await session.init();
          busy = false;
          return;
        }
      }
      await session.init();
    } catch (err) {
      error = errorMessage(err);
    } finally {
      busy = false;
    }
  }

  async function cancelMfa() {
    await session.logout();
    await session.init();
  }
</script>

<div class="page">
  <form class="card" onsubmit={submit}>
    <div class="brand">
      <Logo size={40} />
      <span class="word">node<span class="starr">starr</span></span>
    </div>

    {#if setup}
      <h1>Create the admin account</h1>
      <p class="muted">
        This is a new Nodestarr. Choose a username and password, and enter the setup code from Nodestarr's log
        (<code>docker logs nodestarr</code>).
      </p>
      <label>
        <span>Setup code</span>
        <input class="input mono" bind:value={code} autocomplete="one-time-code" placeholder="ABCD-EFGH" required />
      </label>
      <label>
        <span>Username</span>
        <input class="input" bind:value={username} autocomplete="username" required />
      </label>
      <label>
        <span>Password</span>
        <input class="input" type="password" bind:value={password} autocomplete="new-password" minlength="8" required />
        <small class="faint">At least 8 characters.</small>
      </label>
      <label>
        <span>Repeat password</span>
        <input class="input" type="password" bind:value={confirm} autocomplete="new-password" required />
      </label>
    {:else if mfa}
      <h1>Two-factor code</h1>
      <p class="muted">Enter the 6-digit code from your authenticator app, or one of your recovery codes.</p>
      <label>
        <span>Code</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="input mono code" bind:value={code} inputmode="numeric" autocomplete="one-time-code" autofocus required />
      </label>
    {:else}
      <h1>Sign in{session.status?.instance_name && session.status.instance_name !== 'Nodestarr' ? ` to ${session.status.instance_name}` : ''}</h1>
      <label>
        <span>Username</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input class="input" bind:value={username} autocomplete="username" autofocus required />
      </label>
      <label>
        <span>Password</span>
        <input class="input" type="password" bind:value={password} autocomplete="current-password" required />
      </label>
      <label class="check">
        <input type="checkbox" bind:checked={remember} />
        <span>Keep me signed in on this device</span>
      </label>
    {/if}

    {#if error}<p class="error" role="alert">{error}</p>{/if}

    <button class="btn primary submit" type="submit" disabled={busy}>
      {#if busy}<Spinner size={15} />{/if}
      {setup ? 'Create account' : mfa ? 'Verify' : 'Sign in'}
    </button>
    {#if mfa}
      <button type="button" class="btn ghost" onclick={cancelMfa}>Use a different account</button>
    {/if}
  </form>
  <p class="foot faint">Nodestarr {session.status?.version}</p>
</div>

<style>
  .page {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 16px;
    min-height: 100vh;
    padding: 24px 16px;
    /* A night sky: scattered stars and a faint glow of the accent. */
    background:
      var(--starfield-wide) center / 640px 640px,
      radial-gradient(ellipse at 50% 0%, color-mix(in srgb, var(--accent) 8%, transparent), transparent 60%),
      var(--bg);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 14px;
    width: min(400px, 100%);
    padding: 28px;
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    background: var(--panel);
    box-shadow: var(--shadow);
  }
  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 6px;
  }
  .word {
    font: 700 32px / 1 var(--font-data);
    letter-spacing: -0.01em;
  }
  .starr {
    color: var(--accent-text);
  }
  h1 {
    font-size: var(--fs-lg);
  }
  p {
    margin: 0;
  }
  label {
    display: grid;
    gap: 6px;
    font-weight: 500;
  }
  label.check {
    display: flex;
    align-items: center;
    gap: 10px;
    font-weight: 400;
    color: var(--text-2);
  }
  .code {
    font-size: 20px;
    letter-spacing: 0.2em;
    text-align: center;
  }
  .error {
    color: var(--bad);
  }
  .submit {
    height: 40px;
    margin-top: 4px;
  }
  .foot {
    font-size: var(--fs-sm);
  }
</style>
