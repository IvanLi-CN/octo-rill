# ADR 0016: Demo Inspector Admin Jobs Surface State Isolation

Status: accepted

Demo Scene remains a page-level mock route preset. Admin Jobs instead exposes two independent surfaces, content processing and LLM scheduling, each with URL-shareable data case and network profile state. This avoids treating distinct endpoint families and domain concepts as one data set while preserving reproducible, surface-specific browser evidence.

## Decision

- Use `d_content_case` / `d_content_net` for content processing and `d_llm_case` / `d_llm_net` for LLM scheduling. A missing case means `loaded`; a missing profile means `normal`.
- Derive the current surface from `/admin/jobs/ai-records` or `/admin/jobs/llm`; the Inspector navigates between those routes and does not introduce a competing `d_surface` query key.
- Data cases are `loaded`, `empty`, `many`, and held `loading`; network profiles are `normal`, `slow`, and `faulty`. `faulty` returns the surface error state, `slow` delays a resolvable fixture, and `loading` remains pending until it is replaced or cancelled.
- A case/profile change aborts the old surface request and advances its request epoch. Caches and handoffs include the full surface identity; only normal resolved fixtures may use them.

## Considered Options

- Use one Admin Jobs data case: rejected because content-processing records and LLM logical calls have distinct endpoint families, empty semantics, and detail routes.
- Make Scene select an Admin Jobs surface: rejected because Scene is a page-level route preset, while both surfaces are independently readable within Admin Jobs.
- Reuse global `d_net` for the two surfaces: rejected because it affects unrelated API requests and cannot provide isolated evidence.

## Consequences

- Demo transport must provide coherent fixtures for every endpoint in each surface family, including content-processing activity reads.
- URL replay, request cancellation, cache isolation, and visual evidence are part of the Admin Jobs Demo contract rather than incidental test setup.
