# Dashboard 可读区块分页 主题历史

> 本文只保留主题替代关系、兼容性和必要背景；长期行为见 `./SPEC.md`，实现事实见 `./IMPLEMENTATION.md`。

## Lifecycle / Compatibility

- 本主题是 `dashboard-folded-history-pagination` 的后继。旧主题的客户端“可见进展”暂停机制不再是有效的 Dashboard 根页分页合同。
- `/api/feed` 继续服务原始动态、筛选和 scoped 阅读；`/api/briefs` 与其详情接口继续服务日报 tab、侧栏和兼容调用方。

## Replacements / Background

- 历史日报折叠曾在客户端根据渲染投影暂停 raw cursor，以避免自动请求不可见历史页。生产复现证明数据请求成功后仍可能没有新的可读区块，因此该机制只能暴露为“继续加载历史动态”，不能满足连续阅读。
- 后继合同把区块排序、日报覆盖与 continuation 边界移到服务端，并把完整原始动态延迟到用户明确选择“列表”。完整日报和未覆盖补充动态仍属于主阅读流。

## Related Changes

- PR #250: 客户端显式续载实现，作为被替代行为的历史交付记录。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
- `../dashboard-folded-history-pagination/SPEC.md`
