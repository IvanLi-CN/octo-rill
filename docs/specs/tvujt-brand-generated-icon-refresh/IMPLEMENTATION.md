# 实现状态（品牌刷新：生成图接管 favicon / Web / Docs）

## 当前状态

- Lifecycle: active
- Implementation: 已交付
- Created: 2026-04-06
- Last: 2026-04-06
- Summary: 已交付；local implementation completed; PR pending screenshot push approval
- Spec: [SPEC.md](./SPEC.md)
- History: [HISTORY.md](./HISTORY.md)

## 文档更新（Docs to Update）

- `README.md`
- `docs-site/rspress.config.ts`
- `docs/specs/README.md`

## 资产晋升（Asset promotion）

| Asset | Plan source (path) | Used by (runtime/test/docs) | Promote method (copy/derive/export) | Target (project path) | References to update | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| 新品牌参考图 | `brand/source/reference/generated-brand-refresh-reference.png` | docs/render | copy | `brand/source/reference/generated-brand-refresh-reference.png` | 本 spec / render script | 主人确认的最新生成图，只作参考与派生源 |
| 正式品牌导出物 | `brand/exports/*` | runtime/docs | export | `brand/exports/*` | README / public copies | Web、docs-site、README 使用的正式资产 |
| Web public 品牌副本 | `brand/exports/*` | runtime | copy | `web/public/brand/*`, `web/public/favicon.*` | `web/index.html`, `BrandLogo` | 供 Vite / Storybook 消费 |
| docs-site public 品牌副本 | `brand/exports/*` | docs | copy | `docs-site/docs/public/brand/*`, `docs-site/docs/public/favicon.*` | `docs-site/rspress.config.ts` | 供 Rspress 消费 |

## 实现里程碑（Milestones / Delivery checklist）

- [x] M1: 归档新参考图并完成新的品牌导出链路。
- [x] M2: Web、docs-site 与 README 全部接入新的品牌家族。
- [x] M3: Storybook 与本地视觉证据刷新完成。
