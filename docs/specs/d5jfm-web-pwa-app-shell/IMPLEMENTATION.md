# Web PWA app shell implementation

## Current Coverage

- Implementation covers the main Vite web app only.
- PWA shell uses generated static assets and a conservative same-origin Service Worker.
- Manifest metadata includes install icons, app screenshots, shortcuts, categories, and a stable root app id.
- Precache generation uses an explicit allowlist for app shell, PWA, brand/favicon, and Vite build assets instead of broad extension-based inclusion.
- Version monitoring can ask the registered Service Worker to check for updates on page visibility and observed version drift while preserving user-confirmed activation.
- Axum static hosting is responsible for cache headers that keep app-shell updates discoverable.

## Validation

- Frontend production build completes with generated `sw.js` and `pwa-precache-manifest.json`.
- Build contract coverage checks the generated manifest metadata, PNG icon and screenshot dimensions, shortcuts, precache URL safety, and Service Worker cache-bypass guards.
- Static server tests cover SPA fallback, app-shell cache headers, Service Worker cache headers, manifest cache headers, and immutable hashed assets.
- Browser runtime checks confirm manifest metadata, screenshots, shortcuts, maskable icon declaration, same-origin Service Worker registration, offline app-shell fallback, private path network bypass, update-triggered Service Worker checks, and waiting Service Worker refresh activation.

## Evidence

- Browser preview confirmed manifest metadata, maskable icon declaration, same-origin Service Worker registration, and `/api/**` network behavior.
- Automated coverage includes the PWA metadata Playwright smoke test, production Service Worker offline/update Playwright checks, PWA build contract test, and Axum static cache header tests.

## Remaining Gaps

- Push notifications, background sync, and offline writes remain explicitly out of scope.
