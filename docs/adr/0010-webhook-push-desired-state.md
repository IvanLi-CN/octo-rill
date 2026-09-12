# Webhook Push Desired State

Webhook Push stores the user's intended lifecycle separately from the observed state of each GitHub Hook. The durable target is `enabled`, `paused`, or `deleted`; local delivery is disabled before a pause or delete operation reaches GitHub, and background work retries remote reconciliation. This preserves user intent across transient failures and avoids treating a missing observation row as an abnormal state.

Remote mutations are restricted to Hooks that match the stored Hook ID, callback URL, and `release` event. A user has at most one active management or audit operation, so a later manual request cannot race an earlier target-alignment attempt.
