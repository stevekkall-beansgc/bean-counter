#!/usr/bin/env python3
"""Finite accepted-context refusals on a genuine pre-acceptance private copy."""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_attested_negatives import fresh, refuse
from workflow_conditional import ConditionalJourney
from workflow_supported import sha, canonical


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True, help="completed matching conditional main")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("use a new private output directory")
    args.output.mkdir(mode=0o700, parents=True)
    fixture = args.fixture / "pre-acceptance-fixture"
    for name in ["installation", "inputs"]:
        shutil.copytree(fixture / name, args.output / name, copy_function=shutil.copy2)
    for pattern in ["*.request.json", "*.committed.json"]:
        for path in fixture.glob(pattern):
            shutil.copy2(path, args.output / path.name)
    options = argparse.Namespace(ledger=args.ledger, output=args.output, resume=True, context="matching",
        publication_helper=args.ledger, terms_helper=args.ledger)
    journey = ConditionalJourney(options)
    original = json.loads((fixture / "acceptance.request.json").read_bytes())
    journey.store, journey.scope, journey.source = (original["key"][k] for k in ["logical_store_id", "scope", "source"])
    copied = journey.snapshot()
    journey.installation = fixture / "installation"
    assert journey.snapshot() == copied, "private pre-acceptance copy differs"
    journey.installation = args.output / "installation"
    field = journey.business["conditional_context"]["field"]
    for name, facts, code in [
        ("missing-required-context", {}, "INVALID_SHAPE"),
        ("wrong-context-type", {field: 7}, "INVALID_SCALAR"),
        ("unknown-context-enum", {field: "undeclared"}, "INVALID_SCALAR"),
    ]:
        request = fresh(original, name)
        request["payload"]["binding_inputs"]["accepted_context"]["facts"] = facts
        refuse(journey, name, request, code)
    # The original genuine assent remains usable after every malformed request.
    result = journey.submit("acceptance-control", "agreement.accept", original["payload"])
    control = json.loads((journey.root / "acceptance-control.request.json").read_bytes())
    assert result["receipt"]["original_request_bytes_sha256"] == sha(canonical(control))
    changed = copy.deepcopy(control)
    changed["payload"]["binding_inputs"]["accepted_context"]["facts"][field] = journey.business["conditional_context"]["nonmatching_value"]
    refuse(journey, "changed-original-accepted-context", changed, "IDENTITY_CONFLICT")
    journey.reopen_retry(["acceptance-control"])
    journey.meta["status"] = "conditional_context_refusals_complete"
    journey.save()


if __name__ == "__main__":
    main()
