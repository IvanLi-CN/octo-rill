# HTTP API contract changes for #67n8t

## Scope

This document covers only API fields and path/query/body parameters that carry local application identifiers.

## Required changes

- Any response field representing a local application primary key MUST become a `string` carrying a 16-character NanoID.
- Any request path/body/query field representing a local application primary key MUST accept a `string` NanoID and reject malformed IDs.
- Public routes that use GitHub-origin identifiers (`release_id`, `thread_id`, `repo_id`, `github_user_id`) MUST keep their current semantics and value shapes.

## Affected contracts

- `GET /api/me`
  - `user.id`: `number` -> `string`
- Admin user routes
  - `/api/admin/users/{user_id}` path param: local user id becomes NanoID string
  - `/api/admin/users/{user_id}/profile` path param: local user id becomes NanoID string
  - any embedded local user id in admin responses becomes string
- Admin task/translation routes
  - local task/request/work/batch ids remain string but must conform to the shared NanoID format
- Non-admin routes
  - `release_id`, `thread_id`, and other external resource ids remain unchanged

## Validation

- Malformed local IDs return a client error (`400` or equivalent existing bad-request contract).
- Not-found responses keep their current semantics after ID type changes.
