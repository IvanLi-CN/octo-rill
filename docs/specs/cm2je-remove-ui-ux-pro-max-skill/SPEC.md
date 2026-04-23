# 移除项目内 UI UX Pro Max skill（#cm2je）

## 背景 / 问题陈述

项目曾把 `ui-ux-pro-max` 作为仓库内 skill 资产直接提交，并为此保留了一份安装规格。当前主人明确要求从项目中删除该 skill；如果只删目录而不同步 canonical spec 与索引，仓库会留下失效的文档引用与错误的安装承诺。

## 目标 / 非目标

### Goals

- 从仓库中删除 `.codex/skills/ui-ux-pro-max/**` 全部资产。
- 删除仅服务于该安装动作的旧规格 `docs/specs/ejdn8-uipro-skill-install/SPEC.md`。
- 更新 `docs/specs/README.md`，移除失效索引并登记本次 removal spec。
- 保持除本 removal spec 与 specs index 外，仓库内不再出现 `ui-ux-pro-max` 的残留引用。

### Non-goals

- 不调整 `.codex/skills/**` 的通用 Git 跟踪规则。
- 不引入新的 UI/UX skill 替代品。
- 不改动产品功能、前后端逻辑或运行时配置。

## 范围（Scope）

### In scope

- `/Users/ivan/.codex/worktrees/351e/octo-rill/.codex/skills/ui-ux-pro-max/**`
- `/Users/ivan/.codex/worktrees/351e/octo-rill/docs/specs/ejdn8-uipro-skill-install/SPEC.md`
- `/Users/ivan/.codex/worktrees/351e/octo-rill/docs/specs/README.md`
- 本规格文档

### Out of scope

- 其他 `.codex/skills/*` 目录
- `.gitignore` 现有技能跟踪策略
- 任何业务代码、测试、数据库、CI 工作流

## 需求（Requirements）

### MUST

- 仓库中不得再保留 `.codex/skills/ui-ux-pro-max/**` 文件。
- `docs/specs/README.md` 不得继续引用已删除的 `#ejdn8` 安装规格。
- 除本 removal spec 与 specs index 外，仓库级文本搜索不再命中 `ui-ux-pro-max` 相关引用。

### SHOULD

- 删除动作与文档同步应以原子提交形式记录，便于回滚与审计。

## 功能与行为规格（Functional/Behavior Spec）

- 删除 skill 时，必须同时移除其 `SKILL.md`、`data/**` 与 `scripts/**`。
- 删除旧安装规格时，不保留悬空 index 项。
- 新的 removal spec 作为当前 canonical 记录，说明本次删除的背景、范围与验收条件。

## 验收标准（Acceptance Criteria）

- Given 仓库完成本次变更
  When 检查 `.codex/skills/ui-ux-pro-max/`
  Then 目录不存在。

- Given 仓库完成本次变更
  When 检查 `docs/specs/README.md`
  Then 不再存在 `ejdn8` 的索引行，且存在 `cm2je` 的索引行。

- Given 仓库完成本次变更
  When 执行与仓库根目录相关的文本搜索
  Then 除本 removal spec 与 specs index 外，不再命中 `ui-ux-pro-max` 相关引用。

## 实现前置条件（Definition of Ready / Preconditions）

- [x] 主人已明确要求删除项目内 `ui-ux-pro-max`。
- [x] 待删除目录与旧安装规格路径已定位。
- [x] 当前任务未命中相关 solution / legacy knowledge，`solution_disposition=none`。

## 非功能性验收 / 质量门槛（Quality Gates）

- `git status --short`
- `rg -n --hidden --glob '!.git' "ui-ux-pro-max|/\\.codex/skills/ui-ux-pro-max|\\$ui-ux-pro-max" .`
- `cargo test`
- `cd web && biome check .`

## Visual Evidence

- 不适用：本任务不涉及用户可见 UI 变更。

## 风险 / 假设

- 风险：若后续仍有自动化流程隐式依赖该 skill，本次删除会让这些流程失效；当前搜索未发现仓库内直接引用。
- 假设：主人希望仓库级 skill 安装记录与资产一起移除，而不是保留历史安装规格。
