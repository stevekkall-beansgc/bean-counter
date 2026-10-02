#!/usr/bin/env python3
"""Check that packaged Markdown links and local JSON schema refs resolve.

Remote links and same-file fragments are outside this offline path check.
The package verifier separately checks archive members and their exact bytes.
"""
import json
from pathlib import Path
import re
import sys
from urllib.parse import unquote, urlsplit


def schema_refs(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if key == "$ref" and isinstance(child, str):
                yield child
            else:
                yield from schema_refs(child)
    elif isinstance(value, list):
        for child in value:
            yield from schema_refs(child)


def main():
    if len(sys.argv) != 2:
        raise SystemExit("usage: check-package-docs.py PACKAGE_DIRECTORY")
    root = Path(sys.argv[1]).resolve(strict=True)
    failures = []
    checked = 0
    for path in sorted(root.rglob("*")):
        if not path.is_file():
            continue
        if path.suffix == ".md":
            refs = re.findall(r"\]\(([^)]+)\)", path.read_text(encoding="utf-8"))
        elif path.suffix == ".json":
            refs = list(schema_refs(json.loads(path.read_text(encoding="utf-8"))))
        else:
            continue
        for reference in refs:
            parsed = urlsplit(reference)
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            checked += 1
            target = (path.parent / unquote(parsed.path)).resolve()
            if not target.is_relative_to(root) or not target.is_file():
                failures.append({"file": str(path.relative_to(root)), "reference": reference})
    print(json.dumps({"checked_paths": checked, "missing_or_external_paths": failures}, sort_keys=True))
    raise SystemExit(bool(failures))


if __name__ == "__main__":
    main()
