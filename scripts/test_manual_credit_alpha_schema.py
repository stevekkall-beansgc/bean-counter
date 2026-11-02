#!/usr/bin/env python3
"""Check the alpha request shape against retained synthetic DATA, not authority."""
import copy
import json
from pathlib import Path
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]
schema = json.loads((ROOT / "contracts/candidates/manual-credit-m5/alpha/requests.schema.json").read_text())
Draft202012Validator.check_schema(schema)
validator = Draft202012Validator(schema)
fixture = json.loads((ROOT / "contracts/candidates/manual-credit-m5/vectors/source-data.json").read_text())
credit = fixture["request"]
scope = credit["workflow_scope"]
status = {"schema": "ledger-billing-manual-credit-status/1", "customer": credit["customer"],
          "workflow_scope": scope, "adjustment_id": credit["adjustment_id"]}
read = {"schema": "ledger-billing-manual-credit-read/1", "customer": credit["customer"],
        "workflow_scope": scope, "credit_ref": fixture["test_credit_ref"]}
for value in (credit, status, read):
    validator.validate(value)
for key, value in [("atoms", "0"), ("atoms", "-0"), ("atoms", "-01"),
                   ("atoms", "-" + "1" * 31), ("currency", "EUR"), ("scale", 2)]:
    changed = copy.deepcopy(credit)
    changed["amount"][key] = value
    assert not validator.is_valid(changed), (key, value)
for changed in (dict(credit, unrequested_authority="operator"),
                dict(status, credit_ref=fixture["test_credit_ref"]),
                dict(read, adjustment_id="other"),
                dict(credit, source_action_refs=[])):
    assert not validator.is_valid(changed)
print("PASS: three synthetic request shapes and ten structural refusals; runtime authority and semantic checks are separate")
