# GitHub Release 二进制资产补齐（#sak35）

## 状态

- Status: 已完成
- Created: 2026-04-04
- Last: 2026-04-04

## 背景 / 问题陈述

- 现有 release workflow 会创建 Git tag、GitHub Release、Docker 镜像和 PR 回写评论，但 GitHub Release 页面只有源码压缩包，没有可直接下载运行的程序。
- OctoRill 运行时不仅依赖后端二进制，还依赖 `web/dist/` 静态资源；如果只上传裸二进制，下载后仍然无法正常提供前端页面。
- 2026-04-04 的发布记录暴露出 rerun/backfill 会继续计算新 patch tag 的风险，同一提交 `95bdf4f` 同时出现了 `v2.4.3` 和 `v2.4.4`。

## 目标 / 非目标

### Goals

- 为每个 GitHub Release 提供固定命名的 Linux 下载包 `octo-rill-linux-x86_64.tar.gz`。
- 明确定义下载包目录结构，确保包内至少包含 `octo-rill`、`web/dist/` 和 `.env.example`。
- 将 release rerun/backfill 改成优先复用现有 tag/release，而不是继续抬高 patch 版本。
- 将 `workflow_dispatch` 明确收敛为“现有 release 资产回填”入口，默认回填最新 published release。
- 保持 `Build (Release)` 作为 PR required check，但把验证对象升级为可分发 bundle。

### Non-goals

- 不扩展 macOS、Windows 或多架构 bundle。
- 不改变 GHCR stable/rc tag 语义。
- 不修改运行时 API、数据库 schema 或产品 UI。
- 不追溯批量修复所有历史 release，只覆盖当前最新缺资产 release 与后续新发布。

## 范围（Scope）

### In scope

- `.github/workflows/release.yml` 的 release metadata 解析、bundle 构建、GitHub Release 上传和 backfill 路径。
- `.github/workflows/ci.yml` 的 `Build (Release)` 校验。
- 新增 release metadata helper、自测脚本和 bundle 构建脚本。
- README 中的 release/download/backfill 文档说明。

### Out of scope

- Docker runtime image 内容变更。
- 额外的安装脚本、systemd/unit 文件、发行版包格式。
- 历史 release 的逐个回填自动化编排。

## 需求（Requirements）

### MUST

- GitHub Release 资产固定上传 `octo-rill-linux-x86_64.tar.gz`。
- bundle 顶层目录固定为 `octo-rill-linux-x86_64/`。
- bundle 内必须包含 `octo-rill`、`web/dist/index.html` 和 `.env.example`。
- `workflow_run` rerun 命中已有 tag 时必须复用该 tag，不得继续计算新 patch。
- `workflow_dispatch` 默认回填最新 published release；显式 `release_tag` 优先级最高。
- backfill 路径不得重新推送 Docker `latest`。
- `Build (Release)` 必须解包并校验 bundle 结构与可执行位。

### SHOULD

- release metadata helper 以脚本自测覆盖“首次 stable 计算”“同提交 rerun 复用 tag”“显式 release_tag backfill”。
- PR 回写评论继续指向最终复用的 tag/release。

### COULD

- 后续可在同一 helper 上扩展多平台 bundle 解析与上传。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 自动发版：
  - `CI Pipeline` 在 `main` 成功后触发 `Release` workflow。
  - `prepare` 解析 PR label intent，并根据目标提交上的现有 tag 决定“复用现有 tag”还是“计算新 tag”。
  - `bundle-release` 构建 `octo-rill-linux-x86_64.tar.gz` 并通过 artifact 传给发布阶段。
  - `docker-release` 仅在自动发版路径运行，继续发布 GHCR 镜像。
  - `publish-release` 在 bundle 和 Docker 发布通过后创建或更新 GitHub Release，并上传 tarball 资产。
- 手动回填：
  - `workflow_dispatch` 提供 `release_tag`、`head_sha` 两个可选输入。
  - `release_tag` 存在时，workflow 直接锁定该 release 并只回填资产。
  - 两个输入都为空时，workflow 通过 GitHub API 取 latest release 并回填资产。
  - backfill 路径不会重新推送 Docker tag。

### Edge cases / errors

- `head_sha` 若未指向任何现有 release tag，manual backfill 直接失败并提示需要显式 tag 或 latest release。
- 若 bundle 构建成功但 Docker 发布失败，GitHub Release 不得提前创建/更新。
- 若 release 已存在，`softprops/action-gh-release` 仅更新资产；release notes 在 backfill 路径保持不变。

