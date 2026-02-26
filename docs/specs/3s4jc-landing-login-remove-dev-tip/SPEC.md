# Landing 登录页移除开发提示（#3s4jc）

## 状态

- Status: 已完成
- Created: 2026-02-26
- Last: 2026-02-26

## 背景 / 问题陈述

- 登录页在真实用户可见界面中暴露了开发环境提示（Vite proxy 到 Rust 后端）。
- 该提示属于实现细节，容易造成用户困惑，也不符合产品化界面的信息边界。
- 需要在不影响 OAuth 登录流程与错误反馈的前提下，彻底移除该提示并补齐回归校验。

## 目标 / 非目标

### Goals

- 移除 Landing 登录卡片中的 dev/proxy 技术提示。
- 保持“使用 GitHub 登录”入口与 `bootError` 展示逻辑不回归。
- 增加自动化回归，确保该提示不会再次出现在登录页。

### Non-goals

- 不修改 OAuth 授权链路与后端认证逻辑。
- 不新增替代文案或登录引导文本。
- 不做线上缓存/部署可见性复验流程。

## 范围（Scope）

### In scope

- `web/src/pages/Landing.tsx`
- `web/e2e/landing-login.spec.ts`
- `docs/specs/README.md`

### Out of scope

- 后端 API 与数据模型
- 生产环境发布策略与 CDN 缓存治理

## 需求（Requirements）

### MUST

- 登录页不可出现 `Tip: 在 dev 环境` 或同义技术提示。
- 登录按钮继续可见且 `href=/auth/github/login` 不变。
- 新增 e2e 场景覆盖“未登录落地页无技术提示”。

### SHOULD

- 回归覆盖至少一个既有关键 e2e 场景，确保改动不影响主流程。

### COULD

- 无。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 未登录访问 `/` 时，展示登录卡片（标题、描述、登录按钮、可选错误信息），不展示任何开发环境提示。

### Edge cases / errors

- 当 `bootError` 存在时，错误提示仍需可见；并且不应被新增/删除逻辑影响。

## 接口契约（Interfaces & Contracts）

None

## 验收标准（Acceptance Criteria）

- Given 未登录用户访问 `/`
  When Landing 页面渲染完成
  Then 页面中不包含 `Tip: 在 dev 环境` 文本。

- Given 未登录用户访问 `/`
  When 页面加载完成
  Then “使用 GitHub 登录”按钮可见，且链接为 `/auth/github/login`。

- Given `bootError` 有值
  When Landing 渲染
  Then 错误信息正常显示，不受提示移除影响。

## 实现前置条件（Definition of Ready / Preconditions）

- [x] 范围已冻结（仅前端登录页 + e2e）。
- [x] 验收标准已覆盖核心路径与错误展示边界。
- [x] 接口契约确认为 `None`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- E2E tests: 新增 `landing-login.spec.ts` 并执行。
- E2E regression: 复跑 `release-detail.spec.ts`。

### Quality checks

- `cd web && bun run lint`
- `cd web && bun run build`

## 文档更新（Docs to Update）

- `docs/specs/README.md`: 新增条目并跟踪状态。

## 计划资产（Plan assets）

- Directory: `docs/specs/3s4jc-landing-login-remove-dev-tip/assets/`
- In-plan references: None

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 新建规格并写入 `docs/specs/README.md` 索引。
- [x] M2: 移除 Landing 技术提示并新增 e2e 回归。
- [x] M3: 完成快车道交付（PR、checks、review-loop 收敛）并回写规格状态。

## 方案概述（Approach, high-level）

- 前端最小改动：删除 Landing 中单个提示段落，不影响其他 UI 结构。
- 通过 Playwright e2e 建立“未登录页不出现技术提示”的可执行验收。
- 按快车道流程推进 PR、CI 与 review 收敛，最后同步 spec 状态。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：过度删除导致登录卡片信息层级异常（通过 e2e + 现有回归兜底）。
- 开放问题：无。
- 假设：未登录访问 `/api/me` 返回 401 的行为保持不变。

## 变更记录（Change log）

- 2026-02-26: 创建规格，冻结“全部移除技术提示 + 回归保障”的执行口径。
- 2026-02-26: 完成 Landing 提示移除与 `landing-login` e2e 回归，状态更新为 `部分完成（2/3）`。
- 2026-02-26: 创建 PR #15，CI 与标签门禁通过，review-loop 无阻塞项，状态更新为 `已完成`。
- 2026-02-26: 根据 review-loop 建议补充 `bootError` 分支 e2e，保持规格口径不变。

## 参考（References）

- `web/src/pages/Landing.tsx`
- `web/e2e/release-detail.spec.ts`
