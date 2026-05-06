# 实现状态

- Summary: 本地验证完成；fast-track / public release pages + REST API + admin registry

## Milestones

- [x] M1: 新增公开 usage schema 与同步聚合入口。
- [x] M2: 新增公开列表/详情 API 与 pending retry 语义。
- [x] M3: 新增公开列表/详情页面与管理后台登记列表。
- [x] M4: 完成自动化验证与视觉证据。
- [x] M5: 完成 review-loop。
- [ ] M6: 完成 PR 收敛。

## Current Notes

- 管理后台删除公开登记记录后，若该仓库不再被其他公开登记、登录用户 release 可见性或历史 brief membership 使用，会清理对应共享 `repo_releases`、release AI 缓存与 release sync state；仍被使用时保留缓存。
- 公开列表页默认展示原文正文，但列表态会截断超长正文，详情页仍展示完整正文。

## Verification

- `codex -m gpt-5.5 -c model_reasoning_effort="medium" --sandbox read-only -a never review --base origin/main`
- `cd web && bun run build`
- `cargo test public_release --all-targets`
- `cargo test --all-targets`
- `cd web && bun run storybook:build`
- `cd web && PLAYWRIGHT_WEB_PORT=36830 bun run e2e -- public-release-page.spec.ts`
