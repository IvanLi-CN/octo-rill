# Release 阅读内容与变更依据实现状态

## Current Status

- Lifecycle: active
- Implementation: 主要内容链路、状态合同、公开投影隔离和最近 30 天回填实现已完成；正式审查发现的证据完整性、可见性切换、轮询 variant 隔离、无效阅读状态、回填租约恢复、删除目标计数和旧私有搜索投影问题已修复，新的 CI/审查轮次仍待完成。
- 本主题不触发生产部署或生产回填启动；旧结果在新结果校验通过前继续可读。

## Implementation Coverage

开发基线为 `765e66079839c3e4e00b69131fcb4eecdf5fe4f3`，实现分支为 `th/release-reading-content`。下表记录当前代码覆盖和仍需验收的运行事实。

| 需求 | 已有能力与缺口 |
| --- | --- |
| REQ-RC-READING、REQ-RC-EDITING | `ReleaseReadingResult` 固化版本、变化、无信息结论和机器材料；Release 专属 prompt/校验过滤归属尾句、版本复述、模板话术和来源区。 |
| REQ-RC-VISIBILITY | Feed 与详情优先渲染结构化变化；仅服务端从冻结 evidence 派生的范围/SHA 等 `machine_materials` 使用原生 `details` 折叠，机器材料不进入日报。 |
| REQ-RC-EVIDENCE | `src/api.rs` 冻结原文、语义版本基线、base/merge-base/head SHA、过滤后的 compare digest 和覆盖范围；标题中的具体变化也作为 `release_text` 事实，发布时间晚于当前 Release 的候选会被排除，`diverged`/`behind` 会改用固定两端 tree 比较，并在有限范围内补取两端 blob；公开仓库的版本号空壳 Release 只使用匿名 compare，稳定且修订一致的公共投影（包括处理中投影）可复用，不能复用授权用户的私有差异；非语义标签只接受可验证的较低祖先；legacy 路径复用语义基线，分页、截断、基线缺失、响应不完整和比较失败保持可解释状态，不推断无变化，比较证据失败不会把同一冻结失败快照交给模型重试；需要保留的 PR 依据以服务端受控 `PR#编号` 引用呈现。 |
| REQ-RC-CONSUMPTION | global projection 同时保存 `reading` 与兼容 title/summary；服务端按当前可见性选择 `smart_public` 与 `smart_authorized`，公开状态变化不会复用私有差异，旧授权请求轮询也必须匹配当前 audience；结构化结果不兼容时 Feed、详情、日报和请求接口统一呈现 `incompatible` 事实；旧翻译接口提供可选 `reading` 字段；三个阅读面从同一事实消费，日报不再反解来源链接或提示式列表，当前任务禁止在线或真实模型测试。 |
| REQ-RC-STATES | `reading.outcome` 与工作 status 分离；合法 `insufficient` 保留最简发布记录，queued/running/failed/不兼容结果不伪装成无信息；日报快照不会用较新的处理中结果覆盖已有可读内容。 |
| REQ-RC-BRIEF | `build_repo_rendered` 和 `build_brief_markdown` 删除相关链接及占位 fallback；migration `0102`/`0103`/`0104` 冻结最近 30 天目标、提供按 release 的级联索引、批次恢复和 task 级租约栅栏。服务启动不自动创建回填；管理员显式入口、失败运行重试、过期租约、取消/崩溃恢复、owner+task CAS 和 SQLite 退避会将运行恢复到可重试队列，活动运行返回持久化状态而非机械 `queued`，重试保持原窗口并保留成功/跳过项，暂停恢复会重置等待项，回填目标被删除时按增量计入失败且不遗留活动租约。 |

## Verification

- `cargo fmt --all -- --check`、`cargo check --locked --all-targets`、`cargo clippy --locked --all-targets -- -D warnings` 和 `cargo test --locked --all-targets`（1199 passed、3 ignored）已通过；证据不可用、compare 响应完整性、固定端点 tree/blob 差异、429 限流分类、受控 PR 引用、分页/分叉比较、非语义标签祖先、回填租约、失败重试窗口、删除目标失败计数、内部证据脱敏、旧私有搜索隔离、当前 audience variant 隔离和语义不兼容阅读均有确定性测试；`git diff --check` 与规范合同检查也已通过。禁止真实模型、在线模型和生产凭据测试。
- `bun run build`、`bun run lint` 和 Storybook 测试已通过；桌面/移动 Storybook 元素证据已保存到 `assets/`，提交门禁为 `owner-approved`。
- 两个授权账号、私有公开发布切换、GitHub 403/429 和完整回填替换仍属于合并前集成验证；当前 head 需等待新的 CI 与 Tier 4 正式审查结果。

## Remaining Gaps

- 公开/私有可见性切换和回填失败保留旧结果仍需在不使用真实模型、在线 GitHub 或生产凭据的前提下补齐更高层的确定性集成夹具；当前服务端已在 canonical variant、轮询、失败状态和原子替换边界阻断私有证据泄露及旧结果覆盖。
- 继续保留旧格式结果的可读兼容；既有 ready 投影和日报快照不会因 prompt 变化自动重算，生产回填需另行授权。

## References

- [SPEC.md](./SPEC.md)
- [HISTORY.md](./HISTORY.md)
