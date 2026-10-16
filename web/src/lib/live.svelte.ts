// Live state shared by every page: containers (kept current from the
// server's event stream), tasks, host stats, alerts, uptime and activity.
//
// One EventSource per tab. It closes when the tab has been hidden for a
// minute, so an idle browser costs the server nothing, and reopens with a
// fresh snapshot when the tab is visible again.

import { api, apiUrl } from './api';
import { session } from './session.svelte';
import { ui } from './ui.svelte';
import type {
  ActivityEntry,
  AlertView,
  AppStatus,
  ContainerSummary,
  ContainerView,
  HostStats,
  LiveStat,
  TaskSnapshot,
  UpdateInfo,
  UptimeStatus,
} from './types';

type ServerEvent =
  | { type: 'hello'; data: { version: string; time: number } }
  | { type: 'resync' }
  | { type: 'signed_out' }
  | { type: 'container'; data: ContainerSummary }
  | { type: 'container_removed'; data: { id: string; identity: string } }
  | { type: 'stacks_changed' }
  | { type: 'task'; data: TaskSnapshot }
  | { type: 'task_log'; data: { id: string; lines: string[] } }
  | { type: 'activity'; data: ActivityEntry }
  | { type: 'update'; data: { identity: string; info: UpdateInfo } }
  | { type: 'update_check'; data: { running: boolean; last_check: number | null } }
  | { type: 'stats'; data: LiveStat[] }
  | { type: 'host'; data: HostStats }
  | { type: 'uptime'; data: UptimeStatus }
  | { type: 'app'; data: { identity: string; status: AppStatus } }
  | { type: 'alert'; data: AlertView }
  | { type: 'toast'; data: { level: 'info' | 'success' | 'warning' | 'error'; message: string } }
  | { type: 'settings' }
  | { type: 'prefs'; data: { identity: string } };

const HIDE_GRACE_MS = 60_000;
const TRAIL = 40;
const REMOVED_KEEP = 200;

function sortKey(c: ContainerView): string {
  // Stacks first (alphabetical), standalone containers last.
  return `${c.stack ? `0${c.stack}` : '1'}\u0000${c.display_name.toLowerCase()}`;
}

function isInternal(name: string): boolean {
  return name.startsWith('nodestarr-updater-') || name.includes('-nodestarr-old-');
}

class Live {
  containers = $state<Record<string, ContainerView>>({});
  loaded = $state(false);
  loadError = $state<string | null>(null);
  dockerConnected = $state(true);
  checkingUpdates = $state(false);
  /** When the last full update check finished (unix seconds), once known. */
  lastUpdateCheck = $state<number | null>(null);
  connected = $state(false);
  tasks = $state<Record<string, TaskSnapshot>>({});
  host = $state<HostStats | null>(null);
  /** Recent host samples for sparklines (oldest first). */
  hostTrail = $state<{ ts: number; cpu: number; mem: number }[]>([]);
  /** Recent CPU samples per identity, for sparklines. */
  cpuTrail = $state<Record<string, number[]>>({});
  alerts = $state<AlertView[]>([]);
  activity = $state<ActivityEntry[]>([]);
  uptime = $state<Record<string, UptimeStatus>>({});
  /** Bumped when stacks change; stack pages refetch when it does. */
  stacksVersion = $state(0);
  serverVersion = $state<string | null>(null);

  list = $derived(Object.values(this.containers).sort((a, b) => sortKey(a).localeCompare(sortKey(b))));
  byIdentity = $derived(new Map(Object.values(this.containers).map((c) => [c.identity, c])));
  activeTasks = $derived(
    Object.values(this.tasks)
      .filter((t) => t.status === 'queued' || t.status === 'running')
      .sort((a, b) => a.created_at - b.created_at),
  );

  private es: EventSource | null = null;
  private hadError = false;
  private refreshTimer: ReturnType<typeof setTimeout> | null = null;
  private hideTimer: ReturnType<typeof setTimeout> | null = null;
  private taskLogListeners = new Map<string, Set<(lines: string[]) => void>>();
  private started = false;
  /** Recently removed container ids. Ids are never reused, so a snapshot
   * fetched just before a removal must not bring one back. */
  private removed = new Set<string>();

  /** Loads the snapshot and opens the event stream. */
  async start(): Promise<void> {
    if (this.started) return;
    this.started = true;
    document.addEventListener('visibilitychange', this.onVisibility);
    await this.refresh();
    this.connect();
  }

  stop(): void {
    this.started = false;
    document.removeEventListener('visibilitychange', this.onVisibility);
    this.disconnect();
    this.containers = {};
    this.tasks = {};
    this.loaded = false;
  }

