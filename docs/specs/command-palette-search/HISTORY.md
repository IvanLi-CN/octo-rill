# 命令面板与个人工作区搜索主题历史

> 这里记录主题局部生命周期、兼容性与必要背景；规范正文仍以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- New active topic. No predecessor or successor.
- The endpoint is additive and the existing reading, synchronization, notification and brief routes remain the source of truth for their own workflows.

## Replacements / Background

- The feature deliberately separates local workspace search from GitHub Search API so search cannot expand the user's visibility scope or trigger feed-side writes.
- The quota model is a lazy-start anchored fixed-window counter: a user's first valid search anchors a 300-second interval, avoiding strict sliding-window storage while retaining cross-tab and restart consistency through the shared database state.
- Original, translated and smart expressions are modeled as one canonical search document so opening a result preserves the existing reading target instead of creating parallel result rows.

## Related Changes

- None

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
