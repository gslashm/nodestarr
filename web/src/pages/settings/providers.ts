// Notification channel kinds: names and the settings a new one starts with.

import type { NotificationProvider, ProviderConfig } from '../../lib/types';

export type ProviderKind = ProviderConfig['kind'];

export const PROVIDER_LABEL: Record<ProviderKind, string> = {
  discord: 'Discord',
  email: 'Email',
  telegram: 'Telegram',
  ntfy: 'ntfy',
  gotify: 'Gotify',
  pushover: 'Pushover',
  slack: 'Slack',
  webhook: 'Webhook',
};

const DEFAULT_CONFIG: { [K in ProviderKind]: Extract<ProviderConfig, { kind: K }> } = {
  discord: { kind: 'discord', webhook_url: '', username: '', mention: '' },
  email: { kind: 'email', host: '', port: 587, security: 'starttls', username: '', password: '', from: '', to: [] },
  telegram: { kind: 'telegram', bot_token: '', chat_id: '', topic_id: '' },
  ntfy: { kind: 'ntfy', server: 'https://ntfy.sh', topic: '', token: '', username: '', password: '' },
  gotify: { kind: 'gotify', server: '', app_token: '' },
  pushover: { kind: 'pushover', user_key: '', app_token: '', device: '' },
  slack: { kind: 'slack', webhook_url: '' },
  webhook: { kind: 'webhook', url: '', method: 'POST', headers: {}, body_template: '' },
};

function randomId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(6));
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

export function newProvider(kind: ProviderKind): NotificationProvider {
  return {
    id: randomId(),
    name: PROVIDER_LABEL[kind],
    enabled: true,
    events: [],
    min_severity: 'info',
    config: JSON.parse(JSON.stringify(DEFAULT_CONFIG[kind])),
  };
}
