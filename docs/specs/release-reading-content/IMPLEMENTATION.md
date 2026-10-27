# Release 阅读内容与变更依据实现状态

## Current Status

- Lifecycle: active
- Implementation: 主要内容链路、状态合同、公开投影隔离和最近 30 天回填实现已完成；跨账号与重型集成验证仍待完成。
- 本主题不触发生产部署或生产回填启动；旧结果在新结果校验通过前继续可读。

## Implementation Coverage

开发基线为 `765e66079839c3e4e00b69131fcb4eecdf5fe4f3`，实现分支为 `th/release-reading-content`。下表记录当前代码覆盖和仍需验收的运行事实。

| 需求 | 已有能力与缺口 |
| --- | --- |
| REQ-RC-READING、REQ-RC-EDITING | `ReleaseReadingResult` 固化版本、变化、无信息结论和机器材料；Release 专属 prompt/校验过滤归属尾句、版本复述、模板话术和来源区。 |
| REQ-RC-VISIBILITY | Feed 与详情优先渲染结构化变化；仅服务端从冻结 evidence 派生的范围/SHA 等 `machine_materials` 使用原生 `details` 折叠，机器材料不进入日报。 |
| REQ-RC-EVIDENCE | `src/api.rs` 冻结原文、语义版本基线、base/merge-base/head SHA、过滤后的 compare digest 和覆盖范围；发布时间晚于当前 Release 的候选会被排除，`diverged` 会改用固定两端比较；比较失败保持失败状态，不推断无变化。 |
| REQ-RC-CONSUMPTION | global projection 同时保存 `reading` 与兼容 title/summary；`smart_public` 与 `smart_authorized` 分离公开原文和授权差异；旧翻译接口提供可选 `reading` 字段；Feed、详情和日报从同一事实消费，日报不再反解来源链接或提示式列表。 |
| REQ-RC-STATES | `reading.outcome` 与工作 status 分离；合法 `insufficient` 保留最简发布记录，queued/running/failed/不兼容结果不伪装成无信息。 |
| REQ-RC-BRIEF | `build_repo_rendered` 和 `build_brief_markdown` 删除相关链接及占位 fallback；新 migration `0102` 冻结最近 30 天目标并按最多 100 条批次可恢复处理。 |

## Verification

- `cargo fmt --all -- --check`、`cargo check`、AI/翻译/内容处理/迁移/回填和 Release 基线测试已通过；`git diff --check` 与规范合同检查也已通过。
- `bun run tsc -b`、`bun run lint` 和 Storybook 测试已通过；桌面/移动 Storybook 元素证据已保存到 `assets/`，提交门禁仍为 `pending-owner-approval`。
- 两个授权账号、私有公开发布隔离、GitHub 403/429 和完整回填替换仍属于合并前集成验证；当前重型全量套件需要 Agent VM。

## Remaining Gaps

- 补齐公开/私有可见性切换、搜索/缓存隔离和回填失败保留旧结果的集成测试；当前搜索投影只允许 `smart_public`，授权差异由授权阅读面按 `smart_authorized` 读取；重型全量套件需在 Agent VM 中运行。
- 继续保留旧格式结果的可读兼容；既有 ready 投影和日报快照不会因 prompt 变化自动重算，生产回填需另行授权。

## References

- [SPEC.md](./SPEC.md)
- [HISTORY.md](./HISTORY.md)
