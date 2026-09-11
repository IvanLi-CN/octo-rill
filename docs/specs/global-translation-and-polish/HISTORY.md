# 全局翻译与润色工作模型演进记录

## Lifecycle

- Lifecycle: active
- This topic owns global content-processing identity, authorization association, result projection, legacy evidence and cutover compatibility.
- The existing translation scheduler topic continues to own shared batching, lease, recovery and attempt-audit mechanics.

## Compatibility

- The former user-scoped scheduler and cache tables remain historical evidence and are never rewritten by this topic.
- A result found only in legacy evidence is not promoted to a current global result or work state.
- The migration-bearing compatibility version is the earliest application version permitted after the global schema exists. A migration-preceding binary is intentionally unsupported after that point.
