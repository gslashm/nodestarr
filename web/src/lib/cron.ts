// Schedules are cron expressions ("minute hour day month weekday"). These
// helpers describe the common ones in words and offer presets, so most
// people never have to write one.

import { clockTime as clock } from './format';

const DAYS = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];

/** Plain-language description of a cron expression, or null when it isn't a common shape. */
export function describeCron(expr: string): string | null {
  const parts = expr.trim().split(/\s+/);
  if (parts.length !== 5) {
    const alias: Record<string, string> = { '@hourly': 'Every hour', '@daily': 'Every day at midnight', '@weekly': 'Every Sunday at midnight', '@monthly': 'On the 1st of every month' };
    return alias[expr.trim()] ?? null;
  }
  const [min, hour, dom, mon, dow] = parts;
  const num = (s: string) => (/^\d+$/.test(s) ? Number(s) : null);
  const m = num(min);
  const h = num(hour);
  if (mon !== '*') return null;

  const every = /^\*\/(\d+)$/;
  if (dom === '*' && dow === '*') {
    if (every.test(min) && hour === '*') {
      const n = Number(every.exec(min)![1]);
      return n === 1 ? 'Every minute' : `Every ${n} minutes`;
    }
    if (m !== null && hour === '*') return `Every hour at :${String(m).padStart(2, '0')}`;
    if (m !== null && every.test(hour)) {
      const n = Number(every.exec(hour)![1]);
      return n === 1 ? `Every hour at :${String(m).padStart(2, '0')}` : `Every ${n} hours, at :${String(m).padStart(2, '0')}`;
    }
    if (m !== null && h !== null) return `Every day at ${clock(h, m)}`;
    if (m !== null && /^\d+(,\d+)+$/.test(hour)) return `Every day at ${hour.split(',').map((x) => clock(Number(x), m)).join(', ')}`;
  }
  if (dom === '*' && m !== null && h !== null) {
    if (/^\d$/.test(dow)) return `Every ${DAYS[Number(dow) % 7]} at ${clock(h, m)}`;
    if (dow === '1-5') return `Weekdays at ${clock(h, m)}`;
    if (dow === '0,6' || dow === '6,0') return `Weekends at ${clock(h, m)}`;
  }
  if (dow === '*' && m !== null && h !== null && num(dom) !== null) return `On day ${dom} of every month at ${clock(h, m)}`;
  return null;
}

export interface CronPreset {
  label: string;
  value: string;
}

/** Presets offered next to a schedule field, labeled in the configured time format. */
export function cronPresets(kind: 'check' | 'daily'): CronPreset[] {
  if (kind === 'check') {
    return [
      { label: 'Every hour', value: '17 * * * *' },
      { label: 'Every 6 hours', value: '17 */6 * * *' },
      { label: 'Every 12 hours', value: '17 */12 * * *' },
      { label: 'Once a day', value: '17 5 * * *' },
    ];
  }
  return [
    { label: `Every night at ${clock(3, 30)}`, value: '30 3 * * *' },
    { label: `Every night at ${clock(4, 0)}`, value: '0 4 * * *' },
    { label: `Sundays at ${clock(4, 0)}`, value: '0 4 * * 0' },
    { label: `Weekdays at ${clock(2, 0)}`, value: '0 2 * * 1-5' },
  ];
}