  /** Fetches containers, active tasks, open alerts and uptime checks. */
  async refresh(): Promise<void> {
    try {
      const [list, tasks, alerts, uptime] = await Promise.all([
        api.get<{ containers: ContainerView[]; docker_connected: boolean; checking_updates: boolean }>('/containers'),
        api.get<{ tasks: TaskSnapshot[] }>('/tasks?active=1&limit=100'),
        api.get<{ alerts: AlertView[] }>('/monitoring/alerts?active=1&limit=200'),
        api.get<{ checks: UptimeStatus[] }>('/uptime'),
      ]);
      const next: Record<string, ContainerView> = {};
      for (const c of list.containers) if (!this.removed.has(c.id)) next[c.id] = c;
      this.containers = next;
      this.dockerConnected = list.docker_connected;
      this.checkingUpdates = list.checking_updates;
      const t: Record<string, TaskSnapshot> = {};
      for (const task of tasks.tasks) t[task.id] = task;
      // Keep recently finished tasks this tab already knows about.
      for (const [id, task] of Object.entries(this.tasks)) if (!t[id]) t[id] = task;
      this.tasks = t;
      this.alerts = alerts.alerts;
      this.uptime = Object.fromEntries(uptime.checks.map((u) => [u.id, u]));
      this.loadError = null;
      this.loaded = true;
    } catch (err) {
      this.loadError = err instanceof Error ? err.message : String(err);
    }
  }

