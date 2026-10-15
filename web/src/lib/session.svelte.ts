// Who is signed in, and the server settings the UI depends on.

import { api } from './api';
import { setTimeFormat } from './format';
import { DEFAULT_ACCENT, applyAccent, applyDensity, applyTheme, storedTheme, type ThemeChoice } from './theme';
import type { AuthStatus, Me, Settings } from './types';

interface SettingsResponse {
  settings: Settings;
  defaults: { alert_rules: Settings['monitoring']['rules'] };
  mqtt_topics: Record<string, string>;
  secret_placeholder: string;
}

class Session {
  status = $state<AuthStatus | null>(null);
  me = $state<Me | null>(null);
  settings = $state<Settings | null>(null);
  defaults = $state<SettingsResponse['defaults'] | null>(null);
  mqttTopics = $state<Record<string, string>>({});
  /** Value the server sends in place of stored secrets. */
  secretPlaceholder = '__nodestarr_secret__';

  isAdmin = $derived(this.me?.role === 'admin');
  signedIn = $derived(!!this.status?.authenticated && !!this.me);

  /** Loads the auth state; loads the user and settings when signed in. */
  async init(): Promise<void> {
    this.status = await api.get<AuthStatus>('/auth/status', { quiet401: true });
    if (this.status.authenticated) await this.loadUser();
  }

  async loadUser(): Promise<void> {
    const [me] = await Promise.all([api.get<Me>('/auth/me'), this.loadSettings()]);
    this.me = me;
  }

  async loadSettings(): Promise<void> {
    const r = await api.get<SettingsResponse>('/settings');
    this.settings = r.settings;
    this.defaults = r.defaults;
    this.mqttTopics = r.mqtt_topics;
    this.secretPlaceholder = r.secret_placeholder;
    this.applyLook();
  }

  /** Applies theme, accent, density and time format from settings. */
  applyLook(): void {
    const ui = this.settings?.general.ui;
    const theme = (storedTheme() ?? (ui?.theme as ThemeChoice | undefined) ?? 'dark') as ThemeChoice;
    applyTheme(theme);
    applyAccent(ui?.accent ?? DEFAULT_ACCENT);
    applyDensity(ui?.density ?? 'comfortable');
    setTimeFormat(ui?.time_format ?? '24h');
    const name = this.settings?.general.instance_name || this.status?.instance_name || 'Nodestarr';
    document.title = name;
  }

  async logout(): Promise<void> {
    try {
      await api.post('/auth/logout', {}, { quiet401: true });
    } finally {
      this.me = null;
      this.status = this.status ? { ...this.status, authenticated: false, user: null } : null;
    }
  }

  /** The session ended on the server (401): go back to the sign-in page. */
  expired(): void {
    if (!this.status?.authenticated) return;
    this.me = null;
    this.status = { ...this.status, authenticated: false, user: null };
  }
}

export const session = new Session();
