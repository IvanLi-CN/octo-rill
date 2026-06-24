# 生产 rollout reconcile 与 release 后版本追平验收（#hgyen）

## 背景

- 当前 `Release` workflow 已能在 `main` 合并后稳定创建 GitHub Release 并推送 GHCR 镜像。
- 线上实例 `192.168.31.11` 的 OctoRill stack 使用移动 tag `ghcr.io/ivanli-cn/octo-rill:latest`，但仓库内没有任何自动把生产容器拉到新镜像的机制。
- 2026-06-25 复核线上性能回归时，确认 `v2.37.5` 已发布且镜像已推送，但生产实例仍停在 `v2.37.4`，导致“修复代码已合并但线上症状还在”的误判。
- 仅依赖人工 `docker compose pull && docker compose up -d` 不满足可验证的 production truth；需要一条稳定的 host-side reconcile path，并让 release workflow 对“生产是否追上新版本”给出明确验收结果。

## 目标

- 为 101 上的 OctoRill stack 建立幂等的 host-side rollout reconcile 机制，自动拉取 `:latest` 并在需要时重建容器。
- 让 release automation 在 stable `push@main` 场景下，不仅证明 GitHub Release 与 GHCR 镜像已完成，还要验证生产 `/api/health` 版本最终追平预期版本。
- 在项目文档中明确区分“发版自动化”和“生产 rollout 自动化”的边界，避免后续维护者误以为 GitHub Actions 已直接部署到 101。

## 非目标

- 不把 production deploy 改成 GitHub Actions 直连 SSH 或远程执行 `docker compose`。
- 不新增 owner-facing UI、环境变量、数据库 schema 或产品接口。
- 不为 prerelease / historical backfill 强制要求生产实例立即切到对应版本。

## 需求

### Host-side reconcile

- `192.168.31.11` 上的 `/home/ivan/srv/octo-rill/` 必须提供可重跑的 rollout reconcile 脚本，至少覆盖：
  - `docker compose pull`
  - `docker compose up -d`
  - 公网 `/api/health` 成功校验
- reconcile 脚本必须是幂等的：
  - 镜像未变化时不得无故重建容器
  - 同一时刻只能允许一个 reconcile 运行
- 该机制必须通过 user-level systemd timer 持续运行，而不是一次性手动命令。
- 部署卡片必须把 reconcile 脚本、timer 路径与常用排障命令写清楚。

### Release-side verification

- `Release` workflow 对 stable `push@main` run 必须新增 production rollout verify job。
- verify job 必须轮询 `https://octo-rill.ivanli.cc/api/health`，直到 `version` 等于本次 stable release 的 `APP_EFFECTIVE_VERSION`，或超时失败。
- verify job 失败时，release run 必须失败，从而进入既有 release failure notification 链路。
- prerelease 与显式 historical backfill 不要求生产实例切到该版本；对应 run 必须跳过 production rollout verify。

### Documentation / truth

- `docs/repository-governance.md` 必须明确：
  - `Release` workflow 负责版本决策、GitHub Release 和 GHCR 镜像发布
  - 生产 rollout 由 101 上的 host-side reconcile 接住，不是 repo 内 deploy job
- `docs/README.md` 与 `docs/architecture.md` 必须给出“release 已出但线上版本未更新”时的正确排查入口。

## 验收标准

- Given stable release workflow 已成功推送 `ghcr.io/ivanli-cn/octo-rill:latest`
  When 101 上定时 reconcile 运行
  Then 生产实例会在下一次 reconcile 窗口内切到新镜像，并且 `/api/health.version` 追平发布版本。

- Given 生产实例已经是最新镜像
  When reconcile 再次运行
  Then 容器不会被无故重建，且健康检查仍返回 `200`。

- Given stable `push@main` release workflow 完成 GitHub Release 与 GHCR 镜像推送
  When production 迟迟没有追平到 `APP_EFFECTIVE_VERSION`
  Then `verify-prod-rollout` job 失败，整条 release workflow 失败并触发既有失败通知。

- Given release run 是 prerelease 或 historical backfill
  When release workflow 执行
  Then production rollout verify job 跳过，不以生产版本追平作为该 run 的成功条件。
