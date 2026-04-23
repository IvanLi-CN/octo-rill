# Release 成功后回写 PR 版本评论（#qvewp）

## 背景

- 当前 release 流水线会解析 PR、计算 `APP_EFFECTIVE_VERSION` / `APP_RELEASE_TAG`、创建 Git tag、创建 GitHub Release、推送 Docker 镜像。
- 发版成功后，PR 页面缺少“这次发布实际产出的版本号”这一条稳定回写，回看合并记录时需要手动在 Releases 页面反查。
- 现有 `prepare` job 已经稳定产出 `pr_number`、`app_effective_version` 与 `app_release_tag`，适合作为单一真相源复用，不应再额外重新解析 commit 对应 PR。

## 目标

- 仅在 release 全链路成功后，为对应 PR 回写一条版本评论。
- 评论内容固定展示 `Version`、`Tag` 与 `Release` 链接。
- 同一 PR 对本功能只保留一条 bot 评论；rerun / backfill 走更新，不刷屏。
- 评论逻辑从 workflow YAML 中抽离到独立 helper，便于本地测试与后续维护。

## 非目标

- 不修改 release label 语义与版本计算规则。
- 不为 `type:docs` / `type:skip` PR 追加“未发布”评论。
- 不修改应用运行时 API、数据库、Docker 产物或 UI。
- 不清理历史人工评论或其他不带本功能 marker 的 bot 评论。

## 关键约束

- 评论 marker 固定为 `<!-- octo-rill:release-version -->`。
- 仅更新带有该 marker 且评论作者为 `github-actions[bot]` 的评论。
- 评论正文固定三项：
  - `Version`: `APP_EFFECTIVE_VERSION`
  - `Tag`: `APP_RELEASE_TAG`
  - `Release`: `${GITHUB_SERVER_URL}/${GITHUB_REPOSITORY}/releases/tag/${APP_RELEASE_TAG}`
- 评论 job 必须依赖 `prepare` 与 `docker-release` 成功，确保“发版成功后”才回写。
- 新 job 只授予评论所需的最小权限：`issues: write` 与 `pull-requests: write`，不放大全局 workflow 权限。

## 实现要求

- Release workflow:
  - 新增 `pr-release-comment` job。
  - `needs` 必须包含 `prepare` 与 `docker-release`。
  - `if` 必须要求 `should_release=true` 且 `pr_number` 非空。
  - 使用本仓库脚本 `.github/scripts/release_pr_comment.py` 执行评论 upsert。
- Helper script:
  - 支持 create / update / skip 三态。
  - 通过 GitHub Issues Comments API 查询 PR comments，并只更新最新一条受控评论。
  - 通过 `GITHUB_OUTPUT` 输出 `comment_action`、`comment_url`、`release_url` 供 workflow summary 使用。
- CI:
  - 新增脚本级自测，覆盖 create、update、skip、stable、rc 与 workflow 接线校验。
  - `Lint & Checks` 必须编译校验该脚本，并执行新增自测。

## 验收

- Given `should_release=true` 且 release workflow 成功完成
  When `pr_number` 存在
  Then 对应 PR 出现或更新一条带 marker 的 bot 评论。

- Given 同一 PR 已存在本功能评论
  When release workflow rerun 或 workflow_dispatch backfill
  Then 原评论被更新，不新增第二条本功能评论。

- Given PR 为 stable release
  When 评论写入
  Then `Version` 为纯 semver，`Tag` 为 `vX.Y.Z`，`Release` 指向对应 release 页面。

- Given PR 为 rc release
  When 评论写入
  Then `Version` 仍为基础 semver，`Tag` 包含 `-rc.<sha7>` 后缀，`Release` 链接指向 rc tag 页面。

- Given `should_release=false` 或 `pr_number` 为空
  When helper 执行
  Then 不发起评论写入请求，并返回 `skip`。
