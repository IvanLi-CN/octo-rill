# Release 失败 Telegram 告警接入

## 状态

- Spec ID: `fvh8d`
- State: `active`
- Status: `已完成`
- Scope: 为 `Release` workflow 接入失败告警与 repo-local smoke test

## 目标

为 `octo-rill` 接入共享的 Telegram 失败告警工作流，使真实 `Release` 失败能够通过 `workflow_run` 自动告警，同时保留一个安全的 repo-local `workflow_dispatch` smoke test 入口。

## 范围

### In scope

- 新增 `.github/workflows/notify-release-failure.yml`
- 监听 `Release` workflow 的失败结果并转发到 `IvanLi-CN/github-workflows`
- 复用单个 repo secret：`SHOUTRRR_URL`
- 提供 repo-local `workflow_dispatch` smoke test

### Out of scope

- 不修改现有 `Release` 发布逻辑
- 不新增第二通知渠道
- 不调整 release 版本计算、tag、GitHub Release 或镜像发布规则

## 需求

### Must

- 当 `main` 上的 `Release` workflow 以 `failure` 结束时，必须触发 notifier workflow
- notifier 必须显式调用 `IvanLi-CN/github-workflows/.github/workflows/release-failure-telegram.yml@main`
- notifier 必须显式传入 `secrets.SHOUTRRR_URL`
- notifier 必须保留 `workflow_dispatch` 入口，用于安全 smoke test
- notifier 不能对非 `main` 分支的失败 run 发送 production-style 告警

## 验收标准

- Given `SHOUTRRR_URL` 已配置
  When 手动运行 `notify-release-failure.yml`
  Then Telegram 收到 smoke-test 消息
- Given `Release` 在 `main` 上失败
  When `notify-release-failure.yml` 接收 `workflow_run`
  Then Telegram 收到失败告警
- Given 某个非 `main` 分支手动触发 `Release`
  When 该 run 失败
  Then 不发送 production-style 失败告警
- Given `Release` 成功
  When notifier workflow 评估事件
  Then 不发送失败告警

## 变更记录

- 2026-04-11: 为 `octo-rill` 接入共享 Telegram 发布失败告警与 repo-local smoke test。
