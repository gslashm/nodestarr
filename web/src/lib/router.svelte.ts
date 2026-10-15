// A small history-API router. Paths are app-relative ("/containers/x");
// the URL base Nodestarr runs under is added and removed here.

import { basePath } from './api';

class Router {
  /** App-relative path, e.g. "/containers/media-sonarr-1". */
  path = $state('/');
  query = $state(new URLSearchParams());

  constructor() {
    this.sync();
    window.addEventListener('popstate', () => this.sync());
  }

  private sync(): void {
    const base = basePath();
    const full = decodeURI(location.pathname);
    let path = base && full.startsWith(base) ? full.slice(base.length) : full;
    if (!path.startsWith('/')) path = `/${path}`;
    this.path = path.length > 1 ? path.replace(/\/+$/, '') : path;
    this.query = new URLSearchParams(location.search);
  }

  /** Navigates to an app-relative path (may include ?query). */
  navigate(to: string, opts: { replace?: boolean; keepScroll?: boolean } = {}): void {
    const url = href(to);
    if (url === location.pathname + location.search) return;
    if (opts.replace) history.replaceState(null, '', url);
    else history.pushState(null, '', url);
    this.sync();
    if (!opts.keepScroll) document.getElementById('main')?.scrollTo({ top: 0 });
  }

  /** Updates query parameters in place (filters, tabs) without a history entry. */
  setQuery(params: Record<string, string | null | undefined>): void {
    const q = new URLSearchParams(location.search);
    for (const [k, v] of Object.entries(params)) {
      if (v === null || v === undefined || v === '') q.delete(k);
      else q.set(k, v);
    }
    const s = q.toString();
    this.navigate(`${this.path}${s ? `?${s}` : ''}`, { replace: true, keepScroll: true });
  }
}

export const router = new Router();

/** Full URL path for an app-relative path. */
export function href(to: string): string {
  return `${basePath()}${to.startsWith('/') ? to : `/${to}`}`;
}

/**
 * Matches "/containers/:id/:tab?" style patterns. Returns the decoded
 * params, or null when the path doesn't match.
 */
export function match(pattern: string, path: string): Record<string, string> | null {
  const pp = pattern.split('/').filter(Boolean);
  const sp = path.split('/').filter(Boolean);
  const params: Record<string, string> = {};
  for (let i = 0; i < pp.length; i++) {
    const p = pp[i];
    const optional = p.endsWith('?');
    const name = p.replace(/^:/, '').replace(/\?$/, '');
    const value = sp[i];
    if (p.startsWith(':')) {
      if (value === undefined) {
        if (optional) continue;
        return null;
      }
      params[name] = decodeURIComponent(value);
    } else if (p !== value) {
      return null;
    }
  }
  return sp.length <= pp.length ? params : null;
}

/**
 * Turns clicks on same-app links into client-side navigation. Installed
 * once on the document; plain <a href={href('/x')}> links just work.
 */
export function interceptLinks(): () => void {
  const onClick = (e: MouseEvent) => {
    if (e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    const a = (e.target as Element | null)?.closest('a');
    if (!a || a.target || a.hasAttribute('download') || a.dataset.external !== undefined) return;
    const url = new URL(a.href, location.href);
    if (url.origin !== location.origin) return;
    const base = basePath();
    if (base && !url.pathname.startsWith(base)) return;
    const path = url.pathname.slice(base.length) || '/';
    if (path.startsWith('/api/') || path === '/metrics') return;
    e.preventDefault();
    router.navigate(path + url.search);
  };
  document.addEventListener('click', onClick);
  return () => document.removeEventListener('click', onClick);
}
