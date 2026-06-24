---
title: Release publication does not prove production rollout
module: release-ops
problem_type: rollout-gap
component: github-actions, ghcr, production-host
tags: [release, rollout, ghcr, production, ops]
status: active
related_specs:
  - docs/specs/at76w-release-reliability-backfill/SPEC.md
  - docs/specs/hgyen-production-rollout-reconcile/SPEC.md
---

# Release publication does not prove production rollout

## Context

OctoRill publishes stable versions through `.github/workflows/release.yml`. That workflow computes the version, creates or repairs the GitHub Release, and pushes GHCR tags including `:latest`.

Production runs separately on machine 101 under `/home/ivan/srv/octo-rill/`, where the container follows `ghcr.io/ivanli-cn/octo-rill:latest`.

## Symptoms

- GitHub shows a new stable release tag, but `https://octo-rill.ivanli.cc/api/health` still reports the previous version.
- Release workflow is green, yet production logs still show the old runtime behavior.
- Maintainers waste time re-diagnosing already-fixed code because they assume “release exists” means “production updated”.

## Root cause

Publishing the GitHub Release and GHCR image only proves that artifacts exist. It does not prove that the production host has pulled the new image or recreated the container.

If there is no explicit host-side reconcile mechanism and no release-side verification of the live `/api/health.version`, the gap between “artifact published” and “production live” remains invisible.

## Resolution

Treat release publication and production rollout as two different contracts.

- Keep `.github/workflows/release.yml` responsible for versioning, GitHub Release creation, and GHCR publication.
- Install a host-side reconcile loop on the production machine that periodically runs `docker compose pull` and `docker compose up -d` for the OctoRill stack, plus a public health check.
- Add a stable-release verification step in the release workflow that waits for the public `/api/health.version` to match the just-published `APP_EFFECTIVE_VERSION`.

For OctoRill on machine 101, the current host-side truth is:

- stack root: `/home/ivan/srv/octo-rill/`
- reconcile script: `/home/ivan/srv/octo-rill/bin/octo-rill-rollout.sh`
- systemd service: `/home/ivan/srv/octo-rill/systemd/octo-rill-rollout.service`
- systemd timer: `/home/ivan/srv/octo-rill/systemd/octo-rill-rollout.timer`

## Guardrails / Reuse notes

- Do not treat a successful GHCR push as rollout proof.
- Do not make historical backfill or prerelease publication wait for production to adopt that exact version; production rollout verification should target stable mainline release runs only.
- Keep the reconcile logic pull-based on the host unless there is a strong reason to introduce repo-to-host SSH deploys.
- When production is stale, inspect the deployment card and rollout timer before reopening runtime bug hypotheses.
- Record machine-local rollout facts in the host ops repo (`/home/ivan/srv`), not in application compose files inside the app repo.

## References

- `.github/workflows/release.yml`
- `.github/scripts/verify_release_rollout.py`
- `docs/repository-governance.md`
- `docs/specs/hgyen-production-rollout-reconcile/SPEC.md`
