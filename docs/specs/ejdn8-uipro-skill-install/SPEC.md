# OctoRill 接入 UI UX Pro Max Skill（#ejdn8）

## 状态

- Status: 已完成
- Created: 2026-02-26
- Last: 2026-02-26

## 背景 / 问题陈述

- 当前仓库默认忽略 `.codex/`，导致项目级 skill 资产无法被追踪与复用。
- 主人明确要求在 `octo-rill` 内安装 `ui-ux-pro-max`，并避免把 skill 目录排除在版本控制之外。

## 目标 / 非目标

### Goals

- 在仓库中安装 `.codex/skills/ui-ux-pro-max/**` 并可执行搜索脚本。
- 调整 `.gitignore`：允许追踪 `.codex/skills/**`，继续忽略 Python 缓存文件。
- 通过最小命令验证 skill 可用。

### Non-goals

- 不改动后端/前端业务实现。
- 不引入额外运行时依赖与服务配置变更。

## 范围（Scope）

### In scope

- `.codex/skills/ui-ux-pro-max/**` 安装产物。
- `.gitignore` 中 Codex 相关规则调整。
- 与该安装任务相关的规格文档更新。

### Out of scope

- `src/**` 与 `web/src/**` 功能逻辑修改。
- CI 工作流与发布流程修改。

## 需求（Requirements）

### MUST

- `ui-ux-pro-max` 的 `SKILL.md`、`data/**`、`scripts/**` 必须存在于仓库内。
- `.codex/skills/**` 默认可被 Git 跟踪。
- `.codex/skills/**/__pycache__/` 与 `*.pyc` 必须保持忽略。

### SHOULD

- 至少执行一次 `search.py` 验证命令并成功返回结果。

## 功能与行为规格（Functional/Behavior Spec）

- 安装命令执行后，目录 `.codex/skills/ui-ux-pro-max/` 完整可见。
- `python3 .codex/skills/ui-ux-pro-max/scripts/search.py ...` 可直接在仓库根目录运行。
- `git check-ignore` 对 skill 文件返回“不忽略”。

## 验收标准（Acceptance Criteria）

- Given 已完成安装  
  When 检查 `.codex/skills/ui-ux-pro-max/SKILL.md`  
  Then 文件存在且可被 Git 跟踪。

- Given 已更新 `.gitignore`  
  When 执行 `git check-ignore .codex/skills/ui-ux-pro-max/SKILL.md`  
  Then 返回未命中忽略规则（退出码 1）。

- Given skill 已安装  
  When 执行 `python3 .codex/skills/ui-ux-pro-max/scripts/search.py "dashboard" --domain style -n 1`  
  Then 命令成功并输出至少 1 条结果。

## 实现里程碑（Milestones）

- [x] M1：安装 `ui-ux-pro-max` 到项目目录。
- [x] M2：修正 `.gitignore`，允许跟踪 `.codex/skills/**`。
- [x] M3：完成命令级验证并记录规格。

## 风险 / 开放问题 / 假设

- 风险：上游模板更新可能导致未来安装产物结构变化。
- 开放问题：无。
- 假设：团队接受将 skill 资产纳入仓库版本管理。

## 变更记录（Change log）

- 2026-02-26：创建并完成本规格，实现项目内 skill 接入与跟踪规则调整。
