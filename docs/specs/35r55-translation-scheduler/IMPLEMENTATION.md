# 实现状态（统一翻译调度器与独立管理界面改造）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-03-07
- Last: 2026-03-27
- Summary: 已交付；PR #38; unified request scheduler + stale runtime recovery completed
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 当前实现说明

- 统一翻译调度器已落地：请求、去重 work item、watcher、batch 与 `llm_calls.parent_translation_batch_id` 已入库。
- 新生产者入口已切到 `POST/GET /api/translate/requests*`，旧翻译接口仍保留兼容 shim 并复用同一翻译核心，避免前后端滚动发布时缓存 bundle 立刻失效。
- Feed 自动翻译改为 `stream` 请求，Release Detail 改为 `wait` 请求，管理员在 `/admin/jobs` 可查看“翻译调度”标签页与批次/LLM 追链。
- 当前批次执行层仍按 `kind + entity_id + scope_user_id` 复用既有翻译核心函数；`source_blocks` / `target_slots` 已作为统一协议、去重哈希与管理端展示输入。
- `translation_batches` 与关联 `llm_calls` 现在持有运行期 lease；服务启动前先回收孤儿 `running` 记录，运行中按固定心跳/过期阈值做 sweep，避免请求、work item、batch、LLM 调用卡死在 `running`。

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: DB schema + translation scheduler runtime + unified producer API.
- [x] M2: Feed / Release Detail / Notification producers migrated to unified API.
- [x] M3: Admin translations tab + request/batch detail + LLM linkage.
- [ ] M4: Validation, review convergence, and PR-ready docs sync.
