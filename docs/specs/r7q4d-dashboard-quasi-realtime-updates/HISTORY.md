# Dashboard 准实时列表更新历史（#r7q4d）

## Decisions

- 选择混合方案：已知任务继续复用 task SSE，未知后台定时同步使用轻量 watermark 轮询发现。
- 不在 v1 增加全局 SSE/WebSocket，因为后台同步是批处理完成后一次性展示，用户体验更适合“可揭示的新批次”。
- 新内容状态仅保留在当前前端 session，刷新页面后重新建立 baseline。
