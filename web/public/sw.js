// Berth service worker: lets the app be installed and shows a clear page
// when the server can't be reached. API calls and live data always go to
// the network; only the app shell and its hashed assets are cached.
const SHELL = 'berth-shell-v2';
const ASSETS = 'berth-assets-v2';
// Hashed assets from older versions are dropped beyond this many entries
// (a build has roughly 200, most of them loaded only when a page needs them).
const MAX_ASSETS = 400;

self.addEventListener('install', (event) => {
  event.waitUntil(caches.open(SHELL).then((c) => c.add('./')).then(() => self.skipWaiting()));
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) => Promise.all(keys.filter((k) => k !== SHELL && k !== ASSETS).map((k) => caches.delete(k))))
      .then(() => self.clients.claim()),
  );
});

let trimming = null;

/** Deletes the oldest cached assets beyond MAX_ASSETS (keys come back in insertion order). */
function trimAssets() {
  trimming ??= caches
    .open(ASSETS)
    .then(async (cache) => {
      const keys = await cache.keys();
      for (const key of keys.slice(0, Math.max(0, keys.length - MAX_ASSETS))) await cache.delete(key);
    })
    .finally(() => (trimming = null));
  return trimming;
}

self.addEventListener('fetch', (event) => {
  const req = event.request;
  if (req.method !== 'GET') return;
  const url = new URL(req.url);
  if (url.origin !== location.origin || url.pathname.includes('/api/') || url.pathname.endsWith('/metrics')) return;
  if (req.mode === 'navigate') {
    // Network first for pages, so updates show up immediately.
    event.respondWith(
      fetch(req)
        .then((resp) => {
          if (resp.ok) {
            const copy = resp.clone();
            event.waitUntil(caches.open(SHELL).then((c) => c.put('./', copy)));
          }
          return resp;
        })
        .catch(() => caches.match('./', { cacheName: SHELL }).then((r) => r || new Response('Berth is not reachable right now.', { status: 503, headers: { 'Content-Type': 'text/plain' } }))),
    );
    return;
  }
  if (url.pathname.includes('/assets/')) {
    // Hashed assets never change: cache first.
    event.respondWith(
      caches.match(req, { cacheName: ASSETS }).then(
        (hit) =>
          hit ||
          fetch(req).then((resp) => {
            if (resp.ok) {
              const copy = resp.clone();
              event.waitUntil(
                caches
                  .open(ASSETS)
                  .then((c) => c.put(req, copy))
                  .then(trimAssets),
              );
            }
            return resp;
          }),
      ),
    );
  }
});
