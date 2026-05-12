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

- `manifest.webmanifest` must use `display: standalone`, root `scope`, root `start_url`, OctoRill name metadata, and install icons including a maskable 512px PNG.
- The Service Worker must ignore non-GET requests, cross-origin requests, `/api/**`, and `/auth/**`.
- Document navigations should prefer the network and fall back to the cached app shell only when the network is unavailable.
- Build assets generated under Vite's hashed asset directory may be served with long immutable cache headers.
- `index.html`, `sw.js`, and `manifest.webmanifest` must not be served with long immutable cache headers.
- A waiting Service Worker must be activated only after the user clicks the existing refresh action.

## Acceptance

- Production build emits `manifest.webmanifest`, `sw.js`, `pwa-precache-manifest.json`, and PNG app icons.
- Browser installability checks identify the app as installable.
- Auth, passkey, OAuth, API, and SSE paths continue to use network behavior.
- The existing version update notice can represent both server version drift and Service Worker update availability.

## Visual Evidence

- Update + install state: [update-install.png](./assets/update-install.png)
- Install-only state: [install-only.png](./assets/install-only.png)
- Update-only state: [update-only.png](./assets/update-only.png)
