#!/usr/bin/env python3
"""Deterministic synthetic executor, not an OpenCode/Zen emulator or cost oracle."""
import json
import sys

source = sys.stdin.read()
prefix = "Products: "
suffix = ". Return their names as a sorted JSON array."
if not source.startswith(prefix) or not source.endswith(suffix):
    raise SystemExit("unsupported synthetic input")
print(json.dumps(sorted(source[len(prefix):-len(suffix)].split(", "))))
