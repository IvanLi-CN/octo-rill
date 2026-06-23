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
