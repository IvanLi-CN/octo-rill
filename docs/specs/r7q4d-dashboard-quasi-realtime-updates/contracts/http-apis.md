# HTTP API Contract

## `GET /api/dashboard/updates`

Authenticated Dashboard-only update check.

Query:

- `token?: string`
- `feed_type?: all | releases | stars | followers`
- `include?: feed,briefs,notifications`

Response:

```json
{
  "token": "opaque-next-token",
  "generated_at": "2026-04-30T10:00:00Z",
  "lists": {
    "feed": { "changed": true, "new_count": 1, "latest_keys": ["release:121"] },
    "briefs": { "changed": false, "new_count": 0, "latest_keys": [] },
    "notifications": { "changed": false, "new_count": 0, "latest_keys": [] }
  }
}
```

Behavior:

- Missing token establishes a baseline and returns `changed=false`.
- Invalid token returns `400 bad_request`.
- The endpoint only reads local database state.
- Omitted `include` lists are omitted from `lists` and preserved inside the next token when possible.
- The opaque token stores compact list signature digests, not public keys, so existing items can report changed when translation, polish, brief, or notification state changes without sending full signatures in the URL.
- `latest_keys` remains stable for client-side fresh marking: `release:<id>`, `repo_star_received:<id>`, `follower_received:<id>`, `brief:<id>`, `notification:<thread_id>`.
