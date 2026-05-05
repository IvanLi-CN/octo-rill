# Web PWA app shell history

## Key Decisions

- PWA support is scoped to installability and app-shell resilience because OctoRill's useful data is authenticated and server-backed.
- Private API caching is excluded to avoid stale data, cross-user leakage, and logout confusion.
- Service Worker updates reuse the existing version update notice so browser-controlled cache updates and server version drift share one refresh affordance.
