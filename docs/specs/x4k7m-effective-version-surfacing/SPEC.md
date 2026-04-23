# 发布有效版本显示修复（#x4k7m）

## 背景 / 问题陈述

当前 UI footer 的版本号来自 `/api/health`，后端直接回传 `CARGO_PKG_VERSION`。发布流水线虽然计算了 `APP_EFFECTIVE_VERSION` 并传入 Docker build-arg，但 Rust 构建过程未消费该变量，导致线上部署新版本后仍显示 `v0.1.0`。

## 目标 / 非目标

### Goals

- 版本解析改为：优先 `APP_EFFECTIVE_VERSION`，回退 `CARGO_PKG_VERSION`。
- 新增稳定版本接口 `GET /api/version`，便于排障与前端直连。
- 保持 `GET /api/health` 响应兼容，沿用相同有效版本来源。
- 前端 footer 优先读取 `/api/version`，失败回退 `/api/health`。
- 打通 Docker 构建注入闭环并增加 release 保护校验。
- 前端构建阶段嵌入版本优先消费 `APP_EFFECTIVE_VERSION`，仅在 env 缺失时回退仓库根 `Cargo.toml`，且缺失时不得崩溃。
- 普通 CI 在既有 `Build (Release)` gate 内增加 Docker smoke，提前覆盖 release `web-builder` 构建路径。

### Non-goals

- 不改 PR label release 语义（`type:*` / `channel:*`）。
- 不改数据库 schema。
- 不改 UI 样式布局。

## 范围（Scope）

### In scope

- `src/version.rs`：统一版本来源解析。
- `src/server.rs`：新增 `/api/version`，更新 `/api/health` 版本来源。
- `web/src/layout/AppMetaFooter.tsx`：双端点读取与回退逻辑。
- `web/vite.config.ts` / `web/build/embeddedVersion.ts`：前端构建期版本解析改为 env 优先、Cargo 可选兜底、缺省不崩。
- `Dockerfile`：web / Rust 构建阶段都消费 `APP_EFFECTIVE_VERSION`，且 web-builder 在完整仓库上下文里保留 `Cargo.toml` 兜底来源。
- `.github/workflows/ci.yml`：在既有 `Build (Release)` gate 内加入 Docker smoke 覆盖 release `web-builder` 路径。
- `.github/workflows/release.yml`：增加 `APP_EFFECTIVE_VERSION` 非空保护，并在历史 backfill 时叠加当前 workflow revision 的 Docker/web 构建基础设施修复。
- 后端/前端测试覆盖新增行为与前端构建期 fallback 契约。

### Out of scope

- 调整 release tag 计算脚本。
- 新增独立版本存储层。

## 接口契约（Interfaces & Contracts）

| 接口 | 类型 | 变更 |
| --- | --- | --- |
| `GET /api/version` | HTTP API | Add |
| `GET /api/health` | HTTP API | Modify (版本来源一致化，字段保持兼容) |

### `GET /api/version` 响应

```json
{
  "ok": true,
  "version": "0.10.1",
  "source": "APP_EFFECTIVE_VERSION"
}
```

`source` 仅允许：
- `APP_EFFECTIVE_VERSION`
- `CARGO_PKG_VERSION`

## 验收标准（Acceptance Criteria）

- Given 构建时注入 `APP_EFFECTIVE_VERSION=9.9.9`
  When 访问 `/api/version`
  Then 返回 `version=9.9.9` 且 `source=APP_EFFECTIVE_VERSION`。

- Given 构建时未注入或注入空白 `APP_EFFECTIVE_VERSION`
  When 访问 `/api/version`
  Then 返回 `version=CARGO_PKG_VERSION` 且 `source=CARGO_PKG_VERSION`。

- Given 前端构建时注入 `APP_EFFECTIVE_VERSION=9.9.9`
  When `vite.config.ts` 解析嵌入版本
  Then 稳定使用 `APP_EFFECTIVE_VERSION`，不会为了读取 fallback 而强依赖仓库根 `Cargo.toml`。

- Given 前端构建时未注入 `APP_EFFECTIVE_VERSION` 且仓库根 `Cargo.toml` 存在
  When `vite.config.ts` 解析嵌入版本
  Then 使用 `Cargo.toml` 中的 package version 作为兜底。

- Given 前端构建时既未注入 `APP_EFFECTIVE_VERSION`，仓库根 `Cargo.toml` 也缺失或不可读
  When `vite.config.ts` 解析嵌入版本
  Then 回退 `"unknown"` 且构建成功，不得抛出 `ENOENT`。

- Given CI 运行在 pull request / merge_group / main push
  When 执行 `CI Pipeline` 的 `Build (Release)` gate
  Then 该 gate 必须额外对真实 `Dockerfile` 完成 `linux/amd64` Docker smoke，并传入合成 `APP_EFFECTIVE_VERSION`。

- Given Release workflow 在补跑历史 `release_head_sha`，且当前 workflow revision 包含构建链路修复
  When `docker-release` job checkout 历史目标源码
  Then 必须在构建前叠加当前 workflow revision 的 `Dockerfile`、`web/vite.config.ts` 与 `web/config/embeddedVersion.ts`，避免历史 backfill 复现已修复的构建断链。

- Given `/api/version` 不可用
  When 前端 footer 拉取版本
  Then 自动回退 `/api/health` 并正确显示版本。

- Given `/api/version` 与 `/api/health` 都失败
  When 前端 footer 拉取版本
  Then 显示 `Version unknown`。
