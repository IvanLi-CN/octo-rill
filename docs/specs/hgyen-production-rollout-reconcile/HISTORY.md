# 演进记录（production rollout reconcile 与 release 后版本追平验收）

- 2026-06-25：复核线上性能回归时确认 `v2.37.5` GitHub Release 与 GHCR 镜像已存在，但 101 上生产容器仍停在 `v2.37.4`。由此明确根因不是代码修复失效，而是“release 自动化”和“production rollout”之间存在未被记录也未被验收的空档。
- 2026-06-25：决定不把 production deploy 改成 GitHub Actions 直连 SSH，而是沿用 101 上 `/home/ivan/srv` 作为机器真相源，新增 stack-local reconcile 脚本 + user-systemd timer，让生产实例 pull-based 跟随 `ghcr.io/ivanli-cn/octo-rill:latest`。
- 2026-06-25：为避免再次出现“GitHub Release 成功但线上仍旧版”却无人察觉的情况，新增 stable release 后的 `verify-prod-rollout` gate，把生产 `/api/health.version` 追平纳入 release workflow 的成功条件。
