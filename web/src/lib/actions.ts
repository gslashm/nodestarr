// Container and stack actions shared by lists, the dashboard, detail pages and the
// command palette: confirmations, API calls, and feedback.

import type { Component } from 'svelte';
import ArchiveIcon from '@lucide/svelte/icons/archive';
import CircleArrowUp from '@lucide/svelte/icons/circle-arrow-up';
import ExternalLink from '@lucide/svelte/icons/external-link';
import FileText from '@lucide/svelte/icons/file-text';
import Pause from '@lucide/svelte/icons/pause';
import Play from '@lucide/svelte/icons/play';
import RefreshCw from '@lucide/svelte/icons/refresh-cw';
import RotateCw from '@lucide/svelte/icons/rotate-cw';
import Search from '@lucide/svelte/icons/search';
import Skull from '@lucide/svelte/icons/skull';
import Square from '@lucide/svelte/icons/square';
import SquareTerminal from '@lucide/svelte/icons/square-terminal';
import Trash2 from '@lucide/svelte/icons/trash-2';
import { api, seg } from './api';
import { hasUpdate, imageTag, isUp, resolveLink, stateOf, updateLabel } from './format';
import { href } from './router.svelte';
import { session } from './session.svelte';
import type { ContainerView, UpdateInfo } from './types';
import { ui } from './ui.svelte';

export type ContainerAction = 'start' | 'stop' | 'restart' | 'pause' | 'unpause' | 'kill' | 'recreate' | 'remove';

const PAST: Record<ContainerAction, string> = {
  start: 'started',
  stop: 'stopped',
  restart: 'restarted',
  pause: 'paused',
  unpause: 'resumed',
  kill: 'killed',
  recreate: 'is being recreated',
  remove: 'removed',
};

/** Runs a container action after confirming the disruptive ones. Returns true when it ran. */
export async function runAction(c: ContainerView, action: ContainerAction): Promise<boolean> {
  const name = c.display_name;
  let opts: Record<string, unknown> = {};
  if (action === 'stop' || action === 'kill') {
    const ok = await ui.confirm({
      title: `${action === 'kill' ? 'Kill' : 'Stop'} ${name}?`,
      message:
        action === 'kill'
          ? 'Kill ends the process immediately, without letting it save its work. Prefer Stop unless the app is stuck.'
          : `${name} will be unavailable until you start it again.${c.stack ? ' Compose will not restart it on its own.' : ''}`,
      confirmLabel: action === 'kill' ? 'Kill' : 'Stop',
      danger: action === 'kill',
    });
    if (!ok) return false;
  } else if (action === 'remove') {
    const r = await ui.ask({
      title: `Remove ${name}?`,
      message: c.stack
        ? `This removes the container only. The service stays in the ${c.stack} stack's compose file, so deploying the stack recreates it.`
        : 'The container is deleted. Its bind-mounted folders and named volumes stay on disk.',
      confirmLabel: 'Remove',
      danger: true,
      options: [{ key: 'volumes', label: 'Also delete its anonymous volumes', danger: true }],
      typeToConfirm: c.stack ? undefined : c.name,
    });
    if (!r.ok) return false;
    opts = { volumes: r.options.volumes };
  } else if (action === 'recreate') {
    const ok = await ui.confirm({
      title: `Recreate ${name}?`,
      message: 'The container is replaced with a fresh one from the same image and settings. Data in volumes and folders is kept.',
      confirmLabel: 'Recreate',
    });
    if (!ok) return false;
  }
  try {
    const r = await api.post<{ ok: boolean; task_id: string | null }>(`/containers/${seg(c.id)}/actions/${action}`, opts);
    if (r.task_id) ui.taskStarted(r.task_id, `Recreating ${name}`);
    else ui.toast(`${name} ${PAST[action]}`, 'success');
    return true;
  } catch (err) {
    ui.error(err, `Couldn't ${action === 'unpause' ? 'resume' : action} ${name}`);
    return false;
  }
}

/** Starts an update (optionally to another tag). */
export async function startUpdate(c: ContainerView, tag?: string, skipBackup = false): Promise<string | null> {
  try {
    const r = await api.post<{ task_id: string }>(`/containers/${seg(c.id)}/update`, { tag: tag ?? null, skip_backup: skipBackup });
    ui.taskStarted(r.task_id, tag ? `Switching ${c.display_name} to ${tag}` : `Updating ${c.display_name}`);
    return r.task_id;
  } catch (err) {
    ui.error(err, `Couldn't update ${c.display_name}`);
    return null;
  }
}

/** Confirms and starts an update; explains what will happen first. */
export async function confirmUpdate(c: ContainerView): Promise<void> {
  const u = c.update;
  const newerTag = u?.newer_tag?.tag;
  const backup = session.settings?.updates.backup_before_update && c.prefs.backup_enabled;
  const what = newerTag
    ? `Switches the image tag from ${imageTag(c.image)} to ${newerTag} (${u?.newer_tag?.scope} update).`
    : u?.latest_version
      ? `Pulls the new build of ${imageTag(c.image)} (${u.latest_version}).`
      : `Pulls the new build of ${imageTag(c.image)}.`;
  const r = await ui.ask({
    title: `Update ${c.display_name}?`,
    message: [what, backup ? 'A backup is taken first.' : '', 'If the new version fails to start, Nodestarr rolls back automatically.']
      .filter(Boolean)
      .join(' '),
    confirmLabel: 'Update',
    options: backup ? [{ key: 'skip', label: 'Skip the pre-update backup' }] : [],
  });
  if (r.ok) await startUpdate(c, newerTag, !!r.options.skip);
}

