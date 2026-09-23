#!/usr/bin/env python3
"""Read-only verification of the additive R3 freeze and preserved older registries."""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PREFIX = "contracts/candidates/central-adjudication-r3-candidate1/"
FREEZE = ROOT / "contracts/freezes/central-adjudication-r3-1.json"


def require(condition: bool, message: object) -> None:
    if not condition:
        raise ValueError(message)


def verify() -> dict:
    manifest = json.loads(FREEZE.read_bytes())
    require(manifest["schema"] == "ledger-phase4-contract-freeze/1", "freeze schema")
    require(manifest["profile"] == "central-adjudication-r3/1", "freeze profile")
    require(manifest["status"] == "frozen-unreleased", "freeze status")
    require(manifest["reviewed_commit"] == "c8fbddc682e22e10c6122abbcf4c8212a1c407fa", "reviewed commit")
    require(manifest["reviewed_tree"] == "96c0dc5d60cc48c5a3e559e6d2d7e4692ecc01d2", "reviewed tree")
    groups = ("candidate_files", "controls", "preserved_registries")
    for group in groups:
        for name, expected in manifest[group].items():
            path = Path(name)
            require(not path.is_absolute() and ".." not in path.parts, name)
            raw = (ROOT / path).read_bytes()
            require(len(raw) == expected["bytes"], (group, name, "length"))
            require(hashlib.sha256(raw).hexdigest() == expected["sha256"], (group, name, "hash"))
    frozen = manifest["candidate_files"]
    require(len(frozen) == 91 and all(name.startswith(PREFIX) for name in frozen), "candidate paths")
    actual = set(subprocess.check_output(
        ["git", "ls-files", "-co", "--exclude-standard", "--", PREFIX],
        cwd=ROOT, text=True,
    ).splitlines())
    require(actual == set(frozen), "closed candidate file set")
    inventory_name = PREFIX + "ARTIFACTS.json"
    require(frozen[inventory_name]["sha256"] == "301766b5f458ccaf175e90f9f962daa7fa6fa1cf144e19f1526003d9229f78be", "accepted inventory")
    inventory = json.loads((ROOT / inventory_name).read_bytes())
    entries = {PREFIX + entry["path"]: {key: entry[key] for key in ("bytes", "sha256")}
               for entry in inventory["files"]}
    require(len(entries) == len(inventory["files"]) == 90, "inventory membership")
    require(entries == {name: pin for name, pin in frozen.items() if name != inventory_name}, "inventory pins")
    return {"status": "PASS", "candidate_files": len(frozen),
            "controls": len(manifest["controls"]),
            "preserved_registries": len(manifest["preserved_registries"]),
            "runtime_conformance": "NOT_CLAIMED"}


if __name__ == "__main__":
    print(json.dumps(verify(), sort_keys=True))
