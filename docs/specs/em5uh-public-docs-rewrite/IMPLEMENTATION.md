# 实现状态（公开文档体系重写与职责收口）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-19
- Last: 2026-04-19
- Summary: 已交付；PR #95; fast-track / public docs rewrite / PR ready
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `README.md`：收口为仓库入口与最短启动路径
- `docs-site/docs/*`：按公开文档职责重写
- `docs/product.md`：重写为内部产品参考
- `web/README.md`：收口为前端与 Storybook 贡献说明
- `docs-site/rspress.config.ts`：调整导航与侧边栏

## 资产晋升（Asset promotion）

None

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 重写 README 与 docs-site 公开文档，使每个入口只承担一种职责
- [x] M2: 重写内部产品参考与前端 README，消除与公开文档的重复描述
- [x] M3: 通过 docs build、Storybook build、assembled smoke check，并完成浏览器验收与 PR-ready 收口

## 交付结果（Delivery outcome）

- PR：`#95`
- 分支：`th/em5uh-public-docs-rewrite`
- 收口状态：PR ready（未自动合并）

## 验证结果

### Verification summary

- docs-site build：通过
- Storybook build：通过
- assembled Pages smoke check：通过
- 浏览器核验：首页、快速开始、配置、产品、Storybook 导览页均已检查
