# 修复 Release 自动发版断链并补齐漏发版本（#at76w）

## 背景

- 当前 `/Users/ivan/.codex/worktrees/607d/octo-rill/.github/workflows/release.yml` 依赖 `workflow_run(CI Pipeline completed)`，只有 `push@main` 的 CI 结论为 `success` 才继续发版。
- 2026-04-11 已确认两次已合并的稳定版 PR 因对应 `push@main` CI 被 `cancelled`，导致 Release workflow 被触发但直接 `skipped`：
  - PR #63 / `28c3ff8f919d881f8e4bdc63c9bb9aae1543cfe9`
  - PR #62 / `991bee71b861c7b1be0038fc0909928186c369e2`
- 现状缺少自动 audit/backfill：一旦主干 CI 被取消，后续没有机制补发 tag / GitHub Release / Docker image / PR release comment。
- GitHub Actions 默认 `GITHUB_TOKEN` 无法为历史提交创建缺失的 release tag；当 backfill target 不是当前 `main` HEAD 且目标 tag 尚不存在时，workflow 需要仓库 secret `RELEASE_TOKEN`（具备创建历史 release tag 的 GitHub token），否则只能先手动把目标 tag 推到远端再重跑 release。

## 目标

- 将 Release 触发从 `workflow_run(CI success)` 改为 `push@main` 直接驱动，并保留 `workflow_dispatch(head_sha)` 作为显式补发入口。
- 在主干存在更早的漏发版本时，优先按合并顺序补发最早的漏项，避免版本号跳号或倒序。
- 为已打 tag 但缺少 GitHub Release / PR release comment 的提交提供可重跑、幂等的 repair 路径。
- 修复 PR 自身必须保持 `type:skip + channel:stable`，不能抢占产品版本号。

## 非目标

- 不改 `type:patch|minor|major` 与 `channel:*` 的既有语义。
- 不回补 `type:docs` / `type:skip` PR 的版本发布。
- 不修改产品运行时 HTTP API、数据库 schema、前端/后端业务逻辑。
- 不深挖是谁取消了 `CI Pipeline`；本轮只移除这条脆弱依赖。

## 关键约束

- 主干 push 触发的 release 需要先扫描当前 `main` 的 first-parent merge 历史，再决定本次真正要发布的目标 SHA。
- 若存在更早的未发布 release-eligible merge commit，本次 push 不能先发布较新的 merge commit。
- 无论本次要发布的是当前 push 还是更早的 backfill 目标，都必须校验该 target SHA 自己的 `CI Pipeline` push run 已进入终态；`success` 与 `cancelled` 允许继续发布，`failure`/`timed_out` 必须阻断发布。
- 当某个历史 target SHA 的 `CI Pipeline` 结论为阻断态时，本次 run 不能发布该 SHA，但仍要继续评估并 dispatch 更靠后的 pending release，避免整个 backfill 队列永久卡死。
- 对同一 target SHA 重跑 release 时，版本号必须复用该提交上已存在的 release tag，而不是继续向后 bump。
- 当 target SHA 不是当前 `main` HEAD 且对应 release tag 尚不存在时，workflow 必须在创建 release 前显式校验 `RELEASE_TOKEN` 是否可用；若不可用，需给出可执行的 fallback（预先推送目标 tag 后再重跑）。
- 当目标 tag 已经存在时，workflow 必须复用现有 tag 创建/更新 GitHub Release，而不是继续传入会触发历史 tag 创建的参数。
- 当前确定的补发顺序固定为：
  1. `28c3ff8f919d881f8e4bdc63c9bb9aae1543cfe9`（PR #63）
  2. `991bee71b861c7b1be0038fc0909928186c369e2`（PR #62）

## 实现要求

- Release workflow:
  - 改为监听 `push.branches=[main]` 与 `workflow_dispatch(head_sha)`。
  - 新增 release target planning / audit 步骤，按 first-parent merge 顺序选择最早的漏发提交。
  - 对 push 触发完成当前补发后，如仍存在后续漏项，自动 dispatch 下一次 backfill。
  - 对历史 backfill target 的缺失 tag，优先使用仓库 secret `RELEASE_TOKEN` 创建 release；若 secret 缺失，则在 workflow 内明确阻断并提示先推送 tag 再重跑。
- Release helper scripts:
  - `/Users/ivan/.codex/worktrees/607d/octo-rill/.github/scripts/release-intent.sh` 需要接受 `RELEASE_HEAD_SHA` 作为 push/backfill 的统一输入。
  - `/Users/ivan/.codex/worktrees/607d/octo-rill/.github/scripts/compute-version.sh` 需要在目标提交已有 release tag 时复用既有版本/tag，保证 rerun/backfill 幂等。
  - 新增 audit/backfill helper，负责扫描主干 merge 历史、识别漏发与 repair-only 提交，并为 workflow 输出下一次 backfill 目标。
- CI:
  - 新增 release automation 自测，覆盖：
    - `RELEASE_HEAD_SHA` push 场景；
    - `compute-version` 复用已有 stable / rc tag；
    - audit 选择顺序与 `workflow_dispatch` 显式 SHA 行为；
    - `release.yml` 从 `workflow_run` 切到 `push` 的接线合同。

## 验收

- Given `main` 上一个 release-eligible merge commit 的 `CI Pipeline` 被取消
  When 该 merge 已进入 `main`
  Then Release workflow 仍会在 `push@main` 或 audit/backfill 中补齐该版本。

- Given 主干同时存在多个漏发 release-eligible merge commit
  When Release workflow 选择本次发布目标
  Then 必须先选择更早的漏项，再处理更新的 merge commit。

- Given 目标提交已经存在 release tag
  When rerun 或 backfill 同一个 SHA
  Then workflow 复用既有 `APP_EFFECTIVE_VERSION` / `APP_RELEASE_TAG`，不会生成新的版本号。

- Given 当前漏发项仅为 PR #63 与 PR #62
  When 修复 PR 合并后触发 backfill
  Then 稳定版发布顺序保持为 `v2.11.0` 后 `v2.12.0`，且对应 tag / GitHub Release / PR release comment / Docker image 全部齐全。
