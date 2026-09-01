# Dashboard 折叠历史的可见进展分页 主题历史

> 这里记录主题局部生命周期、替换、兼容性与必要背景；完整 ADR 取舍保留在 `docs/adr/`。单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Lifecycle / Compatibility

- 新主题；与既有 Dashboard 日分组和历史日报折叠规范兼容，不替代其阅读语义。

## Replacements / Background

- 原有历史日报折叠正确地减少了可见卡片，但没有定义“成功追加却无可见投影变化”时的分页行为。本主题补齐该交界处的稳定契约。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `../dashboard-day-grouping/SPEC.md`
- `../dashboard-brief-social-folding/SPEC.md`
