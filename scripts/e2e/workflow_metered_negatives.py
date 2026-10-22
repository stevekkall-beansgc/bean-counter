#!/usr/bin/env python3
"""Finite quantity-correction refusals on private pre-correction fixture copies."""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_metered import MeteredJourney
from workflow_supported import canonical, sha, unique


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("use a new private output directory")
    args.output.mkdir(mode=0o700, parents=True)
    shutil.copytree(args.fixture / "installation", args.output / "installation", copy_function=shutil.copy2)
    shutil.copytree(args.fixture / "inputs", args.output / "inputs", copy_function=shutil.copy2)
    for pattern in ["*.request.json", "*.committed.json"]:
        for path in args.fixture.glob(pattern):
            shutil.copy2(path, args.output / path.name)
    args.resume, args.variant = True, "main"
    args.publication_helper = args.terms_helper = args.ledger
    journey = MeteredJourney(args)
    original = json.loads((args.fixture / "main-quantity-correct.request.json").read_bytes())
    journey.store, journey.scope, journey.source = (original["key"][k] for k in ["logical_store_id", "scope", "source"])
    copied = journey.snapshot()
    journey.installation = args.fixture / "installation"
    assert journey.snapshot() == copied
    journey.installation = args.output / "installation"
    journey.meta["fixture"] = str(args.fixture.resolve())
    journey.meta["original_quantity_request_sha256"] = sha(canonical(original))
    journey.save()

    def refuse(name, request, codes):
        before = journey.snapshot()
        path = journey.write(name + ".request.json", request)
        response = journey.workflow(name, "submit", path, expected=4 if codes == ["IDENTITY_CONFLICT"] else 3)
        assert response["code"] in codes, (name, response, codes)
        assert journey.snapshot() == before, name + " changed retained state"
        journey.meta["checks"].append({"check": name, "code": response["code"], "no_append": True})
        journey.save()

    def fresh(name):
        request = copy.deepcopy(original)
        request["key"]["operation_id"] = name
        return request

    request = fresh("stale-root")
    request["payload"]["expected_revision"] = "999"
    refuse("stale-root", request, ["STALE_REVISION"])
    request = fresh("stale-dependent")
    request["payload"]["dependent_reversals"][0]["expected_revision"] = "999"
    refuse("stale-dependent", request, ["STALE_REVISION", "INTEGRITY_FAILURE"])
    request = fresh("stale-family")
    request["payload"]["family_closures"][0]["expected_revision"] = "999"
    refuse("stale-family", request, ["INTEGRITY_FAILURE", "STALE_REVISION"])
    request = fresh("missing-dependent")
    request["payload"]["dependent_reversals"] = []
    refuse("missing-dependent", request, ["REVERSAL_DEPENDENTS_REQUIRED"])
    request = fresh("missing-family")
    request["payload"]["family_closures"] = []
    refuse("missing-family", request, ["REVERSAL_DEPENDENTS_REQUIRED"])
    request = fresh("quantity-beyond-original-bounds")
    request["payload"]["replacement_quantity"] = "4"
    refuse("quantity-beyond-original-bounds", request, ["INVALID_SCALAR"])
    request = fresh("wrong-subject-kind")
    request["payload"]["work_ref"] = original["payload"]["family_closures"][0]["family_state_ref"]
    refuse("wrong-subject-kind", request, ["INTEGRITY_FAILURE", "INVALID_REFERENCE"])
    request = fresh("wrong-source")
    request["key"]["source"] = "urn:example:unregistered-source"
    refuse("wrong-source", request, ["AUTHORITY_REQUIRED"])
    request = fresh("wrong-scope")
    request["key"]["scope"][0] = "different-tenant"
    refuse("wrong-scope", request, ["AUTHORITY_REQUIRED"])
    request = json.loads((args.fixture / "main-work.request.json").read_bytes())
    request["payload"]["facts"]["support:quantity"] = "2"
    refuse("changed-original-key", request, ["IDENTITY_CONFLICT"])

    # Both siblings start from exactly the same genuine pre-correction history.
    # Same saved bytes/identity succeeds unrestricted, then refuses after a real
    # monotone source restriction on the second sibling. No SQL seeding.
    control = args.output / "unrestricted-control"
    shutil.copytree(journey.installation, control, copy_function=shutil.copy2)
    restricted = journey.installation
    journey.installation = control
    assert journey.snapshot() == copied
    response = journey.submit("main-quantity-correct", "quantity.correct", original["payload"])
    assert response["receipt"]["original_request_bytes_sha256"] == sha((args.fixture / "main-quantity-correct.request.json").read_bytes())
    changed = copy.deepcopy(original)
    changed["payload"]["replacement_quantity"] = "1"
    refuse("changed-quantity-correction-key", changed, ["IDENTITY_CONFLICT"])
    (journey.root / "unrestricted-control-after.json").write_bytes(canonical(journey.snapshot()))
    journey.installation = restricted
    assert journey.snapshot() == copied
    enrollment = json.loads((args.fixture / "source-enroll.committed.json").read_bytes())
    enrollment_ref = unique(enrollment, "workflow-source-enrollment")
    source = json.loads((args.fixture / "source-enroll.request.json").read_bytes())
    ceiling = source["payload"]["ceiling"]
    ceiling["permissions"] = [v for v in ceiling["permissions"] if v != "correct"]
    ceiling["operations"] = [v for v in ceiling["operations"] if v != "quantity.correct"]
    journey.submit("restrict-quantity-right", "authority.change", {"schema": "ledger-workflow-source-restrict/1",
        "action": "source.restrict", "enrollment_ref": enrollment_ref, "expected_revision": "1", "replacement_ceiling": ceiling})
    (journey.root / "restricted-before.json").write_bytes(canonical(journey.snapshot()))
    refuse("restricted-quantity-consumer", original, ["AUTHORITY_REQUIRED", "INTEGRITY_FAILURE"])
    (journey.root / "restricted-after.json").write_bytes(canonical(journey.snapshot()))
    journey.reopen_retry(["main-work", "main-finalize", "main-success"])
    journey.meta["status"] = "finite_metered_refusals_complete"
    journey.save()


if __name__ == "__main__":
    main()
