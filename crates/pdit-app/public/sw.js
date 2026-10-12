// pdit's service worker (online version only, M-007): keeps the app's own files so pdit opens and works offline after
// the first visit. Never sees PDFs: they are opened from the visitor's disk, not fetched. Only this site's files are
// cached. "pdit-dev" is replaced per deployment (.github/workflows/pages.yml), so a new version gets a fresh cache;
// the old one is dropped once no tab uses it any more (no skipWaiting: an open tab keeps its files).
const CACHE = 'pdit-dev';

// precache.txt (written by the deploy workflow) lists every file, so fonts and models load offline before first use.
self.addEventListener('install', event => {
  event.waitUntil((async () => {
    const cache = await caches.open(CACHE);
    const list = await fetch('precache.txt', { cache: 'no-store' }).then(r => (r.ok ? r.text() : ''), () => '');
    const files = ['./', ...list.split('\n').map(f => f.trim()).filter(Boolean)];
    await cache.addAll(files);
  })());
});

self.addEventListener('activate', event => {
  event.waitUntil((async () => {
    for (const key of await caches.keys()) {
      if (key.startsWith('pdit-') && key !== CACHE) await caches.delete(key);
    }
    await self.clients.claim();
  })());
});

self.addEventListener('fetch', event => {
  const request = event.request;
  if (request.method !== 'GET' || new URL(request.url).origin !== self.location.origin) return;
  event.respondWith((async () => {
    const cache = await caches.open(CACHE);
    // The page itself: newest when online, the kept one offline.
    if (request.mode === 'navigate') {
      try {
        const response = await fetch(request);
        if (response.ok) cache.put('./', response.clone());
        return response;
      } catch {
        return (await cache.match('./')) ?? Response.error();
      }
    }
    // Everything else has its content's hash in its name: kept once fetched.
    const kept = await cache.match(request);
    if (kept) return kept;
    const response = await fetch(request);
    if (response.ok) cache.put(request, response.clone());
    return response;
  })());
});
