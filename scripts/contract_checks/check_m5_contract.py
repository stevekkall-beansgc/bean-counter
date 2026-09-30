"""Offline Draft 2020-12 and exact-golden audit for the M5 billing candidate."""

from __future__ import annotations

import base64
import copy
import csv
import hashlib
import io
import json
import sys
from pathlib import Path
from urllib.parse import unquote, urldefrag

import jsonschema
from referencing import Registry, Resource
from referencing.exceptions import Unresolvable


ROOT = Path(__file__).resolve().parents[2]
CANDIDATE = ROOT / "contracts/candidates/billing-lifecycle-m5"
SCHEMAS = CANDIDATE / "schemas"
VECTORS = CANDIDATE / "vectors"
V2_RECORDS = ROOT / "contracts/candidates/v2/schemas/canonical-records.schema.json"
SAFE_INTEGER = 9_007_199_254_740_991


def strict_json(raw: bytes):
    if raw.startswith(b"\xef\xbb\xbf"):
        raise ValueError("BOM")

    def pairs(items):
        value = {}
        for key, item in items:
            if key in value:
                raise ValueError(f"duplicate key: {key}")
            value[key] = item
        return value

    def integer(token):
        value = int(token)
        if token == "-0" or abs(value) > SAFE_INTEGER:
            raise ValueError("non-canonical JSON integer")
        return value

    def invalid_number(_token):
        raise ValueError("unsupported JSON number")

    value = json.loads(
        raw.decode("utf-8"),
        object_pairs_hook=pairs,
        parse_int=integer,
        parse_float=invalid_number,
        parse_constant=invalid_number,
    )
    canonical(value)
    return value


def canonical(value, depth=0):
    if depth > 32:
        raise ValueError("JSON nesting exceeds 32")
    if isinstance(value, dict):
        keys = sorted(value, key=lambda key: key.encode("utf-16-be"))
        return b"{" + b",".join(
            canonical(key, depth + 1) + b":" + canonical(value[key], depth + 1)
            for key in keys
        ) + b"}"
    if isinstance(value, list):
        return b"[" + b",".join(canonical(item, depth + 1) for item in value) + b"]"
    if value is None or not isinstance(value, (str, int, bool)):
        raise ValueError("unsupported JSON scalar")
    if isinstance(value, int) and not isinstance(value, bool) and abs(value) > SAFE_INTEGER:
        raise ValueError("unsafe JSON integer")
    return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")


def load(path: Path):
    return strict_json(path.read_bytes())


def pointer(document, fragment, label):
    if not fragment:
        return document
    if not fragment.startswith("/"):
        raise AssertionError(f"{label}: only JSON Pointer fragments are allowed")
    value = document
    for raw in fragment[1:].split("/"):
        token = unquote(raw).replace("~1", "/").replace("~0", "~")
        if isinstance(value, list):
            value = value[int(token)]
        else:
            value = value[token]
    return value


def iter_refs(value):
    if isinstance(value, dict):
        if "$ref" in value:
            yield value["$ref"]
        for child in value.values():
            yield from iter_refs(child)
    elif isinstance(value, list):
        for child in value:
            yield from iter_refs(child)


def audit_local_refs(schema_paths, documents):
    count = 0
    allowed_root = (ROOT / "contracts/candidates").resolve()
    for path in schema_paths:
        for ref in iter_refs(documents[path]):
            count += 1
            target_name, fragment = urldefrag(ref)
            if "://" in target_name or target_name.startswith("urn:"):
                raise AssertionError(f"{path.name}: non-local $ref {ref}")
            target = path if not target_name else (path.parent / target_name).resolve()
            if target != path.resolve() and allowed_root not in target.parents:
                raise AssertionError(f"{path.name}: $ref escapes candidate contracts: {ref}")
            if target not in documents:
                documents[target] = load(target)
            try:
                pointer(documents[target], fragment, f"{path.name}: {ref}")
            except (KeyError, IndexError, ValueError) as error:
                raise AssertionError(f"{path.name}: unresolved $ref {ref}") from error
    assert count == 444, f"M5 local $ref inventory changed: {count}"
    return count


