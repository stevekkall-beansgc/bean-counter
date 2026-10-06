#!/usr/bin/env python3
"""Offline bounded-cohort demonstration against the released v0.9.4 CLI.

Synthetic terms and evidence only. This is a fresh-trial example, not a resumable
outbox or a production outcome SDK. Preserve its directory after any failure.
"""
import argparse
from datetime import datetime, timedelta, timezone
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time

FIXTURES = Path(__file__).parent / "bounded-cohort"
CUSTOMER, SOURCE = "synthetic-customer", "urn:example:product"
SCOPE = ["example-company", "local"]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def now():
    return datetime.now(timezone.utc)


def stamp(value):
    return value.isoformat(timespec="microseconds").replace("+00:00", "Z")


def encode(value):
    return (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()


def save(path, value):
    data = value if isinstance(value, bytes) else encode(value)
    with path.open("xb") as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def physical(path):
    path = path.absolute()
    require(path == path.resolve(), f"Use a physical path (no symlink components): {path}")
    return path


def wait_until(boundary):
    while True:
        remaining = (boundary - now()).total_seconds()
        if remaining <= 0:
            return
        time.sleep(min(0.2, remaining))


class Trial:
    def __init__(self, ledger, root):
        self.ledger, self.root = str(ledger), root
        self.store = root / "store"
        self.sequence = 0

    def call(self, label, args, expected=0):
        self.sequence += 1
        prefix = self.root / f"{self.sequence:02d}-{label}"
        argv = [self.ledger, *map(str, args)]
        save(prefix.with_suffix(".command.json"), argv)
        result = subprocess.run(argv, capture_output=True)
        save(prefix.with_suffix(".stdout"), result.stdout)
        save(prefix.with_suffix(".stderr"), result.stderr)
        save(prefix.with_suffix(".exit.json"), {"exit_code": result.returncode})
        require(result.returncode == expected,
                f"{label}: exit {result.returncode}, expected {expected}; preserve {self.root}")
        return json.loads(result.stdout)

    def billing(self, label, args, expected=0):
        return self.call(label, ["billing", "--directory", self.store, *args, "--json"], expected)

    def history(self, label, target=None):
        args = ["explain", "--customer", CUSTOMER, target] if target else ["statement", "--customer", CUSTOMER]
        history = self.billing(label, args)
        require(history.get("status") == "ok" and history.get("complete") is True
                and history.get("schema") == "ledger-billing-statement/2"
                and history.get("customer") == CUSTOMER and history.get("scope") == SCOPE
                and history.get("currency") == "USD" and history.get("scale") == 2,
                "Incomplete, unknown or wrong-scope history; do not acknowledge")
        return history

    def verify(self, operation, request, receipt, target, base_receipt, label):
        kind = "base-acceptance" if operation == "accept" else "receipt"
        require(receipt.get("kind") == kind and isinstance(receipt.get("id"), str)
                and receipt["id"] and receipt.get("scope") == SCOPE, "Wrong receipt kind/identity/scope")
        if operation == "accept":
            require(receipt["body"].get("target") == target, "Wrong base target")
        else:
            require("target" not in receipt["body"], "Unexpected adjustment receipt target")
        history = self.history(label, target)
        matches = [e for e in history["entries"] if e.get("target") == target
                   and e.get("source") == SOURCE and e.get("receipt") == receipt]
        require(len(matches) == 1, "Missing or ambiguous exact retained receipt")
        entry = matches[0]
        require(entry.get("agreement_id") == "synthetic-agreement"
                and entry.get("agreement_version") == "1", "Wrong agreement/version")
        require(sum(e.get("receipt") == base_receipt and e.get("target") == target
                    and e.get("source") == SOURCE for e in history["entries"]) == 1,
                "Original base receipt missing")
        records = entry["records"]
        events = [r for r in records if r["kind"] == "event"]
        require(len(events) == 1, "Missing or ambiguous event")
        event, data = events[0], events[0]["body"]["data"]
        common = {"source": SOURCE, "external_id": request["id"], "occurred_at": request["occurred_at"]}
        if operation == "accept":
            require(event["id"] == target, "Base event/target mismatch")
            common.update(type="base", customer=CUSTOMER, operation_id=request["operation_id"],
                          work_type="content.generated", status="succeeded", quantity="1", unit="call", evidence=[])
        else:
            common.update(type="outcome", target=target, family_id=request["family"], code=request["code"])
            require(event["id"] == receipt["body"].get("event_id"), "Receipt event mismatch")
            evidence = [r for r in records if r["kind"] == "evidence" and r["id"] in data["evidence"]]
            require(len(evidence) == 1, "Missing or ambiguous outcome evidence")
            require(json.loads(evidence[0]["body"]["utf8"]) == {
                "scope": SCOPE, "source": SOURCE, "target": target, "family": request["family"],
                "occurred_at": request["occurred_at"], "retained_evidence": request["evidence"]},
                "Retained outcome facts differ")
        require(all(data.get(k) == v for k, v in common.items()), "Retained event facts differ")
        expected_atoms = 2 if operation == "accept" or request["code"] == "success" else 0
        postings = entry["postings"]
        require(all(p["body"]["amount"]["currency"] == "USD" and p["body"]["amount"]["scale"] == 2
                    for p in postings), "Wrong posting money")
        require(sum(int(p["body"]["amount"]["atoms"]) for p in postings) == expected_atoms
                and int(entry["net_atoms"]) == expected_atoms, "Posting/entry totals differ")
        # Every receipt member/reference must be retained with its exact hash.
        if operation == "accept":
            refs = receipt["body"]["members"]
        else:
            manifests = [r for r in records if r["kind"] == "decision-manifest"
                         and r["id"] == receipt["body"].get("decision_id")]
            require(len(manifests) == 1, "Missing or ambiguous receipt decision manifest")
            refs = manifests[0]["body"]["members"]
        retained = {}
        for history_entry in history["entries"]:
            for record in [*history_entry["records"], history_entry["receipt"]]:
                key = json.dumps([record["kind"], record["id"]], sort_keys=True)
                require(key not in retained or retained[key] == record, "Inconsistent repeated retained record")
                retained[key] = record
        require(all(retained.get(json.dumps([ref["kind"], ref["id"]], sort_keys=True), {}).get("content_hash")
                    == ref["content_hash"] for ref in refs), "Receipt membership missing or mismatched")

    def submit(self, operation, path):
        response = self.billing(path.stem, [operation, "--customer", CUSTOMER, "--source", SOURCE, path])
        require(response.get("status") == "accepted", "Fresh request not accepted")
        return response["receipt"]


def cohort(trial):
    created = now()
    start = created + timedelta(seconds=12)
    setup = json.loads((FIXTURES.parent / "setup-synthetic.json").read_bytes())
    setup["accepted_at"] = stamp(created)
    setup["assent_evidence"] = (
        "SYNTHETIC ONLY. Base USD 0.02 per completed local report. The agreed outcome is a NEW "
        "assessment after ordinary start: generated_total == sum(values) earns +2 scale-2 USD atoms "
        "(success); a valid report failing that criterion earns 0 (criteria-not-met). "
        "Unknown/unparseable reports remain pending. No cutoff condition or real assent.")
    family = setup["outcome_policy"]["families"][0]
    for name, offset in (("ordinary", 0), ("corrections", 2)):
        endpoints = [start + timedelta(seconds=offset + n) for n in (0, 60, 75, 90)]
        require(endpoints[0] < endpoints[1] <= endpoints[2] <= endpoints[3], "Invalid windows")
        family[name] = dict(zip(("starts_at", "occurs_before", "received_by", "accepted_by"), map(stamp, endpoints)))
    family["codes"] = [{"code": code, "amount": {"kind": "fixed", "money": {
        "currency": "USD", "scale": 2, "atoms": atoms}}} for code, atoms in (("success", "2"), ("criteria-not-met", "0"))]
    family["replacement_codes"] = ["success", "criteria-not-met"]
    setup["outcome_policy"]["limits"][0]["premium"]["atoms"] = "2"
    save(trial.root / "setup.json", setup)
    print("Illustrative synthetic terms/windows:\n" + json.dumps(setup, indent=2), flush=True)
    trial.call("init", ["billing", "init", trial.store, "--setup", trial.root / "setup.json", "--json"])
    artifacts = []
    for fixture in json.loads((FIXTURES / "artifacts.json").read_bytes()):
        begun = now()
        artifact = trial.root / (fixture["name"] + ".artifact.json")
        save(artifact, fixture)
        completed = now()  # file and directory were fsynced; no copied window timestamp
        record = {"name": fixture["name"], "attempt_started_at": stamp(begun), "completed_at": stamp(completed),
                  "artifact_sha256": hashlib.sha256(artifact.read_bytes()).hexdigest(),
                  "operation_id": fixture["name"] + "-generation"}
        save(trial.root / (fixture["name"] + ".completion.json"), record)
        require(completed <= start and completed <= start + timedelta(seconds=2),
                "Work missed a start; stop without rewriting times or resetting this store")
        request = {"schema": "ledger-event/1", "id": fixture["name"] + "-base",
                   "operation_id": record["operation_id"], "type": "content.generated", "customer": CUSTOMER,
                   "occurred_at": stamp(completed)}
        path = trial.root / (fixture["name"] + ".base-request.json")
        save(path, request)
        receipt = trial.submit("accept", path)
        target = receipt["body"]["target"]
        trial.verify("accept", request, receipt, target, receipt, fixture["name"] + "-base-explain")
        artifacts.append((artifact, record, path, receipt, target))
    print("Five reports completed and base receipts reconciled; waiting for ordinary start.", flush=True)
    wait_until(start)
    operations = [("accept", p, r, t, r) for _, _, p, r, t in artifacts]
    assessed = []
    for artifact, record, _, base, target in artifacts:
        raw = artifact.read_bytes()
        require(hashlib.sha256(raw).hexdigest() == record["artifact_sha256"], "Artifact changed; keep outcome pending")
        report = json.loads(raw)
        passed = report["generated_total"] == sum(report["values"])
        assessment_at = now()  # the new quality evaluation has now actually completed
        require(start <= assessment_at < start + timedelta(seconds=60), "Assessment outside ordinary window")
        evidence = {**record, "assessment_at": stamp(assessment_at), "classification": "criteria-met" if passed else "criteria-not-met",
                    "expected_total": sum(report["values"]), "observed_total": report["generated_total"], "synthetic": True}
        save(trial.root / (record["name"] + ".assessment.json"), evidence)
        request = {"schema": "ledger-billing-outcome/2", "customer": CUSTOMER, "source": SOURCE,
                   "id": record["name"] + "-assessment", "target": target, "family": "delivery",
                   "occurred_at": stamp(assessment_at), "evidence": json.dumps(evidence, sort_keys=True),
                   "code": "success" if passed else "criteria-not-met"}
        path = trial.root / (record["name"] + ".outcome-request.json")
        save(path, request)
        receipt = trial.submit("outcome", path)
        trial.verify("outcome", request, receipt, target, base, record["name"] + "-outcome-explain")
        operations.append(("outcome", path, receipt, target, base))
        assessed.append(evidence)
    before = trial.history("before-retries")
    for operation, path, receipt, target, base in operations:
        response = trial.billing(path.stem + "-retry", [operation, "--customer", CUSTOMER, "--source", SOURCE, path])
        require(response.get("status") == "duplicate" and response.get("receipt") == receipt, "Retry changed receipt")
        trial.verify(operation, json.loads(path.read_bytes()), receipt, target, base, path.stem + "-retry-explain")
    after = trial.history("after-retries")
    require(before == after, "Exact retries changed complete history")
    entries = after["entries"]
    base_atoms = sum(int(e["net_atoms"]) for e in entries if e["receipt"]["kind"] == "base-acceptance")
    adjustment_atoms = sum(int(e["net_atoms"]) for e in entries if e["receipt"]["kind"] == "receipt")
    require(sum(e["classification"] == "criteria-met" for e in assessed) == 2
            and (base_atoms, adjustment_atoms, int(after["net_atoms"]), len(entries)) == (10, 4, 14, 10),
            "Observed outcomes/history differ from fixture; preserve actual evidence")
    return {"base_atoms": base_atoms, "adjustment_atoms": adjustment_atoms, "net_atoms": int(after["net_atoms"]),
            "currency": "USD", "scale": 2, "retained_entries": len(entries), "complete": after["complete"],
            "pending_outcomes": 0, "refused_operations": 0, "failed_operations": 0, "identical_retries": 10,
            "payment_collected": False, "assessments": assessed}


def classify(observation):
    if observation["transport"] != "ok":
        return "transport-error", None, "not-attempted"
    if observation["http_status"] != 200:
        return "provider-blocked", None, "not-attempted"
    try:
        response = json.loads(observation["body"])
        results = response["results"]
        require(isinstance(results, list) and all(isinstance(x, str) and x for x in results), "Invalid results schema")
    except (ValueError, KeyError, TypeError, RuntimeError):
        return "parse-error", None, "invalid"
    return ("verified-success" if results else "verified-zero-results"), len(results), "valid"


def classification(trial):
    observations = json.loads((FIXTURES / "observations.json").read_bytes())
    records = []
    for observation in observations:
        label, count, parse = classify(observation)
        records.append({**observation, "assessment_at": stamp(now()), "classification": label,
                        "verified_count": count, "parse_status": parse, "outcome_acknowledged": False,
                        "observation_sha256": hashlib.sha256(encode(observation)).hexdigest()})
    require([r["classification"] for r in records] == ["verified-success", "verified-zero-results", "provider-blocked", "transport-error", "parse-error"], "Classification mismatch")
    cutoff = now() + timedelta(seconds=2)
    records.append({"classification": "pending-before-cutoff", "assessment_at": stamp(now()),
                    "cutoff": stamp(cutoff), "cutoff_reached": False, "outcome_acknowledged": False})
    wait_until(cutoff)
    # Explicit synthetic probe condition: the valid zero-results fixture has no results at K.
    label, count, parse = classify(observations[1])
    records.append({"classification": "cutoff-assessed", "assessment_at": stamp(now()), "cutoff": stamp(cutoff),
                    "cutoff_reached": True, "underlying_classification": label, "verified_count": count,
                    "parse_status": parse, "condition": "valid synthetic fixture has zero results at K",
                    "outcome_acknowledged": False})
    save(trial.root / "classifications.json", records)
    return {"classifications": records, "ledger_writes": 0, "pending_unknown_observations": 3,
            "outcomes_acknowledged": 0, "synthetic": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--work-dir", type=Path, required=True, help="new directory under a private physical parent")
    parser.add_argument("--scenario", choices=("five-completed-artifacts", "evidence-classification"), required=True)
    args = parser.parse_args()
    os.umask(0o077)
    ledger, root = physical(args.ledger), physical(args.work_dir)
    require(ledger.is_file(), "Missing verified released executable")
    require(root.parent.is_dir() and root.parent.stat().st_mode & 0o077 == 0, "Parent must be private")
    root.mkdir(mode=0o700)  # refuse every existing trial; never reset or rewrite its evidence
    trial = Trial(ledger, root)
    version = subprocess.run([str(ledger), "--version"], capture_output=True)
    save(root / "version.command.json", [str(ledger), "--version"])
    save(root / "version.exit.json", {"exit_code": version.returncode})
    save(root / "version.stdout", version.stdout)
    save(root / "version.stderr", version.stderr)
    require(version.returncode == 0 and version.stdout.strip() == b"ledger 0.9.4 (local development)", "Requires released v0.9.4")
    identity = {"ledger": str(ledger), "ledger_sha256": hashlib.sha256(ledger.read_bytes()).hexdigest(),
                "example_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "scenario": args.scenario, "synthetic": True, "work_dir": str(root)}
    save(root / "identity.json", identity)
    summary = cohort(trial) if args.scenario == "five-completed-artifacts" else classification(trial)
    save(root / "summary.json", summary)
    print(json.dumps(summary, indent=2))
    print(f"Complete synthetic evidence: {root}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, RuntimeError) as error:
        print(f"Unacknowledged failure: {error}. Preserve all original requests and evidence.", file=sys.stderr)
        sys.exit(1)
