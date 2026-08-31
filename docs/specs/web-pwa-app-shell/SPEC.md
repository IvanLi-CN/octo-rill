# Web PWA app shell

## Background

OctoRill is a frequently visited personal workbench. The web app should be installable from modern browsers while preserving the current authenticated, server-backed data model.

The PWA layer is an enhancement around the existing React + Vite app shell and Axum static hosting. It must not turn private API responses, OAuth redirects, passkey flows, or realtime task streams into cached offline behavior.

## Goals

- Make the main `web/` app installable through a valid Web App Manifest, theme metadata, and PNG icons derived from existing OctoRill brand assets.
- Provide a Service Worker that precaches only safe build artifacts and app-shell assets.
- Surface Service Worker updates through the existing version update notice pattern so the user chooses when to refresh.
- Keep Axum static cache headers aligned with PWA update safety.

## Non-goals

- No push notifications.
- No background sync or offline mutation queue.
- No private `/api/**` or `/auth/**` response caching.
- No docs-site PWA.

## Requirements

- `manifest.webmanifest` must use a stable root `id`, `display: standalone`, root `scope`, root `start_url`, OctoRill name metadata, categories, install icons including a maskable 512px PNG, screenshots, and shortcuts for primary app destinations.
- Production install icon URLs must include a content hash derived from the served PNG bytes. When icon content changes, the manifest must publish a new URL while `id`, `scope`, and `start_url` remain unchanged.
- Where a browser supports manifest-driven installation, the manifest is the authoritative install metadata source. The `apple-touch-icon` link is retained only as an explicitly legacy Apple Web Clip fallback and must not be treated as a second Chromium install icon source.
- The Service Worker must ignore non-GET requests, cross-origin requests, `/api/**`, and `/auth/**`.
- Document navigations should prefer the network and fall back to the cached app shell only when the network is unavailable.
- The precache list must be generated from an explicit allowlist covering only app-shell files, safe PWA screenshots, brand/favicon assets, static reaction icons, and Vite build artifacts. It must exclude `manifest.webmanifest`, all install icons, and the legacy Apple touch icon so install metadata is never pinned by the Service Worker.
- Build assets generated under Vite's hashed asset directory may be served with long immutable cache headers.
- `index.html`, `sw.js`, and `manifest.webmanifest` must not be served with long immutable cache headers; content-hashed install icon PNGs must be served with immutable cache headers.
- The app should proactively ask the registered Service Worker to check for updates when the page becomes visible and when backend version polling detects frontend version drift.
- A waiting Service Worker must be activated only after the user clicks the existing refresh action.
- Offline anonymous boot must distinguish network unavailability from authentication failure and keep login-only actions visibly unavailable until connectivity is restored.
- Offline authenticated boot should reuse recent auth and dashboard warm caches: if the active page has cached content, show a small offline cache notice while preserving that content; if the active page has no cache, show a large offline empty state with retry instead of a misleading empty list.
- Dashboard server-state may use short-lived React Query persistence for PWA responsiveness, but only for whitelisted Dashboard query keys. This does not relax the Service Worker rule: `/api/**`, `/auth/**`, OAuth, passkey, SSE, mutation, and admin detail responses remain network-only and are not cached by the PWA layer.

## Related ADRs

- None

## Acceptance

- Production build emits `manifest.webmanifest`, `sw.js`, `pwa-precache-manifest.json`, and PNG app icons.
- Browser installability checks identify the app as installable and expose the declared screenshots and shortcuts.
- The build and static-server contracts prove manifest identity stability, content-hashed icon URL/content consistency, revalidation of install metadata, immutable caching for hashed icons, and Service Worker exclusion of install metadata.
- Auth, passkey, OAuth, API, and SSE paths continue to use network behavior.
- The existing version update notice can represent both server version drift and Service Worker update availability.
- The app shell can render a clear offline boundary when `/api/me` cannot be reached from a cached PWA shell.
- Already-authenticated offline visits preserve active-page warm feed content when available and show a distinct no-cache offline empty state when unavailable.
- Browser Back/Forward and short-lived PWA restores may reuse Dashboard React Query cache for up to 1 hour, then reconcile through normal network requests without caching private API responses in the Service Worker.

## Platform Update Contract

- Chromium desktop and Android Chrome/WebAPK installations use the manifest identity and `icons` members to recognize and update the same installed application. The implementation keeps `id`, `scope`, and `start_url` stable and changes an icon URL when its bytes change, which is the supported manifest signal for an icon update. The Service Worker update notice remains an app-shell refresh control; it is not the install metadata update mechanism.
- Existing iOS/iPadOS Web Clips and browsers that do not apply manifest-driven installation are platform-owned shortcuts. The current hashed `apple-touch-icon` link supports newly created Apple Web Clips, but a web response cannot force-migrate an existing Web Clip or another browser's stored icon. This limitation is documented explicitly and is not a normal update path requiring users to reinstall.
- Contract references: [Web Application Manifest](https://www.w3.org/TR/appmanifest/), [Chrome web app update behavior](https://developer.chrome.com/blog/improvements-to-web-app-updates), and [Apple Web Clip icon configuration](https://developer.apple.com/library/archive/documentation/AppleApplications/Reference/SafariWebContent/ConfiguringWebApplications/ConfiguringWebApplications.html).

## Visual Evidence

- Update + install state: [update-install.png](./assets/update-install.png)
- Install-only state: [install-only.png](./assets/install-only.png)
- Update-only state: [update-only.png](./assets/update-only.png)
- Offline anonymous boot fallback: [offline-boot-fallback.png](./assets/offline-boot-fallback.png)
- Offline authenticated dashboard with cached content: [offline-dashboard-cached-content.png](./assets/offline-dashboard-cached-content.png)
- Offline authenticated dashboard without cached content: [offline-dashboard-empty-state.png](./assets/offline-dashboard-empty-state.png)
