# ADR 0003: Collection Record Time and Attempt Filtering

## Status

Accepted

## Context

The administration collection-record view must answer two independent questions: when the source business event happened, and how much processing it has received. Historical Release rows can lack a discovery timestamp because that timestamp was not recorded at the time. Filtering those rows by discovery time silently removes valid releases from the source-record view.

Release and announcement records can have more than one applicable processing pipeline. Treating each missing pipeline as an independently matching zero-attempt record makes a partially processed source look unprocessed.

## Decision

- The collection-record list is a source-record view. Release, announcement, and daily-brief sources remain eligible even when they have no processing task or attempt history.
- Time filtering uses source business time: Release publication time, announcement occurrence time, and daily-brief generation time. Discovery time remains an optional audit field and is displayed as `未知` when it was never captured. Historical discovery timestamps are not fabricated or backfilled.
- A record's filterable total attempt count is the maximum total attempt count among its applicable processing pipelines. The total includes the initial execution and retries. A value of zero therefore means no applicable pipeline has begun an attempt.
- The attempt filter exposes an inclusive range from `0` through `10`. Its upper bound may be omitted to mean unlimited. The default is `0` through unlimited and does not exclude any record.
- Filtering occurs before sorting and pagination. The response continues to expose per-pipeline status and retry counts for inspection.

## Consequences

Historical releases are visible in their actual publication window without inventing audit facts. Operators can isolate unprocessed sources, first-pass processing, or repeatedly processed records without conflating retry count, total attempts, and LLM logical calls. The API and UI must validate the range and preserve its meaning across desktop and mobile layouts.
