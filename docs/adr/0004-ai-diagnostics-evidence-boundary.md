# ADR 0004: AI Diagnostics Evidence Boundary

## Status

Accepted

## Context

An LLM provider can return content successfully while the content fails the consuming operation's schema or semantic contract. Treating both outcomes as one status made a failed content-processing attempt appear as a generic translation failure, and batch-level association could point an operator at an unrelated successful call. Long-lived attempt audit also has a different security and retention requirement from the short-lived prompt and response needed to diagnose a recent failure.

## Decision

- Release polishing has one execution owner: producers submit scheduler work and do not directly invoke the release-polish LLM path or write terminal polish cache entries.
- An LLM logical call records the provider exchange. Its provider result is independent from the response contract result and the content-processing attempt result. A received response can therefore be a successful provider call and an invalid output contract at the same time.
- Content-processing attempts record a stable failure stage, error code, safe summary, and retry disposition. `failure_class` remains limited to upstream recovery classes and is not used as a catch-all explanation for schema, parser, or semantic failures.
- Every model call used by a content-processing attempt is linked at call time with the work item, attempt number, processing stage, ordinal, and relation role. Batch membership is not evidence that a call produced every item in that batch.
- Prompt, normalized messages, model response, and provider delivery metadata are administrator-only diagnostic payloads. They remain available for seven days, are redacted for sensitive patterns, and their reveal or copy access is auditable. The durable attempt audit stores only safe metadata and keeps an expired-call reference so operators can distinguish expired evidence from missing attribution.
- A response-contract failure receives at most one bounded in-attempt recovery: a length-limited response is regenerated with an adequate output budget; another invalid structured response receives one schema-repair attempt. Further failure follows the scheduler's normal recovery policy. Technical failures are not terminal content-cache hits.

## Consequences

Operators can distinguish provider success, output-contract failure, and business failure without exposing raw diagnostic payload through collection-record APIs. The scheduler becomes the only release-polish writer, and precise attempt-call links continue to explain the causal chain after diagnostic payload retention expires.
