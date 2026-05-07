# Web PWA app shell implementation

## Current Coverage

- Implementation covers the main Vite web app only.
- PWA shell uses generated static assets and a conservative same-origin Service Worker.
- Axum static hosting is responsible for cache headers that keep app-shell updates discoverable.

## Validation

- Frontend production build completes with generated `sw.js` and `pwa-precache-manifest.json`.
- Static server tests cover SPA fallback, app-shell cache headers, Service Worker cache headers, manifest cache headers, and immutable hashed assets.
- Browser runtime checks confirm manifest metadata, maskable icon declaration, same-origin Service Worker registration, and `/api/**` network behavior.

## Evidence

- Browser preview confirmed manifest metadata, maskable icon declaration, same-origin Service Worker registration, and `/api/**` network behavior.
- Automated coverage includes the PWA metadata Playwright smoke test and Axum static cache header tests.

## Remaining Gaps

- Push notifications, background sync, and offline writes remain explicitly out of scope.