export async function backupNow(c: ContainerView): Promise<void> {
  try {
    const r = await api.post<{ task_id: string }>(`/containers/${seg(c.id)}/backups`);
    ui.taskStarted(r.task_id, `Backing up ${c.display_name}`);
  } catch (err) {
    ui.error(err, `Couldn't back up ${c.display_name}`);
  }
}

export async function checkNow(c: ContainerView): Promise<UpdateInfo | null> {
  try {
    const info = await api.post<UpdateInfo>(`/containers/${seg(c.id)}/check`);
    c.update = info;
    const label = updateLabel(info);
    if (info.status === 'error') ui.toast(`Check failed: ${info.error}`, 'error');
    else ui.toast(label ? `${c.display_name}: update available (${label})` : `${c.display_name} is up to date`, label ? 'info' : 'success');
    return info;
  } catch (err) {
    ui.error(err, 'Update check failed');
    return null;
  }
}

/** The best URL for a container's web UI, if it has one. */
export function webUrl(c: ContainerView): string | null {
  if (c.prefs.web_url) return resolveLink(c.prefs.web_url);
  return c.links[0] ? resolveLink(c.links[0].url) : null;
}

export interface ActionItem {
  label: string;
  icon: Component;
  danger?: boolean;
  disabled?: boolean;
  hint?: string;
  href?: string;
  run?: () => unknown;
}

/** Menu entries for a container's overflow menu. */
export function containerMenu(c: ContainerView): (ActionItem | 'separator')[] {
  const st = stateOf(c);
  const up = isUp(st);
  const admin = session.isAdmin;
  const path = `/containers/${encodeURIComponent(c.name)}`;
  const items: (ActionItem | 'separator')[] = [];
  const web = webUrl(c);
  if (web) items.push({ label: 'Open web UI', icon: ExternalLink, run: () => window.open(web, '_blank', 'noopener') });
  items.push({ label: 'Logs', icon: FileText, href: href(`${path}/logs`) });
  if (admin) items.push({ label: 'Console', icon: SquareTerminal, href: href(`${path}/console`), disabled: !up });
  if (!admin) return items;
  items.push('separator');
  if (st === 'paused') items.push({ label: 'Resume', icon: Play, run: () => runAction(c, 'unpause') });
  else if (up || st === 'restarting') {
    items.push({ label: 'Restart', icon: RotateCw, run: () => runAction(c, 'restart') });
    items.push({ label: 'Stop', icon: Square, run: () => runAction(c, 'stop') });
    items.push({ label: 'Pause', icon: Pause, run: () => runAction(c, 'pause') });
  } else items.push({ label: 'Start', icon: Play, run: () => runAction(c, 'start') });
  items.push('separator');
  if (hasUpdate(c.update)) items.push({ label: 'Update', icon: CircleArrowUp, hint: updateLabel(c.update) ?? '', run: () => confirmUpdate(c) });
  items.push({ label: 'Check for updates', icon: Search, run: () => checkNow(c) });
  items.push({ label: 'Back up now', icon: ArchiveIcon, run: () => backupNow(c) });
  items.push({ label: 'Recreate', icon: RefreshCw, run: () => runAction(c, 'recreate'), disabled: c.is_self });
  items.push('separator');
  items.push({ label: 'Kill', icon: Skull, danger: true, run: () => runAction(c, 'kill'), disabled: !up || c.is_self });
  items.push({ label: 'Remove', icon: Trash2, danger: true, run: () => runAction(c, 'remove'), disabled: c.is_self });
  return items;
}

export type StackAction = 'up' | 'down' | 'start' | 'stop' | 'restart' | 'pull' | 'update';

const STACK_VERB: Record<StackAction, string> = {
  up: 'Deploying',
  down: 'Taking down',
  start: 'Starting',
  stop: 'Stopping',
  restart: 'Restarting',
  pull: 'Pulling images for',
  update: 'Updating',
};

/** Runs a stack action (confirming the disruptive ones). */
export async function stackAction(name: string, action: StackAction, services: string[] = []): Promise<string | null> {
  let body: Record<string, unknown> = { services };
  if (action === 'down') {
    const r = await ui.ask({
      title: `Take down ${name}?`,
      message: 'Its containers are stopped and removed. Folders and named volumes stay unless you choose otherwise.',
      confirmLabel: 'Take down',
      danger: true,
      options: [
        { key: 'orphans', label: 'Also remove containers no longer in the compose file', checked: true },
        { key: 'volumes', label: 'Delete named volumes (their data is lost)', danger: true },
      ],
    });
    if (!r.ok) return null;
    body = { services, remove_orphans: r.options.orphans, remove_volumes: r.options.volumes };
  } else if (action === 'stop' || action === 'restart') {
    const ok = await ui.confirm({
      title: `${action === 'stop' ? 'Stop' : 'Restart'} ${services.length ? services.join(', ') : `the ${name} stack`}?`,
      confirmLabel: action === 'stop' ? 'Stop' : 'Restart',
    });
    if (!ok) return null;
  }
  try {
    const r = await api.post<{ task_id: string }>(`/stacks/${seg(name)}/actions/${action}`, body);
    ui.taskStarted(r.task_id, `${STACK_VERB[action]} ${name}`);
    return r.task_id;
  } catch (err) {
    ui.error(err, `Couldn't run that on ${name}`);
    return null;
  }
}
