# OctoRill

OctoRill is a personal GitHub activity workspace. This glossary fixes the product terms that shape reading, admin, and sync surfaces so future changes do not drift across similar-but-different repository concepts.

## Language

**项目处理仓库总数**:
The deduplicated count of repositories OctoRill currently processes for one user across watched repositories and owned-repository baselines. One repository counts once even if it appears in multiple sources.
_Avoid_: 关注 + 私有仓库, 仓库总数（未说明口径）, private repo total

**关注仓库**:
A repository stored from the user's GitHub starred-repository snapshot. This is the canonical social/release source for explicit user attention.
_Avoid_: watched repo, processed repo, owned repo

**自有仓库基线**:
A repository baseline discovered from the current GitHub viewer's owner repository snapshot and stored for release/social processing. It is not equivalent to a watched repository or a private-repository flag.
_Avoid_: 私有仓库, star baseline, watched repo

**我的发布纳入状态**:
The user preference that controls whether owned-repository baselines participate in release visibility. It describes release inclusion, not repository ownership or sync freshness.
_Avoid_: 私有仓库开关, owner repo enabled

**LLM 逻辑调用**:
一次由 OctoRill 调度并最终归属于单个模型的 LLM 请求。内部重试仍属于同一次逻辑调用，最终只产生一个成功或失败结果。
_Avoid_: 执行, attempt, 单次重试

**可见进展分页**:
Dashboard `全部` tab 的分页规则：自动续载只在新页扩展用户可见的日组、日报面板或原始活动时继续；若新页完全被既有历史日报折叠，则转为用户点击的显式续载。
_Avoid_: 无限自动补拉, 哨兵重试, 隐藏页循环
