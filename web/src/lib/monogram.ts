// Monogram avatars for apps, colored like shipping-line containers. Berth
// doesn't bundle third-party logos; an icon URL template can be set in
// Settings > General to use an icon set you host.

const COLORS = [
  '#3f9fcf', // sea blue
  '#2f8f5b', // evergreen
  '#d9702c', // signal orange
  '#b98b25', // mustard
  '#33508f', // navy
  '#a3473a', // rust
  '#2a9d8f', // teal
  '#b02d78', // magenta
  '#6a7b89', // steel
];

function hash(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

export function monogramColor(key: string): string {
  return COLORS[hash(key.toLowerCase()) % COLORS.length];
}

/** One or two letters: "Sonarr" → "So", "immich-server" → "Is", "Home Assistant" → "HA". */
export function monogramText(name: string): string {
  const words = name
    .replace(/[^\p{L}\p{N}]+/gu, ' ')
    .trim()
    .split(' ')
    .filter(Boolean);
  if (words.length >= 2 && /^\p{Lu}/u.test(words[1])) return (words[0][0] + words[1][0]).toUpperCase();
  const w = words[0] ?? '?';
  return w.length > 1 ? w[0].toUpperCase() + w[1].toLowerCase() : w.toUpperCase();
}

/** Icon URL from the configured template, or null for a monogram. */
export function iconUrl(template: string, slug: string | null, override: string): string | null {
  if (override) return override;
  if (!template || !slug) return null;
  return template.replaceAll('{name}', slug);
}
