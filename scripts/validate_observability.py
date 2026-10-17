#!/usr/bin/env python3
"""Validate the checked-in monitoring assets without optional dependencies."""

from __future__ import annotations

import json
import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
OBS = ROOT / "ops" / "observability"
REQUIRED_UIDS = {"octorill-overview", "octorill-http-sse", "octorill-sqlite", "octorill-queues", "octorill-upstream"}
METRIC = re.compile(r"\boctorill_[a-z][a-z0-9_]*")
GRAFANA_SELECTOR = 'service=~"$service",environment=~"$environment",instance=~"$instance"'


def expand_catalog_query(template: str) -> str:
    return template.replace("SERVICE_SELECTOR", GRAFANA_SELECTOR).replace("WINDOW", "5m")


def main() -> int:
    errors: list[str] = []
    catalog_path = OBS / "query-catalog.json"
    try:
        catalog = json.loads(catalog_path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        errors.append(f"query catalog: {error}")
        catalog = {}
    if catalog.get("schema_version") != 1:
        errors.append("query catalog schema_version must be 1")
    queries = catalog.get("queries", {})
    commands = catalog.get("commands", {})
    if not queries or set(commands) != {"overview", "http", "sqlite", "queues", "upstream", "health"}:
        errors.append("query catalog command set is incomplete")
    for command, ids in commands.items():
        for query_id in ids:
            if query_id not in queries:
                errors.append(f"command {command} references unknown query {query_id}")
            elif "SERVICE_SELECTOR" not in queries[query_id] and "octorill_" not in queries[query_id]:
                errors.append(f"query {query_id} is not a fixed OctoRill query")
    seen_uids: set[str] = set()
    for dashboard in sorted((OBS / "grafana" / "dashboards").glob("*.json")):
        try:
            data = json.loads(dashboard.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError) as error:
            errors.append(f"dashboard {dashboard.name}: {error}")
            continue
        uid = data.get("uid")
        if uid in seen_uids or not uid:
            errors.append(f"dashboard {dashboard.name}: missing or duplicate uid")
        seen_uids.add(uid)
        expressions = []
        for panel in data.get("panels", []):
            for target in panel.get("targets", []):
                query_id = target.get("catalog_query_id")
                expression = target.get("expr", "")
                if query_id not in queries:
                    errors.append(f"dashboard {dashboard.name}: target has unknown catalog_query_id {query_id!r}")
                elif expression != expand_catalog_query(queries[query_id]):
                    errors.append(f"dashboard {dashboard.name}: target {query_id} drifts from query catalog")
                expressions.append(expression)
        expressions = "\n".join(expressions)
        if not METRIC.search(expressions):
            errors.append(f"dashboard {dashboard.name}: no OctoRill metric target")
    if seen_uids != REQUIRED_UIDS:
        errors.append(f"dashboard UIDs must be {sorted(REQUIRED_UIDS)}")
    for required in (
        OBS / "docker-compose.yml",
        OBS / "prometheus" / "prometheus.yml",
        OBS / "prometheus" / "rules" / "octorill-recording.yml",
        OBS / "prometheus" / "rules" / "octorill-alerts.yml",
    ):
        if not required.exists():
            errors.append(f"missing observability asset {required}")
    if errors:
        for error in errors:
            print(f"error: {error}", file=sys.stderr)
        return 1
    print(f"observability assets valid: dashboards={len(seen_uids)} queries={len(queries)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
