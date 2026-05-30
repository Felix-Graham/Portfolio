// ─────────────────────────────────────────────
// sw.js — Service Worker
// Caches app shell + all vocab files for offline use
// ─────────────────────────────────────────────

const CACHE = "quiz-v1"

const APP_SHELL = [
    "/",
    "/index.html",
    "/quiz.js",
    "/manifest.json"
]

// On install: cache app shell
self.addEventListener("install", e => {
    e.waitUntil(
        caches.open(CACHE).then(cache => cache.addAll(APP_SHELL))
    )
    self.skipWaiting()
})

// On activate: clear old caches
self.addEventListener("activate", e => {
    e.waitUntil(
        caches.keys().then(keys =>
            Promise.all(keys.filter(k => k !== CACHE).map(k => caches.delete(k)))
        )
    )
    self.clients.claim()
})

// On fetch: cache-first for vocab, network-first for everything else
self.addEventListener("fetch", e => {
    const url = new URL(e.request.url)

    // Cache-first for vocab files (they don't change often)
    if (url.pathname.startsWith("/vocab/")) {
        e.respondWith(
            caches.match(e.request).then(cached => {
                if (cached) return cached
                return fetch(e.request).then(res => {
                    const clone = res.clone()
                    caches.open(CACHE).then(cache => cache.put(e.request, clone))
                    return res
                })
            })
        )
        return
    }

    // Network-first for app shell
    e.respondWith(
        fetch(e.request)
            .then(res => {
                const clone = res.clone()
                caches.open(CACHE).then(cache => cache.put(e.request, clone))
                return res
            })
            .catch(() => caches.match(e.request))
    )
})
