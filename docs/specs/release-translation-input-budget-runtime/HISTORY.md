# 演进记录（Release 翻译输入预算与运行时设置收口）

## 生命周期

- Lifecycle: active
- Created: 2026-04-13
- Last: 2026-07-07

## 变更记录

- 2026-04-13: 建立当前 contract，明确 release 翻译限制来自 LLM 输入预算而非正文长度上限；旧 `#3k9fd` / `#epn56` 转为历史参考。
- 2026-07-07: release detail batch 翻译不再只按输入预算组批；新增输出预算估算与动态 `max_tokens`，并在 batch JSON 截断、解析失败或缺项时先二分拆小重试，避免整批直接退回逐条标题/正文调用。
