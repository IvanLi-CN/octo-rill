# Web PWA app shell implementation

## Current Coverage

- Implementation covers the main Vite web app only.
- PWA shell uses generated static assets and a conservative same-origin Service Worker.
- Manifest now carries install identity, shortcuts, and screenshots sourced from stable app-shell evidence.
- Version notice now surfaces both Service Worker refresh and native install prompt actions in one shared shell surface.
- Anonymous boot now presents network-aware offline copy when the cached app shell loads but `/api/me` cannot be reached, and keeps login-only actions visibly unavailable until retry.
- Authenticated boot now preserves recent dashboard warm feed content even when lazy route loading races auth reconciliation, and distinguishes active-page no-cache offline state from cached-content offline state.
- Feed initial and append failures carry network-aware kind/detail metadata so offline API misses do not look like empty data or auth failures.
- Precache generation uses an explicit allowlist for app shell, PWA, brand/favicon, static reaction icons, and Vite build assets instead of broad extension-based inclusion.
- Version monitoring can ask the registered Service Worker to check for updates on page visibility and observed version drift while preserving user-confirmed activation.
- Axum static hosting is responsible for cache headers that keep app-shell updates discoverable.

## Validation

- Frontend production build completes with generated `sw.js` and `pwa-precache-manifest.json`.
- Build contract coverage checks the generated manifest metadata, shortcuts, screenshots, PNG icon dimensions, precache URL safety, and Service Worker cache-bypass guards.
- Static server tests cover SPA fallback, app-shell cache headers, Service Worker cache headers, manifest cache headers, and immutable hashed assets.
- Browser runtime checks confirm manifest metadata, standalone/iOS install meta, screenshots, shortcuts, maskable icon declaration, same-origin Service Worker registration, offline app-shell fallback, offline anonymous boot copy, authenticated offline cached-content and no-cache states, private path network bypass, update-triggered Service Worker checks, waiting Service Worker refresh activation, and install prompt behavior.
- Storybook covers the Landing offline boot fallback and Dashboard authenticated offline cache/no-cache states as stable visual evidence sources.

## Evidence

- Browser preview confirmed manifest metadata, maskable icon declaration, same-origin Service Worker registration, and `/api/**` network behavior.
- Automated coverage includes the PWA metadata Playwright smoke test, production Service Worker offline/update Playwright checks, authenticated offline warm-cache/no-cache checks, install prompt behavior checks, PWA build contract test, and Axum static cache header tests.
- Visual evidence captures the Storybook Landing offline boot fallback and authenticated Dashboard offline states at mobile viewports from mock-only sources.

## Remaining Gaps

- Push notifications, background sync, and offline writes remain explicitly out of scope.
