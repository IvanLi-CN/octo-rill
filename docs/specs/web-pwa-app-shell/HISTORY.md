# Web PWA app shell history

## Key Decisions

- PWA support is scoped to installability and app-shell resilience because OctoRill's useful data is authenticated and server-backed.
- Private API caching is excluded to avoid stale data, cross-user leakage, and logout confusion.
- Service Worker updates reuse the existing version update notice so browser-controlled cache updates and server version drift share one refresh affordance.
- 2026-05-10: Added production-dist PWA contract and Playwright coverage for offline app-shell fallback, private path bypass, and user-confirmed waiting Service Worker activation.
- 2026-05-11: Expanded install completeness with manifest identity, shortcuts, screenshots, and a native install prompt action in the shared version notice surface.
- 2026-05-12: Added explicit precache allowlist, proactive Service Worker update checks, and regression coverage for `/auth/**` bypass and user-confirmed activation after version-drift update discovery.
- 2026-05-12: Hardened offline anonymous boot and install metadata so a cached app shell surfaces network unavailability explicitly while preserving private API/auth bypass rules.
- 2026-05-13: Split authenticated offline Dashboard into cached-content and no-cache states, and kept warm feed content available across lazy route loading and auth reconciliation.
- 2026-07-08: Added Dashboard React Query persistence as a whitelisted 1 hour server-state cache for PWA responsiveness while keeping Service Worker private API/auth bypass rules unchanged.
- 2026-09-01: Fixed installed-app icon delivery by content-addressing production icon URLs, keeping manifest identity stable, excluding install metadata from Service Worker precache, and documenting Chromium versus Apple Web Clip update limits.
