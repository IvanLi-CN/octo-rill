# 实现状态（Translation worker board follow-up）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-09
- Last: 2026-03-27
- Summary: 已交付；PR #32, PR #38; 3 general + 1 user_dedicated worker board + runtime lease recovery
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 当前实现说明

- 统一翻译调度器、管理员翻译调度 tab、请求/批次详情抽屉与 LLM 详情链路已经存在。
- 固定 `3 general + 1 user_dedicated` worker runtime、`request_origin`、`worker_slot` 与 `request_count` 已落地。
- `translation_batches` 现在额外持有运行期 lease 元数据；worker 在执行时持续心跳，启动恢复与周期 sweep 会把过期批次级联关闭到 request/work item/linked llm call 终态。
- 管理页工作者板、队列/历史 tabs 与详情跳转链路已能反映上述真实运行态。

## 计划资产（Plan assets）

- Directory: `docs/specs/nbz5z-translation-worker-board/assets/`
- In-plan references: `![...](./assets/<file>.png)`
- PR visual evidence source: maintain `## Visual Evidence (PR)` in this spec when PR screenshots are needed.
- If an asset must be used in impl (runtime/test/official docs), list it in `资产晋升（Asset promotion）` and promote it to a stable project path during implementation.

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: follow-up spec、DB/runtime 契约与管理端接口增量冻结。
- [x] M2: 固定 4 槽位 translation worker runtime + `request_origin`/`worker_slot`/`request_count` 落地。
- [x] M3: 管理页重构为工作者板 + 需求队列/任务记录 tabs，并完成移动端退化。
- [ ] M4: Storybook / Playwright / Rust 测试收口，快车道 PR 与 review-loop 收敛。
