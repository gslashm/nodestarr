// Theme, accent color and density. The server stores the defaults (Settings
// > General); this browser can override the theme (kept in localStorage).

const THEME_KEY = 'berth.theme';

export type ThemeChoice = 'dark' | 'light' | 'system';

export function storedTheme(): ThemeChoice | null {
  try {
    const v = localStorage.getItem(THEME_KEY);
    return v === 'dark' || v === 'light' || v === 'system' ? v : null;
  } catch {
    return null;
  }
}

export function storeTheme(choice: ThemeChoice | null): void {
  try {
    if (choice) localStorage.setItem(THEME_KEY, choice);
    else localStorage.removeItem(THEME_KEY);
  } catch {
    // Private mode or blocked storage: the choice just won't persist.
  }
}

let media: MediaQueryList | null = null;
let current: ThemeChoice = 'dark';

function resolve(choice: ThemeChoice): 'dark' | 'light' {
  if (choice !== 'system') return choice;
  return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

/** Applies a theme choice ("system" follows the OS and keeps following it). */
export function applyTheme(choice: ThemeChoice): void {
  current = choice;
  document.documentElement.dataset.theme = resolve(choice);
  if (!media) {
    media = window.matchMedia('(prefers-color-scheme: light)');
    media.addEventListener('change', () => {
      if (current === 'system') document.documentElement.dataset.theme = resolve('system');
    });
  }
  const meta = document.querySelector('meta[name="theme-color"]');
  meta?.setAttribute('content', resolve(choice) === 'light' ? '#ffffff' : '#121b23');
}

export function activeTheme(): 'dark' | 'light' {
  return document.documentElement.dataset.theme === 'light' ? 'light' : 'dark';
}

/** Sets the accent and a readable text color for it. */
export function applyAccent(hex: string): void {
  const valid = /^#[0-9a-f]{6}$/i.test(hex) ? hex : '#f2c230';
  const root = document.documentElement.style;
  root.setProperty('--accent', valid);
  root.setProperty('--accent-ink', luminance(valid) > 0.45 ? '#1d1600' : '#ffffff');
}

export function applyDensity(density: string): void {
  document.documentElement.dataset.density = density === 'compact' ? 'compact' : 'comfortable';
}

function luminance(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const lin = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * lin[0] + 0.7152 * lin[1] + 0.0722 * lin[2];
}
