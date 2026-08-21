// Service worker: offline use, without the risk of a permanently stale app.
//
// PatchFerret is the tool you open in a production office with borrowed wifi,
// or on a tablet in a truck. Everything already happens in the tab — the show
// file is never uploaded — so the only thing standing between it and working
// with no signal is that the page has to be fetched at all.
//
// The strategy here differs from the rest of the fleet's tools on purpose.
// They are Vite builds whose asset filenames carry a content hash, so a given
// URL's bytes can never change and cache-first is safe. **Nothing here is
// hashed.** `patchferret.js` and `patchferret.wasm` keep their names across
// every deploy, so cache-first would pin whichever build happened to be live
// when someone first visited, forever, with no way for them to know. That is
// the failure worth designing against: a wrong patch list is worse than no
// patch list.
//
// So everything same-origin is network-first, and the cache only answers when
// the network does not:
//
//   online  -> always the current build
//   offline -> the last build that was seen
//
// This costs a round trip per asset, which is cheap: Cloudflare serves these
// with `must-revalidate` and an ETag, so an unchanged 400 kB wasm answers 304
// with no body.
//
// Bump CACHE when the caching behaviour itself changes. Old caches are deleted
// on activate, so a bump costs one cold fetch.

const CACHE = 'patchferret-v1';

// Enough to boot with no network. The wasm is the app, not an extra.
const SHELL = [
  '/',
  '/patchferret.js',
  '/patchferret.wasm',
  '/manifest.webmanifest',
  '/icon.svg',
];

self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(CACHE).then(async (cache) => {
      // Individually, so one missing entry cannot fail the whole install and
      // silently leave the previous worker in place.
      await Promise.all(SHELL.map((url) => cache.add(url).catch(() => {})));
      await self.skipWaiting();
    }),
  );
});

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(names.filter((n) => n !== CACHE).map((n) => caches.delete(n)));
      await self.clients.claim();
    })(),
  );
});

self.addEventListener('fetch', (event) => {
  const { request } = event;
  if (request.method !== 'GET') return;

  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;

  event.respondWith(
    (async () => {
      try {
        const fresh = await fetch(request);
        // Only store a real answer. Caching an error page under the wasm's URL
        // would make the app broken offline rather than merely absent.
        if (fresh.ok) {
          const cache = await caches.open(CACHE);
          cache.put(request.mode === 'navigate' ? '/' : request, fresh.clone());
        }
        return fresh;
      } catch {
        const cached =
          (await caches.match(request)) ||
          (request.mode === 'navigate' ? await caches.match('/') : undefined);
        if (cached) return cached;
        throw new Error('offline and nothing cached');
      }
    })(),
  );
});
