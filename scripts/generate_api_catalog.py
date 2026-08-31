#!/usr/bin/env python3
"""Generate an API route catalog from a checked-out 1Panel source tree.

The official Swagger document is intentionally supplemented with router source,
because some runtime routes are not represented in the generated Swagger file.
"""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

GROUP_RE = re.compile(r'(?P<name>\w+)\s*:=\s*(?P<parent>\w+)\.Group\("(?P<path>[^"]*)"\)')
ROUTE_RE = re.compile(
    r'(?P<group>\w+)\.(?P<method>GET|POST|PUT|DELETE|PATCH)\("(?P<path>[^"]*)"\s*,\s*(?P<handler>[\w.]+)'
)


def join_path(*parts: str) -> str:
    cleaned = [part.strip("/") for part in parts if part.strip("/")]
    return "/" + "/".join(cleaned)


def swagger_metadata(swagger: dict, route_path: str, method: str) -> dict:
    swagger_path = route_path.removeprefix("/api/v2") or "/"
    operation = swagger.get("paths", {}).get(swagger_path, {}).get(method.lower(), {})
    return {
        "summary": operation.get("summary", ""),
        "tags": operation.get("tags", []),
        "swaggerDocumented": bool(operation),
    }


def extract_router(path: Path, prefix: str, swagger: dict, source_root: Path) -> list[dict]:
    text = path.read_text(encoding="utf-8")
    groups = {"Router": ""}
    pending = list(GROUP_RE.finditer(text))
    while pending:
        remaining = []
        progressed = False
        for match in pending:
            parent = match.group("parent")
            if parent not in groups:
                remaining.append(match)
                continue
            groups[match.group("name")] = join_path(groups[parent], match.group("path"))
            progressed = True
        if not progressed:
            break
        pending = remaining

    routes = []
    for match in ROUTE_RE.finditer(text):
        group = match.group("group")
        if group not in groups:
            continue
        route_path = join_path(prefix, groups[group], match.group("path"))
        method = match.group("method")
        route = {
            "method": method,
            "path": route_path,
            "handler": match.group("handler").removeprefix("baseApi."),
            "source": str(path.relative_to(source_root)),
        }
        route.update(swagger_metadata(swagger, route_path, method))
        routes.append(route)
    return routes


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True, help="1Panel source checkout")
    parser.add_argument("--version", default="dev-v2")
    parser.add_argument("--output", type=Path, default=Path("references/api-dev-v2.json"))
    args = parser.parse_args()

    source = args.source.resolve()
    swagger_path = source / "core/cmd/server/docs/swagger.json"
    swagger = json.loads(swagger_path.read_text(encoding="utf-8"))

    routes = []
    for router_dir, prefix in (
        (source / "agent/router", "/api/v2"),
        (source / "core/router", "/api/v2/core"),
    ):
        for path in sorted(router_dir.glob("*.go")):
            routes.extend(extract_router(path, prefix, swagger, source))

    unique = {(route["method"], route["path"]): route for route in routes}
    result = {
        "panelVersion": args.version,
        "sourceRef": args.version,
        "routeCount": len(unique),
        "swaggerDocumentedCount": sum(route["swaggerDocumented"] for route in unique.values()),
        "routes": sorted(unique.values(), key=lambda route: (route["path"], route["method"])),
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {len(unique)} routes to {args.output}")


if __name__ == "__main__":
    main()
