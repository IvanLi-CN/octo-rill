# 实现状态（production rollout reconcile 与 release 后版本追平验收）

> 当前有效规范以 [`./SPEC.md`](./SPEC.md) 为准；这里记录当前实现覆盖、验证与 rollout 事实。

## Summary

已实现首版闭环：

- 101 上新增了 OctoRill host-side rollout reconcile 脚本与 user-systemd timer。
- stable `push@main` 的 `Release` workflow 新增 `verify-prod-rollout` job，直接验收公网 `/api/health.version` 是否追平本次发布版本。
- 内部项目文档已明确区分 GitHub Release automation 与 production rollout automation。

## Coverage

- Host-side reconcile
  - `/home/ivan/srv/octo-rill/bin/octo-rill-rollout.sh`
  - `/home/ivan/srv/octo-rill/systemd/octo-rill-rollout.service`
  - `/home/ivan/srv/octo-rill/systemd/octo-rill-rollout.timer`
  - `~/.config/systemd/user/octo-rill-rollout.{service,timer}` symlink install
- Repo release verification
  - `.github/workflows/release.yml` `verify-prod-rollout` job
  - `.github/scripts/verify_release_rollout.py`
  - `.github/scripts/test-release-automation.sh` contract + script coverage
- Project docs truth
  - `docs/repository-governance.md`
  - `docs/README.md`
  - `docs/architecture.md`

## Validation

- Host self-test:
  - `systemctl --user start octo-rill-rollout.service`
  - 结果：当前镜像未变化时 `before_started == after_started`，`recreated=false`，健康检查一次通过。
- Timer state:
  - `systemctl --user list-timers --all | grep octo-rill-rollout`
  - 结果：timer 已启用并周期触发。
- Production health:
  - `curl -fsS https://octo-rill.ivanli.cc/api/health`
  - 结果：返回 `{"ok":true,"version":"2.37.5"}`。
- Repo-side self-tests:
  - 见 CI `Quality gates self-tests` 中的 `test-release-automation.sh` 覆盖。

## Remaining notes

- host-side reconcile 当前是机器 101 上的运维事实，不属于 repo tracked files；其维护记录与 deployment card 留在 `/home/ivan/srv`。
- 当前 verify gate 只覆盖 stable `push@main` release，不对 prerelease / historical backfill 强制要求生产追平。