def registry_for(schema_paths, documents):
    resources = []
    for path in [*schema_paths, V2_RECORDS]:
        schema = documents[path.resolve()]
        resource = Resource.from_contents(schema)
        aliases = {path.resolve().as_uri(), path.name, schema.get("$id")}
        if path == V2_RECORDS:
            aliases.add("../../v2/schemas/canonical-records.schema.json")
        for alias in aliases:
            if alias:
                resources.append((alias, resource))
    return Registry().with_resources(resources)


def schema_families(schema):
    result = set()
    for branch in schema["oneOf"]:
        ref = branch["$ref"]
        definition = pointer(schema, ref[1:], ref)
        const = definition.get("properties", {}).get("schema", {}).get("const")
        if const:
            result.add(const)
    return result


def validate(validator, value, label):
    errors = sorted(validator.iter_errors(value), key=lambda error: list(error.absolute_path))
    if errors:
        error = errors[0]
        location = "/".join(str(item) for item in error.absolute_path) or "<root>"
        raise AssertionError(f"{label}: {location}: {error.message}")


def assert_rejected(validator, value, label):
    if not list(validator.iter_errors(value)):
        raise AssertionError(f"{label}: invalid probe was accepted")


def digest(prefix: bytes, raw: bytes):
    return hashlib.sha256(prefix + raw).hexdigest()


def assert_canonical(value, text, label):
    raw = text.encode("utf-8")
    assert strict_json(raw) == value, f"{label}: canonical text parses to another value"
    assert canonical(value) == raw, f"{label}: non-canonical UTF-8"
    return raw


def audit_command_goldens(validators):
    golden = load(VECTORS / "m5-command-goldens.json")
    request_count = result_count = child_count = 0
    seen_cases = set()
    for case in golden["cases"]:
        assert case["id"] not in seen_cases, f"duplicate command case: {case['id']}"
        seen_cases.add(case["id"])
        commands = [case["command"]] if "command" in case else case.get("commands", [])
        for index, command in enumerate(commands):
            label = f"{case['id']}/command/{index}"
            validate(validators["requests"], command["request"], label + "/request")
            validate(validators["results"], command["result"], label + "/result")
            validate(validators["records"], command["command_record"], label + "/command_record")
            request_count += 1
            result_count += 1

            request = assert_canonical(
                command["request"], command["request_canonical_utf8"], label + "/request"
            )
            response = assert_canonical(
                command["result"], command["result_canonical_utf8"], label + "/result"
            )
            identity = assert_canonical(
                command["identity"], command["identity_key_canonical_utf8"], label + "/identity"
            )
            request_hash = digest(b"bean-counter/m5/request/1\0", request)
            response_hash = digest(b"bean-counter/m5/response/1\0", response)
            assert command["request_hash"] == request_hash, f"{label}: request hash"
            assert command["response_hash_storage_only"] == response_hash, f"{label}: response hash"

            record = command["command_record"]
            assert record["identity"] == command["identity"], f"{label}: command identity"
            assert record["request_hash"] == request_hash, f"{label}: stored request hash"
            assert record["response_hash"] == response_hash, f"{label}: stored response hash"
            assert base64.b64decode(record["identity_key_base64"], validate=True) == identity
            assert base64.b64decode(record["request_bytes_base64"], validate=True) == request
            assert base64.b64decode(record["response_bytes_base64"], validate=True) == response

            children = command["domain_children"]
            assert int(record["child_count"]) == len(children), f"{label}: child count"
            assert record["child_record_ids"] == [child["record_id"] for child in children]
            prior_key = None
            for child_index, child in enumerate(children):
                child_label = f"{label}/child/{child_index}"
                validate(validators["records"], child["payload"], child_label + "/payload")
                child_count += 1
                assert child["family"] == child["payload"]["schema"], f"{child_label}: family"
                assert child["record_id"] == child["payload"]["record"]["record_id"]
                assert child["sequence"] == child["payload"]["record"]["sequence"]
                child_key = assert_canonical(
                    child["child_key"], child["child_key_canonical_utf8"], child_label + "/key"
                )
                assert prior_key is None or prior_key < child_key, f"{label}: child key order"
                prior_key = child_key
                expected_id = "m5r_" + digest(
                    b"bean-counter/m5/record-id/1\0", identity + b"\0" + child_key
                )
                assert child["record_id"] == expected_id, f"{child_label}: record id"
                payload = assert_canonical(
                    child["payload"], child["payload_canonical_utf8"], child_label + "/payload"
                )
                assert payload
                unhashed = copy.deepcopy(child["payload"])
                unhashed["record"].pop("payload_hash")
                payload_hash = digest(
                    b"bean-counter/m5/record/1\0" + child["family"].encode() + b"\0",
                    canonical(unhashed),
                )
                assert child["payload_hash"] == payload_hash, f"{child_label}: payload hash"
                assert child["payload"]["record"]["payload_hash"] == payload_hash

        for alias_index, alias in enumerate(case.get("activity_semantic_aliases", [])):
            validate(
                validators["requests"],
                alias["request"],
                f"{case['id']}/activity_semantic_alias/{alias_index}/request",
            )

    assert len(seen_cases) == 12, f"M5 command case inventory changed: {len(seen_cases)}"
    assert request_count == result_count == 22, "M5 command inventory changed"
    assert child_count == 33, f"M5 child inventory changed: {child_count}"
    return request_count, result_count, child_count


