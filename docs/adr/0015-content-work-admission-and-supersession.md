# ADR 0015: Content Work Admission and Supersession Boundary

Status: accepted

全局内容工作项按规范来源的权威源版本顺序判断当前性，而不是按同步请求到达顺序判断。进入 `superseded` 的工作项是不可重开的历史终态；重复提交只能产生独立的工作准入审计事实，不能把它重新置为 queued 或产生新的尝试。worker 在 provider 调用前必须通过一个与尝试审计一致的 provider admission 线性化边界；准入后源版本才变化时，调用可以完成但不得发布结果或安排旧版本恢复。启动时的幂等 reconciliation 只收口已知旧版本活动工作，不执行 provider 调用。

## Considered Options

- 继续允许 `superseded` 工作重开：拒绝。同步重复提交会绕过源版本边界并产生无效 provider 调用。
- 只在 provider 返回后检查源版本：拒绝。它无法阻止已知过时工作产生调用。
- 用同步到达顺序决定源版本：拒绝。乱序和重复同步会让旧内容覆盖或重开新内容。
- 把工作准入和内容处理尝试混在同一事件流：拒绝。没有 provider 调用的拒绝或 no-op 也必须可审计，而尝试事件代表实际执行。

## Consequences

- `superseded` 成为严格终态；普通重试必须转向当前源版本，历史源版本不复用普通重试入口。
- provider 成功、内容处理尝试完成和当前结果发布继续是三个独立事实。
- 线上可以区分重复 admission、旧版本拒绝、provider 已准入后的正常竞态以及真正的浪费调用。
- 需要为准入审计和 provider admission 增加持久化事实及回归覆盖；输出契约修复仍作为独立轨道交付。
