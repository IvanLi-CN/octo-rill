# Web PWA app shell history

## Key Decisions

- PWA support is scoped to installability and app-shell resilience because OctoRill's useful data is authenticated and server-backed.
- Private API caching is excluded to avoid stale data, cross-user leakage, and logout confusion.
- Service Worker updates reuse the existing version update notice so browser-controlled cache updates and server version drift share one refresh affordance.
- 2026-05-10: Added production-dist PWA contract and Playwright coverage for offline app-shell fallback, private path bypass, and user-confirmed waiting Service Worker activation.
