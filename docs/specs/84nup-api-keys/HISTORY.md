# API Key 用户接口调用演进历史（#84nup）

> 这里记录会影响 Agent 理解“为什么一步步变成现在这样”的关键演进；单次任务流水账不放这里，规范正文仍以 `./SPEC.md` 为准。

## Decision Trace

- 新增本 spec：API Key 是独立的认证与权限边界，不并入 GitHub PAT 或 LinuxDO 设置 spec。

## Key Reasons / Replacements

- 用户需要脚本和外部客户端访问 OctoRill 用户态业务接口，但现有浏览器 session 不适合非交互调用。
- API Key 不应等同网页登录态；账号绑定、凭据、admin 与 API Key 管理仍必须由 session 执行。
- 后端只保存 hash，避免 API Key 泄漏风险扩大为数据库可逆秘密泄漏。

## References

- `./SPEC.md`
- `./IMPLEMENTATION.md`
