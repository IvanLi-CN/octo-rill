# Web PWA app shell

## Context and Scope

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

- `REQ-PWA-MANIFEST`: `manifest.webmanifest` MUST use a stable root `id`, `display: standalone`, root `scope`, root `start_url`, OctoRill name metadata, categories, install icons including a maskable 512px PNG, screenshots, and shortcuts for primary app destinations.
- `REQ-PWA-ICON-ID`: Production install icon URLs MUST include a content hash derived from the served PNG bytes. When icon content changes, the manifest MUST publish a new URL while `id`, `scope`, and `start_url` remain unchanged. The hash MUST be computed from the production PNG bytes that the server will serve.
- `REQ-PWA-HTML-ICON`: Product App HTML MUST NOT declare `rel="apple-touch-icon"`; browsers with manifest-driven installation MUST use the Manifest as the sole product install icon metadata source. The docs site may keep an independent icon path outside this topic.
- `REQ-PWA-NETWORK`: The Service Worker MUST ignore non-GET requests, cross-origin requests, `/api/**`, and `/auth/**`.
- `REQ-PWA-NAVIGATION`: Document navigations SHOULD prefer the network and fall back to the cached app shell only when the network is unavailable.
- `REQ-PWA-PRECACHE`: The precache list MUST come from an explicit allowlist covering only app-shell files, safe PWA screenshots, brand/favicon assets, static reaction icons, and Vite build artifacts. It MUST exclude `manifest.webmanifest` and all install icons.
- `REQ-PWA-CACHE`: Vite hashed build assets and content-hashed install icon PNGs MAY use long immutable cache headers. `index.html`, `sw.js`, and `manifest.webmanifest` MUST NOT use long immutable cache headers.
- `REQ-PWA-UPDATE-CHECK`: The app SHOULD ask the registered Service Worker to check for updates when the page becomes visible and when backend version polling detects frontend version drift.
- `REQ-PWA-WAITING-WORKER`: A waiting Service Worker MUST be activated only after the user clicks the existing refresh action; the click MUST read the current `registration.waiting` worker rather than retaining the worker first discovered.
- `REQ-PWA-ACTIVATION`: After `SKIP_WAITING`, the page MUST wait for `controllerchange` and reload once. If activation has not completed within 15 seconds, the notice MUST return to an enabled retry action.
- `REQ-PWA-WAIT-UNTIL`: The Service Worker MUST call `skipWaiting()` inside `event.waitUntil()` so the browser keeps the message event alive through the activation request.
- `REQ-PWA-NOTICE-LAYOUT`: The full-width app-shell update notice MUST keep its message and complete action group on one row when their content fits; otherwise the complete action group MUST move to the next row and remain right-aligned. Visible action controls keep their established visual dimensions and a touch hit area of at least 36px.
- `REQ-PWA-OFFLINE-ANONYMOUS`: Offline anonymous boot MUST distinguish network unavailability from authentication failure and keep login-only actions visibly unavailable until connectivity is restored.
- `REQ-PWA-OFFLINE-AUTHENTICATED`: Offline authenticated boot SHOULD reuse recent auth and dashboard warm caches: if the active page has cached content, show a small offline cache notice while preserving that content; if it has no cache, show an offline empty state with retry.
- `REQ-PWA-QUERY-PERSISTENCE`: Dashboard server-state MAY use short-lived React Query persistence only for whitelisted Dashboard query keys. `/api/**`, `/auth/**`, OAuth, passkey, SSE, mutation, and admin detail responses MUST remain network-only and MUST NOT be cached by the PWA layer.

## Verification

- `VER-PWA-MANIFEST`: Production build and manifest checks cover: `REQ-PWA-MANIFEST`, `REQ-PWA-ICON-ID`, `REQ-PWA-HTML-ICON`.
- `VER-PWA-CACHE`: Build and static-server cache contracts cover: `REQ-PWA-PRECACHE`, `REQ-PWA-CACHE`.
- `VER-PWA-NETWORK`: Service Worker browser checks cover: `REQ-PWA-NETWORK`, `REQ-PWA-NAVIGATION`, `REQ-PWA-UPDATE-CHECK`.
- `VER-PWA-UPDATE`: Chromium lifecycle tests cover: `REQ-PWA-WAITING-WORKER`, `REQ-PWA-ACTIVATION`, `REQ-PWA-WAIT-UNTIL`.
- `VER-PWA-SHELL`: Responsive shell and offline browser checks cover: `REQ-PWA-NOTICE-LAYOUT`, `REQ-PWA-OFFLINE-ANONYMOUS`, `REQ-PWA-OFFLINE-AUTHENTICATED`, `REQ-PWA-QUERY-PERSISTENCE`.

