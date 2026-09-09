# 星标同步分片对账与独立调度 History

## Decision Trace

- 浅 Star 快照只 upsert 最新窗口，无法删除窗口外的取消星标；因此不能继续把浅快照当作完整 membership truth。
- GitHub GraphQL 给出 `totalCount` 和 cursor，但没有随机 page offset；因此完整扫描采用单页顺序 slice，不采用并行预切页号。
- 取消星标的删除延迟到一个 connection 的完整 epoch 成功结束，且必须保护 epoch 期间由 delta 写入的更新观察。
- Release 订阅/治理与 Star 的工作量、正确性和频率需求不同；两套 runtime clocks 由 ADR 0006 明确分离。
- 实现采用 connection membership ledger 与 terminal-only prune，保留既有用户级 `starred_repos` 读模型，避免可见性消费者感知 cursor/epoch 细节。
- 管理面将 Star delta interval 与 full-sweep completion target 独立持久化，并显示活跃扫描与最近完成事实。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
