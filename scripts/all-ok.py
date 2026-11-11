#!/usr/bin/env python3
"""The `ci-ok` job: read the `needs` context as JSON on stdin; fail unless every job succeeded or was skipped."""
import json
import sys

needs = json.load(sys.stdin)
for name, job in sorted(needs.items()):
    print(f"{name}: {job['result']}")
sys.exit(1 if any(j["result"] not in ("success", "skipped") for j in needs.values()) else 0)
