# 演进记录（仓库级 Worktree Bootstrap）

## 生命周期

- Lifecycle: active
- Created: 2026-03-06
- Last: 2026-03-07

## 变更记录

- 2026-03-06：创建规格，冻结仓库级 worktree bootstrap 方案与验收口径。
- 2026-03-06：完成 repo-local hook 安装入口、worktree bootstrap 脚本、smoke test 与 CI matrix 校验。
- 2026-03-06：补充共享 hooks 的 `LEFTHOOK_BIN` 固定逻辑，避免全局 Lefthook 抢占。
- 2026-03-06：补充共享 Git 配置里的主工作区根目录记录，并增加 `--separate-git-dir` smoke 覆盖。
- 2026-03-07：补充共享 `post-checkout` hook 的历史 revision 安全跳过逻辑，并收紧 README 对 linked worktree `bun install` 行为的表述。
- 2026-03-07：补充共享 hook 对失效 `LEFTHOOK_BIN` 的自动回退，并在 README 中显式标注 `macOS/Linux` 支持边界。
- 2026-03-07：补充对 repo-local `core.hooksPath` 的共享目录收敛，避免自定义 hooksPath 阻断安装或让 hooks 漏到单个 worktree。
- 2026-03-07：补充 linked worktree 本地 `lefthook.yml` 新增 hook 类型时的共享 wrapper 更新，并优先固定到当前 worktree 的 `lefthook`、回退到主工作区二进制。
- 2026-03-07：补充 `LEFTHOOK_BIN` 的 shell-safe 写入与带 `$` 路径 smoke 覆盖，避免 hook 运行时意外展开。
- 2026-03-07：补充对既有 hook 链的链式保留（含默认 `.git/hooks` 的 `*.old` 备份），避免 `core.hooksPath` 收敛后静默停掉旧 hook。