  /** Debounced refresh for bursts of changes. */
  scheduleRefresh(delay = 300): void {
    if (this.refreshTimer) clearTimeout(this.refreshTimer);
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null;
      void this.refresh();
    }, delay);
  }

  /** Finds a container by id, name or identity. */
  find(key: string): ContainerView | undefined {
    return this.containers[key] ?? this.list.find((c) => c.name === key || c.identity === key || c.id.startsWith(key));
  }

  /** Subscribes to log lines of one task; returns an unsubscribe function. */
  onTaskLog(id: string, fn: (lines: string[]) => void): () => void {
    let set = this.taskLogListeners.get(id);
    if (!set) {
      set = new Set();
      this.taskLogListeners.set(id, set);
    }
    set.add(fn);
    return () => {
      set.delete(fn);
      if (set.size === 0) this.taskLogListeners.delete(id);
    };
  }

  /** Tasks (active or recent) for a container identity. */
  tasksFor(identity: string): TaskSnapshot[] {
    return Object.values(this.tasks)
      .filter((t) => t.target === identity)
      .sort((a, b) => b.created_at - a.created_at);
  }

  // ------------------------------------------------------------ stream

  private connect(): void {
    if (this.es) return;
    const es = new EventSource(apiUrl('/events?stats=1'), { withCredentials: true });
    this.es = es;
    es.onopen = () => {
      this.connected = true;
      if (this.hadError) {
        this.hadError = false;
        // We may have missed events while disconnected.
        void this.refresh();
      }
    };
    es.onerror = () => {
      this.connected = false;
      this.hadError = true;
      if (es.readyState === EventSource.CLOSED) {
        // The server refused (e.g. the session ended): check before retrying.
        this.es = null;
        setTimeout(() => void this.recover(), 3000);
      }
    };
    es.onmessage = (msg) => {
      try {
        this.handle(JSON.parse(msg.data) as ServerEvent);
      } catch (err) {
        console.warn('bad event', err);
      }
    };
  }

  private async recover(): Promise<void> {
    if (!this.started || this.es) return;
    try {
      await session.init();
    } catch {
      // Still unreachable; try again shortly.
      setTimeout(() => void this.recover(), 5000);
      return;
    }
    if (session.status?.authenticated) {
      await this.refresh();
      this.connect();
    }
  }

  private disconnect(): void {
    this.es?.close();
    this.es = null;
    this.connected = false;
  }

  private onVisibility = (): void => {
    if (document.hidden) {
      this.hideTimer = setTimeout(() => this.disconnect(), HIDE_GRACE_MS);
    } else {
      if (this.hideTimer) clearTimeout(this.hideTimer);
      this.hideTimer = null;
      if (!this.es && this.started) {
        void this.refresh().then(() => this.connect());
      }
    }
  };

  private handle(ev: ServerEvent): void {
    switch (ev.type) {
      case 'hello':
        if (this.serverVersion && this.serverVersion !== ev.data.version) {
          ui.toast(`Nodestarr was updated to ${ev.data.version}.`, 'info', { label: 'Reload', run: () => location.reload() });
        }
        this.serverVersion = ev.data.version;
        break;
      case 'resync':
        void this.refresh();
        break;
      case 'signed_out':
        // This sign-in was ended elsewhere (signed out, password changed,
        // user or API key removed).
        this.disconnect();
        session.expired();
        break;
      case 'container':
        this.mergeContainer(ev.data);
        break;
      case 'container_removed':
        this.removed.add(ev.data.id);
        for (const id of this.removed) {
          if (this.removed.size <= REMOVED_KEEP) break;
          this.removed.delete(id);
        }
        if (this.containers[ev.data.id]) delete this.containers[ev.data.id];
        break;
      case 'stacks_changed':
        this.stacksVersion++;
        break;
      case 'task':
        this.tasks[ev.data.id] = ev.data;
        this.markBusy(ev.data);
        this.notifyFinished(ev.data);
        break;
      case 'task_log':
        this.taskLogListeners.get(ev.data.id)?.forEach((fn) => fn(ev.data.lines));
        break;
      case 'activity':
        this.activity = [ev.data, ...this.activity].slice(0, 100);
        break;
      case 'update': {
        const c = this.byIdentity.get(ev.data.identity);
        if (c) c.update = ev.data.info;
        break;
      }
      case 'update_check':
        this.checkingUpdates = ev.data.running;
        if (ev.data.last_check) this.lastUpdateCheck = ev.data.last_check;
        break;
      case 'stats':
        this.applyStats(ev.data);
        break;
      case 'host':
        this.host = ev.data;
        this.hostTrail = [
          ...this.hostTrail.slice(-(TRAIL * 3 - 1)),
          {
            ts: Date.now() / 1000,
            cpu: ev.data.cpu,
            mem: ev.data.mem_total ? ((ev.data.mem_total - ev.data.mem_available) / ev.data.mem_total) * 100 : 0,
          },
        ];
        break;
      case 'uptime': {
        this.uptime[ev.data.id] = ev.data;
        const c = ev.data.identity ? this.byIdentity.get(ev.data.identity) : undefined;
        if (c) c.uptime = { state: ev.data.state, latency_ms: ev.data.latency_ms, uptime_24h: ev.data.uptime_24h };
        break;
      }
      case 'app': {
        const c = this.byIdentity.get(ev.data.identity);
        if (c) c.app_status = ev.data.status;
        break;
      }
      case 'alert': {
        const others = this.alerts.filter((a) => a.id !== ev.data.id);
        this.alerts = ev.data.resolved_at ? others : [ev.data, ...others];
        break;
      }
      case 'toast':
        ui.toast(ev.data.message, ev.data.level);
        break;
      case 'settings':
        void session.loadSettings();
        this.scheduleRefresh();
        break;
      case 'prefs':
        this.scheduleRefresh();
        break;
    }
  }

  private mergeContainer(s: ContainerSummary): void {
    if (isInternal(s.name) || this.removed.has(s.id)) return;
    const existing = this.containers[s.id];
    if (existing) {
      Object.assign(existing, s);
      return;
    }
    // A recreated container keeps its identity; carry its view over.
    const previous = this.byIdentity.get(s.identity);
    if (previous) {
      // Late events from the container being replaced (stopping, then
      // removed): its successor already has the view.
      if (s.created < previous.created) return;
      const moved: ContainerView = { ...previous, ...s };
      delete this.containers[previous.id];
      this.containers[s.id] = moved;
      return;
    }
    // Unknown container: the list endpoint knows its preferences and whether it's hidden.
    this.scheduleRefresh();
  }

  private applyStats(stats: LiveStat[]): void {
    const seen = new Set<string>();
    for (const s of stats) {
      seen.add(s.identity);
      const c = this.byIdentity.get(s.identity);
      if (c) c.stats = s;
      const trail = this.cpuTrail[s.identity] ?? [];
      trail.push(s.cpu);
      if (trail.length > TRAIL) trail.splice(0, trail.length - TRAIL);
      this.cpuTrail[s.identity] = trail;
    }
    // Containers that stopped have no stats any more.
    for (const c of Object.values(this.containers)) {
      if (c.stats && !seen.has(c.identity) && c.state !== 'running') c.stats = null;
    }
  }

  private markBusy(task: TaskSnapshot): void {
    if (!task.target) return;
    const c = this.byIdentity.get(task.target);
    if (!c) return;
    c.busy = Object.values(this.tasks).some((t) => t.target === task.target && (t.status === 'queued' || t.status === 'running'));
  }

  /** Toasts the outcome of tasks this tab is not already watching. */
  private notifyFinished(task: TaskSnapshot): void {
    if (task.status !== 'succeeded' && task.status !== 'failed') return;
    if (ui.openTaskId === task.id) return;
    if (task.status === 'failed') {
      ui.toast(`${task.title} failed${task.error ? `: ${task.error}` : ''}`, 'error');
    } else if (task.actor?.startsWith('user:') && task.actor === `user:${session.me?.username}`) {
      ui.toast(`${task.title}: done`, 'success');
    }
  }
}

export const live = new Live();
