# Web PWA app shell implementation

## Current Coverage

- Implementation covers the main Vite web app only.
- PWA shell uses generated static assets and a conservative same-origin Service Worker.
- Manifest carries stable install identity, shortcuts, and screenshots sourced from stable app-shell evidence. Production postbuild rewrites only the three regular/maskable install icon URLs to content-hashed filenames derived from the approved PNG bytes.
- The postbuild step hashes the clean Vite output before renaming each install icon, then rewrites the manifest; it fails if the source and metadata sets diverge. Product HTML has no Apple touch icon link or generated Apple icon asset.
- Chromium install metadata remains manifest-authoritative. iOS standalone meta tags remain descriptive compatibility metadata, but iOS/iPadOS Web Clips are not part of the automatic icon update contract.
- Version notice now surfaces both Service Worker refresh and native install prompt actions in one shared shell surface.
- Anonymous boot now presents network-aware offline copy when the cached app shell loads but `/api/me` cannot be reached, and keeps login-only actions visibly unavailable until retry.
- Authenticated boot now preserves recent dashboard warm feed content even when lazy route loading races auth reconciliation, and distinguishes active-page no-cache offline state from cached-content offline state.
- Dashboard server-state now has a React Query persisted cache for whitelisted Dashboard query keys with a 1 hour max age; this improves Back/Forward and short-lived PWA restores without putting private API responses into the Service Worker cache.
- Feed initial and append failures carry network-aware kind/detail metadata so offline API misses do not look like empty data or auth failures.
- Precache generation uses an explicit allowlist for app shell, safe PWA screenshots, brand/favicon, static reaction icons, and Vite build assets instead of broad extension-based inclusion. Manifest and install icon requests stay outside the Service Worker cache.
- Version monitoring can ask the registered Service Worker to check for updates on page visibility and observed version drift while preserving user-confirmed activation.
- Axum static hosting is responsible for cache headers that keep app-shell updates discoverable.

## Validation

- Frontend production build completes with generated `sw.js` and `pwa-precache-manifest.json`.
- Build contract coverage parses generated HTML to check the sole product Manifest link and absence of an Apple touch icon link, then checks manifest identity, content-hashed icon URLs, byte-for-byte parity with approved `web/public/pwa` artwork, shortcuts, screenshots, PNG icon dimensions, precache URL safety, and Service Worker metadata cache-bypass guards. CI runs this post-build contract.
- Static server tests cover SPA fallback, app-shell cache headers, Service Worker cache headers, manifest revalidation headers, stable icon revalidation, and immutable hashed PWA assets.
- Browser runtime checks confirm manifest metadata, absence of the product Apple touch icon link, standalone/iOS install meta, screenshots, shortcuts, maskable icon declaration, same-origin Service Worker registration, offline app-shell fallback, offline anonymous boot copy, authenticated offline cached-content and no-cache states, private path network bypass, network-revalidated install metadata, same-context Chromium V1-to-V2 Manifest/icon retrieval through the browser-owned CDP Manifest parser, update-triggered Service Worker checks, waiting Service Worker refresh activation, and install prompt behavior. A separate `OCTORILL_REAL_PWA_TEST=1` path uses the ChromeOS DevTools PWA handler to install V1 once, launch the real installed window, and verify V2 metadata without reinstalling.
- Storybook covers the Landing offline boot fallback and Dashboard authenticated offline cache/no-cache states as stable visual evidence sources.

## Evidence

- Browser preview confirmed manifest identity metadata, content-hashed install icon URLs, maskable icon declaration, same-origin Service Worker registration, install metadata network revalidation, and `/api/**` network behavior.
- Automated coverage includes the PWA metadata Playwright smoke test, production Service Worker offline/update Playwright checks, authenticated offline warm-cache/no-cache checks, install prompt behavior checks, install metadata network-bypass checks, PWA build contract test, and Axum static cache header tests.
- The manifest contract is aligned with Chromium desktop and Android Chrome/WebAPK update semantics: stable identity preserves the installed app, while a changed icon URL signals the new icon. The default V1-to-V2 regression uses the browser-owned CDP Manifest parser in one browser context and proves the changed hashed URL and fresh bytes are retrievable; the optional ChromeOS path exercises the actual installed app window. Approved icon pixels remain unchanged, and Apple Web Clips remain explicitly limited to user-controlled device-owned state.
- Visual evidence captures the Storybook Landing offline boot fallback and authenticated Dashboard offline states at mobile viewports from mock-only sources.
- Approved regular/maskable install artwork is verified by exact production-to-source PNG byte and pixel parity; no app-shell screenshot changed because this release changes only installation metadata delivery.

## Remaining Gaps

- Push notifications, background sync, and offline writes remain explicitly out of scope.
