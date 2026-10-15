<!--
  Alerts: what's wrong right now (dismissable), what happened recently, and
  the rules that decide when an alert fires.
-->
<script lang="ts">
  import BellOff from '@lucide/svelte/icons/bell-off';
  import CircleCheck from '@lucide/svelte/icons/circle-check';
  import EmptyState from '../../components/EmptyState.svelte';
  import Spinner from '../../components/Spinner.svelte';
  import TimeAgo from '../../components/TimeAgo.svelte';
  import { alertKind } from '../../lib/alerts';
  import { api, errorMessage } from '../../lib/api';
  import { dateTime, duration } from '../../lib/format';
  import { live } from '../../lib/live.svelte';
  import { href } from '../../lib/router.svelte';
  import { session } from '../../lib/session.svelte';
  import type { AlertRule, AlertView } from '../../lib/types';
  import { ui } from '../../lib/ui.svelte';

  let history = $state<AlertView[] | null>(null);
  let rules = $state<AlertRule[]>([]);
  let error = $state('');

  async function load() {
    try {
      const r = await api.get<{ alerts: AlertView[]; rules: AlertRule[] }>('/monitoring/alerts?limit=200');
      history = r.alerts;
      rules = r.rules;
      error = '';
    } catch (err) {
      error = errorMessage(err);
    }
  }

  // New or resolved alerts change the history too.
  $effect(() => {
    void live.alerts.length;
    void load();
  });

  const ruleById = $derived(new Map(rules.map((r) => [r.id, r])));

  function targetLink(a: AlertView): string | null {
    const c = live.byIdentity.get(a.target);
    return c ? href(`/containers/${encodeURIComponent(c.name)}`) : null;
  }

  async function dismiss(a: AlertView) {
    try {
      await api.post(`/monitoring/alerts/${a.id}/dismiss`);
      live.alerts = live.alerts.filter((x) => x.id !== a.id);
      ui.toast("Dismissed. It won't come back until the problem clears and happens again.", 'success');
    } catch (err) {
      ui.error(err);
    }
  }

  const SEVERITY: Record<string, string> = { info: 'Info', success: 'Info', warning: 'Warning', error: 'Problem' };
</script>

<div class="alerts">
  <section class="panel">
    <div class="panel-head"><h2>Active</h2></div>
    {#if live.alerts.length === 0}
      <EmptyState icon={CircleCheck} title="No active alerts">Nothing needs your attention right now.</EmptyState>
    {:else}
      <ul class="active">
        {#each live.alerts as a (a.id)}
          {@const link = targetLink(a)}
          <li class="sev-{a.severity}">
            <div class="what">
              <strong>{a.message}</strong>
              <span class="faint small">
                {alertKind(ruleById.get(a.rule_id)?.kind ?? a.rule_id).label}
                {#if a.target_name}, {#if link}<a href={link}>{a.target_name}</a>{:else}{a.target_name}{/if}{/if}, since <TimeAgo ts={a.started_at} />
              </span>
            </div>
            {#if session.isAdmin}
              <button type="button" class="btn small ghost" onclick={() => dismiss(a)} title="Hide until it clears and happens again"><BellOff size={14} />Dismiss</button>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}
  </section>

  <div class="split">
    <section class="panel">
      <div class="panel-head"><h2>Recent</h2></div>
      {#if error}
        <p class="panel-body text-bad">{error}</p>
      {:else if !history}
        <p class="panel-body faint"><Spinner size={13} /> Loading…</p>
      {:else if history.length === 0}
        <p class="panel-body muted">No alerts have fired yet.</p>
      {:else}
        <div class="table-wrap">
          <table class="table">
            <thead>
              <tr><th>Started</th><th>What</th><th>Lasted</th></tr>
            </thead>
            <tbody>
              {#each history as a (a.id)}
                <tr>
                  <td class="shrink num">{dateTime(a.started_at)}</td>
                  <td>
                    <span class="sev-text-{a.severity}">{SEVERITY[a.severity] ?? a.severity}</span>
                    {a.message}
                    {#if a.target_name}<span class="faint small">({a.target_name})</span>{/if}
                  </td>
                  <td class="shrink">{a.resolved_at ? duration(a.resolved_at - a.started_at) : 'ongoing'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </section>

    <section class="panel">
      <div class="panel-head">
        <h2>Rules</h2>
        {#if session.isAdmin}<a class="small" href={href('/settings/monitoring')}>Change</a>{/if}
      </div>
      <ul class="rules">
        {#each rules as r (r.id)}
          {@const k = alertKind(r.kind)}
          <li class:off={!r.enabled}>
            <span class="rule-name">{k.label}</span>
            <span class="small muted">{r.enabled ? k.describe(r) : 'Off'}</span>
          </li>
        {/each}
      </ul>
      <p class="panel-body faint small foot">Alerts are sent to the notification channels that subscribe to them, set in Settings > Notifications.</p>
    </section>
  </div>
</div>

<style>
  .alerts {
    display: grid;
    gap: 16px;
  }
  .active {
    display: grid;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .active li {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 16px;
    border-bottom: 1px solid var(--line);
    border-left: 3px solid var(--info);
  }
  .active li:last-child {
    border-bottom: 0;
  }
  .active li.sev-warning {
    border-left-color: var(--warn);
  }
  .active li.sev-error {
    border-left-color: var(--bad);
  }
  .what {
    flex: 1;
    display: grid;
    gap: 2px;
    min-width: 0;
  }
  .small {
    font-size: var(--fs-sm);
  }
  .split {
    display: grid;
    grid-template-columns: minmax(0, 1.5fr) minmax(280px, 1fr);
    gap: 16px;
    align-items: start;
  }
  .sev-text-warning {
    color: var(--warn);
    font-weight: 500;
  }
  .sev-text-error {
    color: var(--bad);
    font-weight: 500;
  }
  .sev-text-info,
  .sev-text-success {
    color: var(--info);
    font-weight: 500;
  }
  .rules {
    display: grid;
    gap: 12px;
    margin: 0;
    padding: 14px 16px 0;
    list-style: none;
  }
  .rules li {
    display: grid;
    gap: 2px;
  }
  .rules li.off .rule-name {
    color: var(--text-3);
  }
  .rule-name {
    font-weight: 500;
  }
  .foot {
    margin: 0;
  }
  @media (max-width: 1000px) {
    .split {
      grid-template-columns: 1fr;
    }
  }
</style>
