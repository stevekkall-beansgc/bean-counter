#!/usr/bin/env python3
"""Finite attested Work/correction refusals on genuine private fixture copies."""
import argparse
import copy
import json
import shutil
from pathlib import Path

from workflow_attested import AttestedJourney
from workflow_supported import canonical, sha, unique


def private_copy(args, source, name):
    output = args.output / name
    output.mkdir(mode=0o700)
    shutil.copytree(source / "installation", output / "installation", copy_function=shutil.copy2)
    shutil.copytree(source / "inputs", output / "inputs", copy_function=shutil.copy2)
    for pattern in ["*.request.json", "*.committed.json"]:
        for path in source.glob(pattern):
            shutil.copy2(path, output / path.name)
    options = argparse.Namespace(ledger=args.ledger, output=output, resume=True,
                                publication_helper=args.ledger, terms_helper=args.ledger)
    journey = AttestedJourney(options)
    original = json.loads((source / "main-work.request.json").read_bytes())
    journey.store, journey.scope, journey.source = (original["key"][k] for k in ["logical_store_id", "scope", "source"])
    copied = journey.snapshot()
    journey.installation = source / "installation"
    assert journey.snapshot() == copied, "private copy differs from genuine fixture"
    journey.installation = output / "installation"
    journey.meta["fixture"] = str(source.resolve())
    journey.save()
    return journey


def refuse(journey, name, request, code):
    before = journey.snapshot()
    (journey.root / (name + ".before.json")).write_bytes(canonical(before))
    try:
        response = journey.workflow(name, "submit", journey.write(name + ".request.json", request),
                                    expected=4 if code == "IDENTITY_CONFLICT" else 3)
    finally:
        after = journey.snapshot()
        (journey.root / (name + ".after.json")).write_bytes(canonical(after))
    assert after == before, name + " changed retained state"
    assert response["code"] == code, (name, response, code)
    journey.meta["checks"].append({"check": name, "code": code, "no_append": True})
    journey.save()


def fresh(original, name):
    request = copy.deepcopy(original)
    request["key"]["operation_id"] = name
    return request


def restrict(journey, name, operation, permission=None):
    original = json.loads((journey.root / "source-enroll.request.json").read_bytes())
    ceiling = copy.deepcopy(original["payload"]["ceiling"])
    ceiling["operations"] = [v for v in ceiling["operations"] if v not in ["agreement.revoke", operation]]
    if permission:
        ceiling["permissions"].remove(permission)
    enrollment = unique(json.loads((journey.root / "source-enroll.committed.json").read_bytes()), "workflow-source-enrollment")
    current = json.loads((journey.root / "main-proof-source-narrow.committed.json").read_bytes())
    revision = journey.document(unique(current, "workflow-source-state"))["revision"]
    return journey.submit(name, "authority.change", {"schema": "ledger-workflow-source-restrict/1",
        "action": "source.restrict", "enrollment_ref": enrollment, "expected_revision": revision,
        "replacement_ceiling": ceiling})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True, help="completed attested main directory")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.output.exists():
        parser.error("use a new private output directory")
    args.output.mkdir(mode=0o700, parents=True)
    work = private_copy(args, args.fixture / "pre-work-fixture", "fresh-work")
    original = json.loads((work.root / "main-work.request.json").read_bytes())
    for name in ["missing-proof", "duplicate-proof", "unmatched-proof-field", "wrong-proof-value", "wrong-proof-business"]:
        request = fresh(original, name)
        payload = request["payload"]
        if name == "missing-proof":
            payload["fact_proofs"] = []
        elif name == "duplicate-proof":
            payload["fact_proofs"] *= 2
        elif name == "unmatched-proof-field":
            payload["fact_proofs"][0]["field"] = "support:unmatched"
        elif name == "wrong-proof-value":
            payload["facts"]["support:quantity"] = "2"
        else:
            payload["business_operation_id"] = "another-business-operation"
        refuse(work, name, request, "INTEGRITY_FAILURE")
    # The original request remains valid after all proof refusals.
    result = work.submit("main-work", "work.accept", original["payload"])
    assert result["receipt"]["original_request_bytes_sha256"] == sha(canonical(original))
    changed = copy.deepcopy(original)
    changed["payload"]["fact_proofs"] = []
    refuse(work, "changed-original-work-proof", changed, "IDENTITY_CONFLICT")
    work.reopen_retry(["main-quantity-proof", "main-proof-source-narrow", "main-work"])
    denied_work = private_copy(args, args.fixture / "pre-work-fixture", "restricted-work")
    restrict(denied_work, "restrict-work-right", "work.accept")
    refuse(denied_work, "restricted-work-consumer", original, "INTEGRITY_FAILURE")

    correction = private_copy(args, args.fixture / "pre-quantity-fixture", "correction")
    original = json.loads((correction.root / "main-quantity-correct.request.json").read_bytes())
    original_proof = json.loads((correction.root / "main-work.request.json").read_bytes())["payload"]["fact_proofs"]
    for name in ["missing-replacement-proof", "duplicate-replacement-proof", "original-proof-for-replacement", "unmatched-replacement-field"]:
        request = fresh(original, name)
        if name == "missing-replacement-proof":
            request["payload"]["fact_proofs"] = []
        elif name == "duplicate-replacement-proof":
            request["payload"]["fact_proofs"] *= 2
        elif name == "original-proof-for-replacement":
            request["payload"]["fact_proofs"] = original_proof
        else:
            request["payload"]["fact_proofs"][0]["field"] = "support:unmatched"
        refuse(correction, name, request, "INTEGRITY_FAILURE")
    result = correction.submit("main-quantity-correct", "quantity.correct", original["payload"])
    assert result["receipt"]["original_request_bytes_sha256"] == sha(canonical(original))
    changed = copy.deepcopy(original)
    changed["payload"]["fact_proofs"] = original_proof
    refuse(correction, "changed-original-correction-proof", changed, "IDENTITY_CONFLICT")
    correction.reopen_retry(["main-work", "main-quantity-correct"])
    denied = private_copy(args, args.fixture / "pre-quantity-fixture", "restricted-correction")
    restrict(denied, "restrict-quantity-right", "quantity.correct", "correct")
    refuse(denied, "restricted-quantity-consumer", original, "INTEGRITY_FAILURE")
    denied.reopen_retry(["main-quantity-proof", "main-work", "main-finalize", "main-success"])
    (args.output / "RESULTS.json").write_bytes(canonical({"status": "finite_attested_refusals_complete",
        "directories": ["fresh-work", "restricted-work", "correction", "restricted-correction"]}))


if __name__ == "__main__":
    main()
