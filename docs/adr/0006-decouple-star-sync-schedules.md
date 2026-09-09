# ADR 0006: Decouple Star Synchronization from Subscription Governance

## Status

Accepted

## Context

`sync.subscriptions` previously refreshed Star, then used the same administrative interval to drive Release governance. Its recurring Star fetch was deliberately shallow: it stopped at the existing watermark or fifty newest items and only upserted rows. That made ordinary refresh inexpensive, but an unstar that had fallen below the shallow window could never be removed until an ad-hoc full replacement.

Star membership changes slowly enough to reconcile in a paced background scan. GitHub GraphQL exposes a connection `totalCount`, `pageInfo`, and forward cursor pagination. It does not expose stable random page offsets, so a full scan cannot be pre-split into independent parallel cursor ranges.

## Decision

- Supersede ADR 0005. Its Release-governance decisions remain in force and are restated here: `sync_auto_fetch_interval_minutes` is the Release subscription/governance window, saves take effect at the next strictly later UTC-aligned boundary, active governance cycles freeze their actual `N+B`, and administrative retry reuses only the original watcher scope.
- Remove Star fetching from `sync.subscriptions`. Subscription runs consume the currently materialized visible repository set for Release, social, and Inbox work.
- Add a single in-process Star Sync Coordinator. It owns Star delta scheduling, full-reconciliation scheduling, connection leases, epoch cursor state, per-connection membership writes, and recomputation of the existing aggregate Star association consumed by read models.
- Expose two independent global admin settings:
  - `star_sync_delta_interval_minutes`, default `30`, range `1-120`.
  - `star_sync_full_sweep_interval_minutes`, default `1440`, range `60-10080`.
- Delta jobs fetch the newest Star page and upsert observations. They do not remove membership.
- A full reconciliation epoch belongs to exactly one `(user_id, github_connection_id)`. It fetches one GraphQL page per scheduled slice, persists `endCursor`, and schedules the next slice from that cursor. `totalCount` is recorded for progress and spacing estimates only; `hasNextPage=false` is the completion proof.
- Start a full epoch from the first page when no active epoch exists and the connection is due. All slices use an exclusive, renewable connection lease. Scheduler overlap records a skip instead of creating a second epoch.
- On successful terminal completion only, remove memberships not seen in the epoch, preserving observations made by a delta job after the epoch started. Then recompute the user-level aggregate Star association for each affected repository. A GraphQL `isOverLimit=true`, incomplete/failed page, expired lease, or cancelled epoch must never prune membership.
- `sync.access_refresh` and an explicit user sync execute a Star delta only; they do not inline a full scan or use the Release-governance clock. A full sweep stays paced even when initiated interactively.

## Consequences

The application gains eventual reconciliation of unstars without a bursty all-page request. Full-sweep completion is targeted for the configured interval by spacing its observed pages evenly, subject to ordinary queue recovery and the scheduler's minimum spacing. The source snapshot is not atomic: GitHub may change the connection during a sweep. Epoch start time plus last-seen facts prevents this from deleting a later delta observation, and the next epoch converges any remaining movement.

The user-facing setting formerly described as a subscription interval now exclusively describes Release subscription/governance. Star settings are separate and report each active epoch's page progress, estimated count, and last successful completion.
