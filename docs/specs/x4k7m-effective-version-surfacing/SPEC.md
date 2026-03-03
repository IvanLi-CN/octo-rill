# 发布有效版本显示修复（#x4k7m）

## 状态

- Status: 已完成
- Created: 2026-03-03
- Last: 2026-03-03

## 背景 / 问题陈述

当前 UI footer 的版本号来自 `/api/health`，后端直接回传 `CARGO_PKG_VERSION`。发布流水线虽然计算了 `APP_EFFECTIVE_VERSION` 并传入 Docker build-arg，但 Rust 构建过程未消费该变量，导致线上部署新版本后仍显示 `v0.1.0`。

## 目标 / 非目标

### Goals

- 版本解析改为：优先 `APP_EFFECTIVE_VERSION`，回退 `CARGO_PKG_VERSION`。
- 新增稳定版本接口 `GET /api/version`，便于排障与前端直连。
- 保持 `GET /api/health` 响应兼容，沿用相同有效版本来源。
- 前端 footer 优先读取 `/api/version`，失败回退 `/api/health`。
- 打通 Docker 构建注入闭环并增加 release 保护校验。

### Non-goals

- 不改 PR label release 语义（`type:*` / `channel:*`）。
- 不改数据库 schema。
- 不改 UI 样式布局。

## 范围（Scope）

### In scope

- `src/version.rs`：统一版本来源解析。
- `src/server.rs`：新增 `/api/version`，更新 `/api/health` 版本来源。
- `web/src/layout/AppMetaFooter.tsx`：双端点读取与回退逻辑。
- `Dockerfile`：Rust 构建阶段消费 `APP_EFFECTIVE_VERSION`。
- `.github/workflows/release.yml`：增加 `APP_EFFECTIVE_VERSION` 非空保护。
- 后端/前端测试覆盖新增行为。

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

- Given `/api/version` 不可用
  When 前端 footer 拉取版本
  Then 自动回退 `/api/health` 并正确显示版本。

- Given `/api/version` 与 `/api/health` 都失败
  When 前端 footer 拉取版本
  Then 显示 `Version unknown`。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 后端版本解析模块与 `/api/version` 完成。
- [x] M2: 前端 footer 双端点回退逻辑完成。
- [x] M3: Docker/release 注入链路校验与自动化测试完成。

## 变更记录（Change log）

- 2026-03-03: 建立规格并冻结修复范围与接口决策。
- 2026-03-03: 完成后端 `APP_EFFECTIVE_VERSION` 优先解析、`/api/version` 接口、footer 回退逻辑与 Docker/Release 注入闭环校验。