def audit_finance_vectors(validators):
    golden = load(VECTORS / "finance-export-v4.json")
    assert len(golden["header"]) == 27, "finance export header width"
    for case in golden["cases"]:
        label = f"finance/{case['id']}"
        statement = case["statement"]
        validate(validators["results"], statement, label + "/statement")
        summary = {
            "schema": "ledger-finance-export/4",
            "status": "exported",
            "complete": True,
            "export_id": case["export_id"],
            "statement_hash": statement["statement_hash"],
            "snapshot_boundary_id": statement["snapshot_boundary_id"],
            "m3_high_water": statement["m3_high_water"],
            "m5_high_water": statement["m5_high_water"],
            "posting_count": str(len(statement["lines"])),
            "control_net_atoms": statement["net_atoms"],
            "currency": statement["currency"],
            "scale": statement["scale"],
            "account_mapping": case["accounts"],
            "delivered": False,
            "payment_collected": False,
        }
        validate(validators["finance-export"], summary, label + "/summary")
        raw = case["csv_utf8"].encode("utf-8")
        assert b"\r\n" in raw and raw.endswith(b"\r\n"), f"{label}: CRLF framing"
        assert raw.replace(b"\r\n", b"").find(b"\n") == -1, f"{label}: bare LF"
        checksum = hashlib.sha256(raw).hexdigest()
        assert case["csv_sha256"] == checksum, f"{label}: CSV checksum"
        if "expected_csv_sha256" in case:
            assert case["expected_csv_sha256"] == checksum, f"{label}: expected checksum"
        rows = list(csv.reader(io.StringIO(case["csv_utf8"], newline="")))
        assert rows[0] == golden["header"], f"{label}: header"
        assert all(len(row) == 27 for row in rows), f"{label}: row width"
        assert rows[-1] == case["trailer_fields"], f"{label}: trailer"
        if "posting_fields" in case:
            assert rows[1:-1] == case["posting_fields"], f"{label}: posting rows"
        assert len(rows[1:-1]) == len(statement["lines"]), f"{label}: posting count"
        if "line_count" in case:
            assert case["line_count"] == len(statement["lines"]), f"{label}: line count"
        if "control_net_atoms" in case:
            assert case["control_net_atoms"] == statement["net_atoms"], f"{label}: net"
    return len(golden["cases"])


def walk_schema_values(value, request_families, result_families):
    if isinstance(value, dict):
        family = value.get("schema")
        if family in request_families:
            yield "requests", value
        elif family in result_families:
            yield "results", value
        for child in value.values():
            yield from walk_schema_values(child, request_families, result_families)
    elif isinstance(value, list):
        for child in value:
            yield from walk_schema_values(child, request_families, result_families)


