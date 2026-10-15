// Thin client for Nodestarr's REST API, live event stream and WebSockets.
//
// Every request carries `X-Nodestarr-Request: 1`, which the server requires for
// cookie-authenticated changes (browsers can't add custom headers to
// cross-site requests without a CORS preflight Nodestarr never approves).

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

/** Path prefix Nodestarr is served under ("" or e.g. "/nodestarr"), from <base href>. */
export function basePath(): string {
  return new URL('.', document.baseURI).pathname.replace(/\/$/, '');
}

export function apiUrl(path: string): string {
  return `${basePath()}/api/v1${path}`;
}

/** ws:// or wss:// URL for an API path. */
export function wsUrl(path: string): string {
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${location.host}${apiUrl(path)}`;
}

let unauthorizedHandler: (() => void) | null = null;

/** Called when the server answers 401 (the session ended). */
export function onUnauthorized(handler: () => void): void {
  unauthorizedHandler = handler;
}

interface RequestOptions {
  signal?: AbortSignal;
  /** Don't trigger the global sign-out handler on 401 (auth endpoints). */
  quiet401?: boolean;
}

async function request<T>(method: string, path: string, body?: unknown, opts: RequestOptions = {}): Promise<T> {
  const headers: Record<string, string> = { 'X-Nodestarr-Request': '1', Accept: 'application/json' };
  let payload: string | undefined;
  if (body !== undefined) {
    headers['Content-Type'] = 'application/json';
    payload = JSON.stringify(body);
  }
  let resp: Response;
  try {
    resp = await fetch(apiUrl(path), { method, headers, body: payload, credentials: 'same-origin', signal: opts.signal });
  } catch (err) {
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    throw new ApiError(0, 'network', 'Nodestarr is not reachable. Check that it is running and that this device is on the same network.');
  }
  if (resp.status === 401 && !opts.quiet401) unauthorizedHandler?.();
  const text = await resp.text();
  let data: unknown = null;
  if (text) {
    try {
      data = JSON.parse(text);
    } catch {
      data = text;
    }
  }
  if (!resp.ok) {
    const err = (data as { error?: { code?: string; message?: string } } | null)?.error;
    const message = err?.message ?? (typeof data === 'string' && data ? data : resp.statusText || `HTTP ${resp.status}`);
    throw new ApiError(resp.status, err?.code ?? 'error', capitalize(message));
  }
  return data as T;
}

function capitalize(s: string): string {
  return s ? s[0].toUpperCase() + s.slice(1) : s;
}

export const api = {
  get: <T>(path: string, opts?: RequestOptions) => request<T>('GET', path, undefined, opts),
  post: <T>(path: string, body: unknown = {}, opts?: RequestOptions) => request<T>('POST', path, body, opts),
  put: <T>(path: string, body: unknown = {}, opts?: RequestOptions) => request<T>('PUT', path, body, opts),
  del: <T>(path: string, opts?: RequestOptions) => request<T>('DELETE', path, undefined, opts),
};

/** Builds a query string, skipping empty values. */
export function qs(params: Record<string, string | number | boolean | null | undefined>): string {
  const out = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === '' || v === false) continue;
    out.set(k, v === true ? '1' : String(v));
  }
  const s = out.toString();
  return s ? `?${s}` : '';
}

/** Percent-encodes one path segment (container identities contain ':' and '/'). */
export function seg(s: string): string {
  return encodeURIComponent(s);
}

/** A readable message from anything thrown. */
export function errorMessage(err: unknown): string {
  if (err instanceof ApiError || err instanceof Error) return err.message;
  return String(err);
}
