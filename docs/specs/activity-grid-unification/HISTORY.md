# 统一活动图组件与探索交互历史

## Lifecycle

This topic centralizes the shared presentation and interaction contract for the existing collection-record and LLM activity views. Their current business read models remain authoritative until both are adapted to `ActivityGrid`.

## Compatibility

- Existing activity APIs, status aggregation, time boundaries and detail payloads remain unchanged.
- Existing collection-record routes, filters and pagination remain valid while their detail navigation gains request isolation.
- LLM retains its matrix layout, responsive density and panel scrolling; collection records retain their hourly layout while moving to page scrolling.

## Related Changes

None.
