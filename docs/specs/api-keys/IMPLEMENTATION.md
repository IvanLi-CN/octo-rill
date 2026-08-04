# API Key 用户接口调用实现状态

> 当前有效规范仍以 `./SPEC.md` 为准；这里记录实现覆盖、交付进度与 rollout 相关事实，避免这些细节散落到 PR / Git 历史里。

## Current Status

- Implementation: implemented
- Lifecycle: active
- Catalog note: fast-track / API Key user business API access

## Coverage / rollout summary

- 新增 `user_api_keys` 迁移，Key 明文使用 `orill_ak_` 前缀；认证保存 SHA-256 hash，设置页回显保存 AES-256-GCM 加密密文。
- 新增 session-only `/api/me/api-keys` 管理接口；列表与创建响应返回完整 Key，撤销使用 `revoked_at` 软删除并立即失效。
- 新增 Bearer API Key 业务认证 helper，允许用户态业务接口复用当前 user id，并继续拒绝禁用账号。
- 设置页新增 `api-keys` section，覆盖空态、创建成功复制态、完整 Key 列表、撤销二次确认与错误态。
- Storybook、Playwright settings E2E 与 Rust API Key tests 已覆盖核心行为。

## Remaining Gaps

- None for this delivery scope.

## Related Changes

- `migrations/0055_user_api_keys.sql`
- `src/api_keys.rs`
- `src/api.rs`
- `src/server.rs`
- `src/translations.rs`
- `web/src/pages/Settings.tsx`
- `web/src/stories/Settings.stories.tsx`
- `web/e2e/settings.spec.ts`

## References

- `./SPEC.md`
- `./HISTORY.md`
