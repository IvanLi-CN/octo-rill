# 仓库级 Worktree Bootstrap（#gvxnw）

## 状态

- Status: 已完成
- Created: 2026-03-06
- Last: 2026-03-07

## 背景 / 问题陈述

- Codex App 会频繁创建新的 linked worktree，但仓库当前不会自动补齐 `.env.local` 等本地开发资源。
- 现有仓库依赖手动复制环境文件，换 worktree 或换机器时容易漏配，导致开发启动失败或重复劳动。
- 主人要求方案必须对 Codex 友好，但不绑定 Codex 私有扩展点，保证手工 `git worktree add` 也同样生效。

## 目标 / 非目标

### Goals

- 在仓库内提供 linked worktree 首次 checkout 自动 bootstrap 的通用方案。
- 使用 repo-local `lefthook` 安装路径，并把 hooks 统一安装到主工作区，避免新机器依赖全局二进制或绑定临时 worktree。
- 提供可扩展的资源清单、可选源目录 override，以及可在 macOS/Linux CI 上复验的 smoke test。

### Non-goals

- 不修改 Codex App 内部行为。
- 不覆盖已存在的目标文件，也不自动同步清单外资源。
- 不承诺原生 PowerShell-only Windows 兼容。

## 范围（Scope）

### In scope

- `post-checkout` hook 驱动的 worktree bootstrap。
- `scripts/worktree-sync.paths` 资源清单与 `scripts/sync-worktree-resources.sh`。
- repo-local hook 安装入口、README onboarding 更新、CI smoke job。

### Out of scope

- GitHub 远端配置或 Codex 私有 undocumented hook。
- 覆盖式同步、删除目标文件、跨机器 secret 分发。
- 清单外本地目录（如证书、IDE 私有缓存）的自动处理。

## 需求（Requirements）

### MUST

- 新 linked worktree 首次 checkout 时，若源路径存在且目标缺失，则自动复制 `.env.local` 与 `.env`。
- 同步脚本不得写死机器绝对路径、用户名目录或 Codex 私有目录模式。
- 主工作区执行 hook、普通 checkout、缺失源路径、重复执行必须安全 no-op 或 skip。
- 仓库内必须提供无需全局 `lefthook` 的安装入口，并把共享 hooks 固定到仓库内解析出的 `lefthook` 二进制。
- 安装入口必须把 repo-local `core.hooksPath` 收敛到共享 hook 目录，避免已有自定义 hooksPath 让安装失败或把 hooks 分散到单个 worktree。
- CI 必须在 `ubuntu-latest` 与 `macos-latest` 上验证 smoke flow。

### SHOULD

- 资源清单采用 repo-relative 列表文件，便于后续扩展。
- 支持通过本地 Git 配置覆盖源工作区根目录，给非常规布局兜底。
- 提供 dry-run 与 force 入口，便于手工排障与验证。

## 功能与行为规格（Functional/Behavior Spec）

### Core flows

- 开发者在主工作区根目录安装 repo-local hooks 后，任意 linked worktree 在首次 checkout 触发 `post-checkout`，同步脚本自动读取资源清单并执行“缺失时复制”。
- 同步脚本默认优先读取共享 Git 配置中记录的主工作区根目录，并在标准布局下回退到 Git 元数据推导；若配置 `codex.worktree-sync.source-root`，则优先使用该 override。
- README 将 `.env.local` 作为推荐的每人本地 secrets 文件，并说明 `scripts/worktree-sync.paths` 的扩展方式。
- hook 安装脚本在共享 `.git/hooks` 中优先注入仓库内解析出的 `LEFTHOOK_BIN`，避免本机全局 `lefthook` 抢占执行；若该路径失效，则回退到 hook 自带的常规 `lefthook` 发现逻辑。
- hook 安装脚本会把 repo-local `core.hooksPath` 重定向到共享 hook 目录，再执行 `lefthook install --force`，避免已有自定义 hooksPath 阻断安装或落到单个 worktree。
- `post-checkout` hook 必须在当前 revision 缺少同步脚本时安全跳过，避免切回历史提交或维护分支时因共享 hooks 报错。

### Edge cases / errors

- 若源路径不存在：记录 `skip source missing`，继续处理清单中的其他条目。
- 若目标已存在文件、目录或软链接：记录 `keep target exists`，不覆盖。
- 若在主工作区执行脚本：直接记录 `skip main worktree` 并退出。
- 若当前 checkout 缺少 `scripts/sync-worktree-resources.sh`：共享 `post-checkout` hook 必须直接 no-op，不得让 checkout 失败。
- 若共享 hook 中记录的 `LEFTHOOK_BIN` 已不存在：hook 必须回退到当前环境可用的 `lefthook` 发现逻辑，不得因为陈旧绝对路径导致 checkout 失败。
- 若仓库已配置本地 `core.hooksPath`：安装入口必须仍能成功完成，并把 hooks 收敛到共享 hook 目录。
- 若启用 `WORKTREE_SYNC_DRY_RUN=1`：只输出将执行的动作，不落盘。

## 接口契约（Interfaces & Contracts）

- `scripts/worktree-sync.paths`：每行一个 repo-relative 路径；支持空行和 `#` 注释。
- `scripts/sync-worktree-resources.sh <old_head> <new_head> <is_branch_checkout>`：供 Git `post-checkout` 调用；支持 `WORKTREE_SYNC_FORCE=1` 与 `WORKTREE_SYNC_DRY_RUN=1`。
- `git config codex.worktree-sync.source-root <path>`：可选本地 override，仅用于非常规布局；支持绝对路径和相对主工作区根目录的写法。

## 验收标准（Acceptance Criteria）

