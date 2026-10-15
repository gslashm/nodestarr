// UI-wide state: toasts, confirmation dialogs and the task drawer.

import { errorMessage } from './api';

export type ToastLevel = 'info' | 'success' | 'warning' | 'error';

export interface Toast {
  id: number;
  level: ToastLevel;
  message: string;
  action?: { label: string; run: () => void };
}

export interface ConfirmRequest {
  title: string;
  message?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  danger?: boolean;
  /** Optional checkboxes (e.g. "Also delete volumes"). */
  options?: { key: string; label: string; checked?: boolean; danger?: boolean }[];
  /** Require typing this text to confirm (destructive actions). */
  typeToConfirm?: string;
}

export interface ConfirmResult {
  ok: boolean;
  options: Record<string, boolean>;
}

class Ui {
  toasts = $state<Toast[]>([]);
  confirmState = $state<(ConfirmRequest & { resolve: (r: ConfirmResult) => void }) | null>(null);
  /** Task shown in the task drawer. */
  openTaskId = $state<string | null>(null);
  /** Mobile navigation drawer. */
  navOpen = $state(false);
  /** Command palette. */
  paletteOpen = $state(false);

  private nextId = 1;

  toast(message: string, level: ToastLevel = 'info', action?: Toast['action']): void {
    const id = this.nextId++;
    this.toasts = [...this.toasts.slice(-4), { id, level, message, action }];
    const ms = level === 'error' ? 9000 : action ? 7000 : 4500;
    setTimeout(() => this.dismiss(id), ms);
  }

  dismiss(id: number): void {
    this.toasts = this.toasts.filter((t) => t.id !== id);
  }

  error(err: unknown, prefix?: string): void {
    const msg = errorMessage(err);
    this.toast(prefix ? `${prefix}: ${msg}` : msg, 'error');
  }

  /** Asks for confirmation; resolves with the choice and option values. */
  ask(req: ConfirmRequest): Promise<ConfirmResult> {
    return new Promise((resolve) => {
      this.confirmState = { ...req, resolve };
    });
  }

  async confirm(req: ConfirmRequest): Promise<boolean> {
    return (await this.ask(req)).ok;
  }

  settleConfirm(result: ConfirmResult): void {
    const s = this.confirmState;
    this.confirmState = null;
    s?.resolve(result);
  }

  /** Shows a started task: a toast with a link to its live log. */
  taskStarted(taskId: string | null | undefined, message: string): void {
    if (!taskId) {
      this.toast(message, 'success');
      return;
    }
    this.toast(message, 'info', { label: 'Show progress', run: () => (this.openTaskId = taskId) });
  }
}

export const ui = new Ui();

/**
 * Runs an async action with error toasts. Returns the result, or undefined
 * when it failed.
 */
export async function attempt<T>(fn: () => Promise<T>, errorPrefix?: string): Promise<T | undefined> {
  try {
    return await fn();
  } catch (err) {
    ui.error(err, errorPrefix);
    return undefined;
  }
}
