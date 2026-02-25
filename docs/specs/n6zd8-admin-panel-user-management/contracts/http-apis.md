# HTTP API contracts

## Modify: `GET /api/me`

### Response

```json
{
  "user": {
    "id": 1,
    "github_user_id": 123,
    "login": "octo",
    "name": "Octo",
    "avatar_url": null,
    "email": null,
    "is_admin": true
  }
}
```

## New: `GET /api/admin/users`

### Query params

- `query` (optional): fuzzy search in `login/name/email`.
- `role` (optional): `all | admin | user` (default `all`).
- `status` (optional): `all | enabled | disabled` (default `all`).
- `page` (optional): default `1`, min `1`.
- `page_size` (optional): default `20`, range `1..100`.

### Response

```json
{
  "items": [
    {
      "id": 1,
      "github_user_id": 123,
      "login": "octo",
      "name": "Octo",
      "avatar_url": null,
      "email": null,
      "is_admin": true,
      "is_disabled": false,
      "created_at": "2026-02-25T01:00:00Z",
      "updated_at": "2026-02-25T01:00:00Z"
    }
  ],
  "page": 1,
  "page_size": 20,
  "total": 1
}
```

## New: `PATCH /api/admin/users/{user_id}`

### Request body

```json
{
  "is_admin": true,
  "is_disabled": false
}
```

At least one field must be provided.

### Response

Returns the updated user item with the same schema as list items.

## Error codes

- `forbidden_admin_only` (`403`): caller is not admin.
- `account_disabled` (`403`): caller is disabled.
- `last_admin_guard` (`409`): operation would remove last admin or last active admin.
- `cannot_disable_self` (`409`): admin tries to disable self.
- `not_found` (`404`): target user does not exist.