- Given 主工作区存在 `.env.local` 与 `.env`，且已安装 repo-local hooks  
  When 执行 `git worktree add --detach <path> HEAD`  
  Then 新 worktree 会自动得到缺失的 `.env.local` 与 `.env`。

- Given 新 worktree 已存在 `.env.local`  
  When 再次执行同步脚本  
  Then 原文件内容保持不变，并输出 `keep target exists`。

- Given 源工作区缺少某个清单路径  
  When 执行同步脚本  
  Then 脚本输出 `skip source missing` 且命令成功返回。

- Given 在主工作区执行同步脚本  
  When 传入 `WORKTREE_SYNC_FORCE=1`  
  Then 脚本输出 `skip main worktree` 且不复制任何文件。

- Given CI 在 macOS 与 Linux runner 上执行 smoke test  
  When 运行 worktree bootstrap 测试入口  
  Then 两个平台均通过，且测试过程不依赖当前开发机绝对路径。

- Given 共享 hooks 已安装，随后 checkout 到仍保留 `lefthook.yml` 但缺少同步脚本的历史 revision  
  When Git 触发 `post-checkout`  
  Then hook 必须直接跳过，不得因为脚本不存在导致 checkout 失败。

- Given 共享 hooks 已安装，随后旧 revision 重新安装依赖导致先前固定的 `LEFTHOOK_BIN` 丢失  
  When Git 再次触发 `post-checkout`  
  Then hook 必须回退到当前环境可用的 `lefthook` 发现逻辑，且 checkout 不得因陈旧绝对路径失败。

- Given 仓库本地已配置自定义 `core.hooksPath`  
  When 执行 `bun install` 安装 repo-local hooks  
  Then 安装必须成功完成，并把 repo-local `core.hooksPath` 收敛到共享 hook 目录，使后续 linked worktree 仍能复用同一套 hooks。

- Given 仓库通过 `git clone --separate-git-dir=<dir>` 初始化，且主工作区已安装 repo-local hooks  
  When 再创建 linked worktree  
  Then 新 worktree 仍会从主工作区复制缺失资源，且共享 hooks 不会把二进制或源目录固定到外置 Git 目录。

## 实现前置条件（Definition of Ready / Preconditions）

- worktree bootstrap 触发层级已锁定为仓库级 Git hook，而不是 Codex 私有扩展点。
- 兼容目标已锁定为 macOS/Linux。
- 默认同步策略已锁定为“缺失时复制”。
- 初始资源清单已锁定为 `.env.local` 与 `.env`。

## 非功能性验收 / 质量门槛（Quality Gates）

### Testing

- Shell syntax: `sh -n scripts/sync-worktree-resources.sh`
- Smoke test: `bun run test:worktree-bootstrap`
- CI smoke matrix: `ubuntu-latest` + `macos-latest`

### Quality checks

- Root tooling install: `bun install`
- Existing repo checks remain green: `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test --locked --all-features`

## 文档更新（Docs to Update）

- `README.md`: 新增 repo-local hook 安装与 `.env.local` worktree bootstrap 说明。
- `docs/specs/README.md`: 新增并同步该规格状态。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 增加 repo-local hook 安装入口与 `post-checkout` wiring。
- [x] M2: 落地 worktree 同步脚本与资源清单。
- [x] M3: 增加 smoke test 与 CI matrix 校验，并同步文档与规格状态。

## 方案概述（Approach, high-level）

- 使用 Git `post-checkout` 作为唯一自动触发点，让 Codex 新工作树与手工 worktree 共享同一实现。
- repo-local `lefthook` 负责把 hook 安装到共享 `.git/hooks`，避免依赖全局工具链。
- 通过 smoke test 直接创建 linked worktree 验证行为，而不是只做脚本单元测试。

## 风险 / 开放问题 / 假设（Risks, Open Questions, Assumptions）

- 风险：开发者若未在主工作区执行一次根目录 `bun install`，hooks 不会自动安装。
- 风险：若主工作区被手工移动后未重新安装 hooks，记录在共享 Git 配置中的主工作区根目录可能过期，需要重新执行 `bun install` 或显式设置 override。
- 风险：若开发者明确需要原生 PowerShell-only Windows 安装体验，当前 POSIX shell 入口仍需另行设计；本规格暂不覆盖。
- 风险：若开发者原本依赖 repo-local 自定义 `core.hooksPath` 承载其他 hook 管理逻辑，安装入口会将其收敛到共享 hook 目录；若需共存，后续需补充迁移方案。
- 开放问题：无。
- 假设：团队接受把 `.env.local` 作为推荐的 per-developer secrets 文件。

## 变更记录（Change log）

- 2026-03-06：创建规格，冻结仓库级 worktree bootstrap 方案与验收口径。
- 2026-03-06：完成 repo-local hook 安装入口、worktree bootstrap 脚本、smoke test 与 CI matrix 校验。
- 2026-03-06：补充共享 hooks 的 `LEFTHOOK_BIN` 固定逻辑，避免全局 Lefthook 抢占。
- 2026-03-06：补充共享 Git 配置里的主工作区根目录记录，并增加 `--separate-git-dir` smoke 覆盖。
- 2026-03-07：补充共享 `post-checkout` hook 的历史 revision 安全跳过逻辑，并收紧 README 对 linked worktree `bun install` 行为的表述。
- 2026-03-07：补充共享 hook 对失效 `LEFTHOOK_BIN` 的自动回退，并在 README 中显式标注 `macOS/Linux` 支持边界。
- 2026-03-07：补充对 repo-local `core.hooksPath` 的共享目录收敛，避免自定义 hooksPath 阻断安装或让 hooks 漏到单个 worktree。