## 接口契约（Interfaces & Contracts）

### 接口清单（Inventory）

| 接口（Name） | 类型（Kind） | 范围（Scope） | 变更（Change） | 契约文档（Contract Doc） | 负责人（Owner） | 使用方（Consumers） | 备注（Notes） |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `Release` workflow dispatch inputs | CLI | internal | Modify | ./contracts/cli.md | CI | maintainers | 新增 `release_tag`，并把 dispatch 语义收敛为 backfill |
| `octo-rill-linux-x86_64.tar.gz` | File format | external | New | ./contracts/file-formats.md | CI | release download users | GitHub Release 公开资产 |

### 契约文档（按 Kind 拆分）

- [contracts/README.md](./contracts/README.md)
- [contracts/cli.md](./contracts/cli.md)
- [contracts/file-formats.md](./contracts/file-formats.md)

## 验收标准（Acceptance Criteria）

- Given `type:patch|minor|major` 的 PR 合入 `main`
  When `Release` workflow 完成
  Then GitHub Release 资产中除源码包外还包含 `octo-rill-linux-x86_64.tar.gz`。

- Given `octo-rill-linux-x86_64.tar.gz`
  When 解包
  Then 顶层目录为 `octo-rill-linux-x86_64/`，且包含 `octo-rill`、`web/dist/index.html`、`.env.example`，其中 `octo-rill` 保持可执行。

- Given 同一提交已经存在稳定 tag
  When release workflow rerun
  Then workflow 复用既有 tag/release，并覆盖资产，而不是生成新的 patch tag。

- Given 维护者手动触发 `workflow_dispatch` 且传入 `release_tag=v2.4.4`
  When workflow 完成
  Then 资产被补到现有 `v2.4.4` release，且不会重新推送 Docker `latest`。

## 实现前置条件（Definition of Ready / Preconditions）

- 目标/非目标、范围（in/out）、约束已明确
- 验收标准覆盖自动发版、rerun 复用 tag、manual backfill 三个核心路径
- 接口契约已定稿，发布流程和 README 可直接按契约同步

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Script tests: `.github/scripts/test-release-metadata.sh`, `.github/scripts/test-release-pr-comment.sh`
- Build validation: `.github/workflows/ci.yml` 中的 `Build (Release)` 解包校验

### UI / Storybook (if applicable)

- Not applicable

### Quality checks

- `python3 -m py_compile .github/scripts/release_metadata.py ...`
- `bash .github/scripts/test-release-metadata.sh`
- `bash .github/scripts/test-release-pr-comment.sh`

## 文档更新（Docs to Update）

- `README.md`: release assets、dispatch backfill 入口与 rerun 语义
- `.github/workflows/release.yml`: workflow_dispatch 接口与发布顺序

## 计划资产（Plan assets）

- Directory: `docs/specs/sak35-release-binary-assets/assets/`
- In-plan references: none
- Visual evidence source: not applicable

## Visual Evidence

无。本规格不涉及 owner-facing UI 变更。

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新增 release metadata helper，覆盖自动发版、rerun 复用 tag 与 manual backfill 目标解析。
- [x] M2: 新增 release bundle 构建脚本，并将 `Build (Release)` 升级为 bundle 解包校验。
- [x] M3: 重构 release workflow 发布顺序，上传 GitHub Release 资产并收口 README/backfill 文档。

## 方案概述（Approach, high-level）

- 将“release 目标解析”和“实际构建发布”拆开：`prepare` 只负责输出单一元数据真相源，后续 job 只消费这些输出。
- bundle 构建复用同一脚本，避免 CI 校验和 release workflow 出现两套打包逻辑。
- 手动 dispatch 只做现有 release 资产回填，避免对历史 stable tag 造成 Docker `latest` 漂移。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：若未来需要多平台 bundle，当前单一 `octo-rill-linux-x86_64.tar.gz` 命名需要扩展。
- 需要决策的问题：None
- 假设（需主人确认）：当前只要求 `linux/amd64` bundle，latest release backfill 默认落在 `v2.4.4`。

## 变更记录（Change log）

- 2026-04-04: 新建规格并完成 release bundle、tag 复用与 manual backfill 实现。

## 参考（References）

- `README.md`
- `.github/workflows/release.yml`
- `.github/workflows/ci.yml`