## Related ADRs

None

## Acceptance

- Production build emits `manifest.webmanifest`, `sw.js`, `pwa-precache-manifest.json`, and PNG app icons.
- Browser installability checks identify the app as installable and expose the declared screenshots and shortcuts.
- The build and static-server contracts prove manifest identity stability, content-hashed icon URL/content consistency, revalidation of install metadata, immutable caching for hashed icons, and Service Worker exclusion of install metadata.
- A browser-owned Chromium V1-to-V2 update regression uses one browser context to prove normal startup and update checking obtain the V2 Manifest and hashed install icons without an uninstall or reinstall step. It is a network/update contract and does not claim an OS-installed record; a dedicated real-installed PWA regression is available with `OCTORILL_REAL_PWA_TEST=1` on a ChromeOS runner with the DevTools PWA handler. This does not claim automatic migration of existing iOS/iPadOS Web Clips.
- Auth, passkey, OAuth, API, and SSE paths continue to use network behavior.
- The existing version update notice can represent both server version drift and Service Worker update availability.
- A same-release Service Worker update activates on one user action, deletes its prior precache, and does not re-show the notice after the page reloads.
- Worker-only updates use resource-update copy; the footer continues to show the release version embedded in the loaded page.
- At 375px, a short update message and the refresh action remain on one row; at 320px or with the update-plus-install state, the action group wraps as one unit and remains right-aligned.
- The app shell can render a clear offline boundary when `/api/me` cannot be reached from a cached PWA shell.
- Already-authenticated offline visits preserve active-page warm feed content when available and show a distinct no-cache offline empty state when unavailable.
- Browser Back/Forward and short-lived PWA restores may reuse Dashboard React Query cache for up to 1 hour, then reconcile through normal network requests without caching private API responses in the Service Worker.

## Platform Update Contract

- Chromium desktop and Android Chrome/WebAPK installations use the manifest identity and `icons` members to recognize and update the same installed application. The implementation keeps `id`, `scope`, and `start_url` stable and changes an icon URL when its bytes change, which is the supported manifest signal for an icon update. The Service Worker update notice remains an app-shell refresh control; it is not the install metadata update mechanism.
- The default automated release regression uses one Chromium browser context and the browser-owned Manifest loader for V1 and V2; it verifies stable identity and fresh V2 manifest/icon retrieval without exercising an install prompt. The dedicated real-installed regression, enabled with `OCTORILL_REAL_PWA_TEST=1` on a ChromeOS runner with the DevTools PWA handler, installs V1 once, launches the actual installed app window, and retrieves V2 metadata in that same app without a second install. These checks cover the Chromium manifest/update path used by Chromium desktop and Android Chrome/WebAPK, while release verification still requires an actual installed-app check on those target platforms.
- Existing iOS/iPadOS Web Clips and browsers that do not apply manifest-driven installation are platform-owned shortcuts. This topic makes no claim that a web response can force-migrate an existing Web Clip or another browser's stored icon, and does not include them in the automatic update acceptance path.
- Contract references: [Web Application Manifest](https://www.w3.org/TR/appmanifest/), [Chrome web app update behavior](https://developer.chrome.com/blog/improvements-to-web-app-updates), and [Apple Web Clip icon configuration](https://developer.apple.com/library/archive/documentation/AppleApplications/Reference/SafariWebContent/ConfiguringWebApplications/ConfiguringWebApplications.html).

## Visual Evidence

- Update + install state: [update-install.png](./assets/update-install.png)
- Install-only state: [install-only.png](./assets/install-only.png)
- Update-only state: [update-only.png](./assets/update-only.png)
- Mobile update-only content feed: [update-only-mobile.png](./assets/update-only-mobile.png)
- Mobile update-and-install content feed: [update-install-mobile.png](./assets/update-install-mobile.png)
- Offline anonymous boot fallback: [offline-boot-fallback.png](./assets/offline-boot-fallback.png)
- Offline authenticated dashboard with cached content: [offline-dashboard-cached-content.png](./assets/offline-dashboard-cached-content.png)
- Offline authenticated dashboard without cached content: [offline-dashboard-empty-state.png](./assets/offline-dashboard-empty-state.png)
- Approved regular/maskable install artwork parity is validated from source and production PNG pixels; this metadata-only change does not require new app-shell screenshots.