def audit_oracle_declared_values(validators, schemas, registry):
    oracle = load(VECTORS / "m5-oracle.json")
    request_families = schema_families(schemas["requests"])
    result_families = schema_families(schemas["results"])
    counts = {"requests": 0, "results": 0}
    for kind, value in walk_schema_values(oracle, request_families, result_families):
        validate(validators[kind], value, f"oracle/{value['schema']}/{counts[kind]}")
        counts[kind] += 1

    receipt_schema = {
        "$schema": schemas["results"]["$schema"],
        "$defs": schemas["results"]["$defs"],
        "$ref": "#/$defs/m4_acceptance_receipt",
    }
    receipt_validator = jsonschema.Draft202012Validator(
        receipt_schema,
        registry=registry,
        format_checker=jsonschema.FormatChecker(),
    )
    receipts = []

    def find_receipts(value):
        if isinstance(value, dict):
            if value.get("kind") == "base-acceptance" and {"body", "scope", "id"} <= value.keys():
                receipts.append(value)
            for child in value.values():
                find_receipts(child)
        elif isinstance(value, list):
            for child in value:
                find_receipts(child)

    find_receipts(oracle)
    for index, receipt in enumerate(receipts):
        validate(receipt_validator, receipt, f"oracle/m4_receipt/{index}")
    assert counts["requests"] >= 10, f"too few oracle requests: {counts['requests']}"
    assert counts["results"] >= 1, f"too few oracle results: {counts['results']}"
    assert receipts, "no declared M4 receipt vectors"
    return counts, len(receipts)


def audit_negative_schema_probes(validators, schemas, registry):
    golden = load(VECTORS / "m5-command-goldens.json")
    command = golden["cases"][0]["command"]
    probes = []
    for kind, value in [
        ("requests", command["request"]),
        ("results", command["result"]),
        ("records", command["command_record"]),
        ("records", command["domain_children"][0]["payload"]),
    ]:
        invalid = copy.deepcopy(value)
        invalid["undeclared"] = True
        probes.append((validators[kind], invalid, f"negative/{kind}/additional-property"))

    receipt_schema = {
        "$schema": schemas["results"]["$schema"],
        "$defs": schemas["results"]["$defs"],
        "$ref": "#/$defs/m4_acceptance_receipt",
    }
    receipt_validator = jsonschema.Draft202012Validator(
        receipt_schema, registry=registry, format_checker=jsonschema.FormatChecker()
    )
    receipt = copy.deepcopy(golden["cases"][3]["m4_acceptance_receipt"])
    receipt["command_sequence"] = "3"
    probes.append((receipt_validator, receipt, "negative/m4-receipt/command-sequence"))
    for validator, value, label in probes:
        assert_rejected(validator, value, label)
    return len(probes)


def main():
    schema_paths = sorted(path.resolve() for path in SCHEMAS.glob("*.schema.json"))
    documents = {path: load(path) for path in schema_paths}
    documents[V2_RECORDS.resolve()] = load(V2_RECORDS)
    for path in schema_paths:
        schema = documents[path]
        assert schema.get("$schema") == "https://json-schema.org/draft/2020-12/schema"
        jsonschema.Draft202012Validator.check_schema(schema)
    ref_count = audit_local_refs(schema_paths, documents)
    registry = registry_for(schema_paths, documents)
    schemas = {path.name.removesuffix(".schema.json"): documents[path] for path in schema_paths}
    validators = {
        name: jsonschema.Draft202012Validator(
            schema, registry=registry, format_checker=jsonschema.FormatChecker()
        )
        for name, schema in schemas.items()
    }
    requests, results, children = audit_command_goldens(validators)
    finance = audit_finance_vectors(validators)
    oracle, receipts = audit_oracle_declared_values(validators, schemas, registry)
    negatives = audit_negative_schema_probes(validators, schemas, registry)
    print(
        "M5 contract gate: passed "
        f"(4 schemas, {ref_count} local refs, {requests} command requests, "
        f"{results} command results, {children} children, {finance} finance vectors, "
        f"{oracle['requests']} oracle requests, {oracle['results']} oracle results, "
        f"{receipts} M4 receipts, {negatives} rejection probes)"
    )


if __name__ == "__main__":
    try:
        main()
    except (
        AssertionError,
        Unresolvable,
        ValueError,
        jsonschema.SchemaError,
        jsonschema.ValidationError,
    ) as error:
        print(f"M5 contract gate: FAILED: {error}", file=sys.stderr)
        raise SystemExit(1) from error
