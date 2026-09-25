#!/usr/bin/env python3
"""Live OpenCode-provider + candidate-CLI E2E using only public synthetic input."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import uuid

ROOT = Path(__file__).resolve().parents[2]
SETUP = ROOT / "contracts/candidates/admission-v1/setup.json"
PROFILE = "admission/1-candidate.1"
PRODUCTS = ["Amber", "Birch", "Cedar", "Delta", "Elm"]
DEFAULT_ZEN_MODEL = "opencode/space-bunny-free"
DEFAULT_LOCAL_MODEL = "local-qwen/unsloth/Qwen3.8-27B-GGUF"
CANDIDATE_STATUS = "unreviewed and unfrozen"


class E2EFailure(Exception):
    pass


def utc_now():
    return datetime.now(timezone.utc).isoformat(timespec="microseconds").replace("+00:00", "Z")


def canonical_json(value):
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()


def artifact_digest(artifact):
    prefix = f"ledgerlab/artifact/{PROFILE}\0".encode()
    return "sha256:" + hashlib.sha256(prefix + canonical_json(artifact)).hexdigest()


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        stream.flush()
        os.fsync(stream.fileno())
    directory_fd = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(directory_fd)
    finally:
        os.close(directory_fd)


def parse_json_output(stdout, label):
    try:
        return json.loads(stdout)
    except json.JSONDecodeError as error:
        raise E2EFailure(f"{label} returned non-JSON output") from error


def candidate_call(binary, operation, store, request_path=None, expected=0):
    argv = [str(binary), "zen-charge-candidate", operation, str(store), "--json"]
    if request_path is not None:
        argv.append(str(request_path))
    completed = subprocess.run(argv, capture_output=True, text=True, timeout=30)
    if completed.returncode != expected:
        raise E2EFailure(f"candidate CLI {operation} returned exit {completed.returncode}, expected {expected}")
    return parse_json_output(completed.stdout, f"candidate CLI {operation}")


def submit(binary, store, run_dir, label, command, expected=0):
    path = run_dir / f"{label}.json"
    write_json(path, command)
    response = candidate_call(binary, "submit", store, path, expected=expected)
    write_json(run_dir / f"{label}-response.json", response)
    return response


def model_record(stdout, model):
    """Read the exact model's JSON record from `opencode models --verbose`."""
    clean = re.sub(r"\x1b\[[0-9;]*[A-Za-z]", "", stdout)
    decoder = json.JSONDecoder()
    for match in re.finditer(r"(?m)^" + re.escape(model) + r"\s*\n\s*\{", clean):
        start = clean.find("{", match.start())
        try:
            value, _ = decoder.raw_decode(clean[start:])
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict):
            return value
    raise E2EFailure(f"OpenCode model catalog did not return {model}")


def preflight_model(model):
    provider, separator, _ = model.partition("/")
    if not separator or provider not in {"opencode", "local-qwen"}:
        raise E2EFailure(f"unsupported provider route: {model}")
    completed = subprocess.run(["opencode", "models", provider, "--verbose", "--refresh"],
                               capture_output=True, text=True, timeout=120)
    if completed.returncode != 0:
        raise E2EFailure(f"OpenCode catalog refresh failed for provider {provider}")
    record = model_record(completed.stdout, model)
    expected_id = model.split("/", 1)[1]
    if record.get("providerID") != provider or record.get("id") != expected_id:
        raise E2EFailure(f"OpenCode catalog identity mismatch for {model}")
    if record.get("status") != "active":
        raise E2EFailure(f"OpenCode model is not active: {model}")
    cost = record.get("cost")
    if not isinstance(cost, dict) or any(not is_zero_cost(cost.get(key)) for key in ("input", "output")):
        raise E2EFailure(f"OpenCode catalog does not report zero input/output cost for {model}")
    cache = cost.get("cache", {})
    if not isinstance(cache, dict) or any(not is_zero_cost(cache.get(key)) for key in ("read", "write")):
        raise E2EFailure(f"OpenCode catalog does not report zero cache cost for {model}")
    return record


def walk(value):
    if isinstance(value, dict):
        yield value
        for nested in value.values():
            yield from walk(nested)
    elif isinstance(value, list):
        for nested in value:
            yield from walk(nested)


def is_zero_cost(value):
    # Equality to zero also excludes NaN/infinity; bool must not masquerade as 0.
    return type(value) in (int, float) and value == 0


def required_zero_cost(records, source):
    present = False
    for record in walk(records):
        if "cost" in record:
            present = True
            if not is_zero_cost(record["cost"]):
                raise E2EFailure(f"{source} contains malformed or nonzero cost evidence")
    if not present:
        raise E2EFailure(f"{source} is missing required cost evidence")
    # Validate every observation, never sum away conflicting positive/negative costs.
    return 0


def read_events(stdout):
    lines = [line for line in stdout.splitlines() if line.strip()]
    if not lines:
        raise E2EFailure("OpenCode returned an empty event stream")
    try:
        return [json.loads(line) for line in lines]
    except json.JSONDecodeError:
        try:
            value = json.loads(stdout)
        except json.JSONDecodeError as error:
            raise E2EFailure("OpenCode returned an unreadable JSON event stream") from error
        return value if isinstance(value, list) else [value]


def event_text(events):
    chunks = []

    def visit(value):
        if isinstance(value, dict):
            kind = value.get("type")
            if kind in {"tool", "tool_use", "tool_call", "tool-invocation"}:
                raise E2EFailure("OpenCode attempted a tool call in the no-tools E2E agent")
            part = value.get("part")
            if kind in {"text", "text_delta"} and isinstance(value.get("text"), str):
                chunks.append(value["text"])
                return
            if isinstance(part, dict) and part.get("type") == "text" and isinstance(part.get("text"), str):
                chunks.append(part["text"])
                return
            if kind == "text" and isinstance(part, str):
                chunks.append(part)
                return
            for nested in value.values():
                visit(nested)
        elif isinstance(value, list):
            for nested in value:
                visit(nested)

    visit(events)
    return "".join(chunks).strip()


def session_metadata(events):
    session_ids = []
    model_pairs = []
    terminal_reasons = []
    token_rows = []
    tool_events = []
    for event in walk(events):
        event_type = str(event.get("type", "")).lower()
        if any(token in event_type for token in ("tool_call", "tool_use", "tool-invocation")):
            tool_events.append(event_type)
        for key in ("sessionID", "sessionId", "session_id"):
            value = event.get(key)
            if isinstance(value, str) and value:
                session_ids.append(value)
        provider = event.get("providerID", event.get("providerId"))
        model = event.get("modelID", event.get("modelId"))
        if isinstance(provider, str) and isinstance(model, str):
            model_pairs.append((provider, model))
        if "finish" in event_type:
            reason = event.get("reason")
            if reason is None and isinstance(event.get("part"), dict):
                reason = event["part"].get("reason")
            if isinstance(reason, str):
                terminal_reasons.append(reason)
        tokens = event.get("tokens")
        if isinstance(tokens, dict):
            token_rows.append(tokens)
    if tool_events:
        raise E2EFailure("OpenCode emitted a tool event in the no-tools E2E agent")
    return {
        "session_id": session_ids[-1] if session_ids else None,
        "model_pairs": model_pairs,
        "terminal": terminal_reasons[-1] if terminal_reasons else None,
        "reported_cost": required_zero_cost(events, "OpenCode event stream"),
        "token_rows": token_rows,
    }


def export_session(session_id, workspace):
    if not session_id:
        raise E2EFailure("OpenCode event stream did not include the session ID needed for sanitized export")
    try:
        completed = subprocess.run(["opencode", "export", session_id, "--sanitize"], cwd=workspace,
                                   capture_output=True, timeout=60)
    except subprocess.TimeoutExpired as error:
        raise E2EFailure("OpenCode sanitized export timed out; the order will be failed") from error
    if completed.returncode != 0:
        raise E2EFailure("OpenCode could not export the completed session in sanitized form")
    digest = hashlib.sha256(completed.stdout).hexdigest()
    try:
        exported = json.loads(completed.stdout)
    except json.JSONDecodeError as error:
        raise E2EFailure("OpenCode sanitized session export was not JSON") from error
    pairs = []
    tokens = []
    for item in walk(exported):
        provider = item.get("providerID", item.get("providerId"))
        model = item.get("modelID", item.get("modelId"))
        if isinstance(provider, str) and isinstance(model, str):
            pairs.append((provider, model))
        if isinstance(item.get("tokens"), dict):
            tokens.append(item["tokens"])
    return digest, {"model_pairs": pairs, "token_rows": tokens,
                    "reported_cost": required_zero_cost(exported, "OpenCode sanitized export")}


def prepare_workspace(path):
    path.mkdir(parents=True, exist_ok=False)
    agent_dir = path / ".opencode/agents"
    agent_dir.mkdir(parents=True)
    agent_file = agent_dir / "bean-counter-e2e.md"
    agent_file.write_text(
        "---\n"
        "description: Public synthetic text-only billing E2E\n"
        "mode: primary\n"
        "permission:\n"
        '  "*": deny\n'
        "---\n"
        "Return only the exact JSON artifact requested by the user. Do not use tools.\n",
        encoding="utf-8")
    # A temporary Git root prevents OpenCode from discovering this repository's AGENTS.md.
    initialized = subprocess.run(["git", "init", "--quiet", str(path)], capture_output=True, text=True, timeout=20)
    if initialized.returncode != 0:
        raise E2EFailure("could not initialize isolated OpenCode workspace")
    return agent_file


def run_model(model, prompt, run_id, run_dir, catalog):
    provider = model.split("/", 1)[0]
    slug = "zen" if provider == "opencode" else "local-qwen"
    workspace = run_dir / f"opencode-workspace-{slug}"
    prepare_workspace(workspace)
    env = dict(os.environ)
    env["OPENCODE_AUTO_SHARE"] = "0"
    env.pop("OPENCODE_SERVER_PASSWORD", None)
    env.pop("OPENCODE_SERVER_USERNAME", None)
    argv = ["opencode", "run", "--pure", "--model", model, "--agent", "bean-counter-e2e",
            "--format", "json", "--title", f"bean-counter-e2e-{run_id}-{slug}", "--dir", str(workspace), prompt]
    started_at = utc_now()
    try:
        completed = subprocess.run(argv, cwd=workspace, env=env, stdin=subprocess.DEVNULL,
                                   capture_output=True, text=True, timeout=300)
    except subprocess.TimeoutExpired as error:
        write_json(run_dir / f"opencode-process-{slug}.json", {
            "exit_code": None,
            "timed_out": True,
            "started_at_utc": started_at,
            "finished_at_utc": utc_now(),
            "timeout_seconds": 300,
            "tools_disabled_by_agent": True,
            "pure_mode": True,
            "sharing_requested": False,
        })
        raise E2EFailure(f"OpenCode invocation timed out for {model}; raw provider output was not saved") from error
    finished_at = utc_now()
    process = {
        "exit_code": completed.returncode,
        "started_at_utc": started_at,
        "finished_at_utc": finished_at,
        "tools_disabled_by_agent": True,
        "pure_mode": True,
        "sharing_requested": False,
    }
    write_json(run_dir / f"opencode-process-{slug}.json", process)
    if completed.returncode != 0:
        raise E2EFailure(f"OpenCode invocation failed for {model}; raw provider output was not saved")
    events = read_events(completed.stdout)
    metadata = session_metadata(events)
    artifact_text = event_text(events)
    try:
        artifact = json.loads(artifact_text)
    except json.JSONDecodeError as error:
        raise E2EFailure(f"{model} did not return a bare JSON artifact") from error
    if not isinstance(artifact, list) or any(not isinstance(item, str) for item in artifact):
        raise E2EFailure(f"{model} returned an artifact with the wrong shape")
    if artifact != PRODUCTS:
        raise E2EFailure(f"{model} output failed the exact five-products acceptance rule")
    session_hash, export_metadata = export_session(metadata["session_id"], workspace)
    pairs = metadata["model_pairs"]
    if export_metadata:
        pairs.extend(export_metadata["model_pairs"])
    if not pairs or any(pair != (provider, model.split("/", 1)[1]) for pair in pairs):
        raise E2EFailure(f"{model} invocation metadata did not verify the requested provider/model")
    terminal = metadata["terminal"]
    if terminal != "stop":
        raise E2EFailure(f"{model} invocation did not finish normally")
    reported_cost = metadata["reported_cost"]
    token_rows = metadata["token_rows"]
    export_cost = export_metadata["reported_cost"]
    if not is_zero_cost(reported_cost) or not is_zero_cost(export_cost) or reported_cost != export_cost:
        raise E2EFailure(f"{model} cost evidence must agree at zero in events and sanitized export")
    token_rows.extend(export_metadata["token_rows"])
    token_totals = token_rows[-1] if token_rows else None
    evidence = {
        "schema": "opencode-provider-charge-e2e-evidence/1",
        "synthetic": True,
        "candidate_status": CANDIDATE_STATUS,
        "requested_model": model,
        "returned_provider": provider,
        "returned_model": model.split("/", 1)[1],
        "catalog_status": catalog["status"],
        "catalog_cost": catalog["cost"],
        "reported_cost": reported_cost,
        "reported_cost_sources": {"events": reported_cost, "sanitized_export": export_cost},
        "token_metadata_present": bool(token_rows),
        "token_usage": token_totals,
        "terminal": terminal,
        "opencode_pure_mode": True,
        "tools_disabled": True,
        "sharing_requested": False,
        "opencode_session_id": metadata["session_id"],
        "sanitized_export_sha256": session_hash,
        "artifact": artifact,
        "artifact_sha256": artifact_digest(artifact),
        "started_at_utc": started_at,
        "finished_at_utc": finished_at,
    }
    write_json(run_dir / f"model-evidence-{slug}.json", evidence)
    return evidence


def command(binding, operation, evidence, artifact=None):
    result = {"schema": PROFILE, "operation": operation, "binding": binding,
              "evidence": dict(evidence)}
    if artifact is not None:
        result["artifact"] = artifact
    return result


def checked_receipt(response, kind, atoms):
    receipt = response.get("receipt")
    if not isinstance(receipt, dict) or receipt.get("body", {}).get("kind") != kind:
        raise E2EFailure(f"candidate did not return a {kind} receipt")
    if receipt["body"].get("atoms") != str(atoms):
        raise E2EFailure(f"candidate {kind} receipt had the wrong amount")
    return receipt


def run_order(binary, store, run_dir, initial, order_index, model, catalog, prompt, run_id):
    binding = initial["orders"][order_index]["binding"]
    order_slug = "zen" if model.startswith("opencode/") else "local-qwen"
    seed = f"{run_id}-{order_slug}"
    evidence = {
        "delivery_id": f"delivery-{seed}",
        "attempt_id": f"attempt-{seed}",
        # This is the preallocated billing-correlation ID; the actual OpenCode session ID is recorded separately.
        "session_id": f"billing-session-{seed}",
        "model": model,
        "outcome_id": f"outcome-{seed}",
    }
    # Retain the original recovery operation before acquiring or charging anything.
    fail_path = run_dir / f"{order_slug}-fail.json"
    write_json(fail_path, command(binding, "fail", evidence))
    reserve = submit(binary, store, run_dir, f"{order_slug}-reserve",
                     command(binding, "reserve", evidence))
    reserve_receipt = checked_receipt(reserve, "reserve", 0)
    admit = submit(binary, store, run_dir, f"{order_slug}-admit",
                   command(binding, "admit", evidence))
    admit_receipt = checked_receipt(admit, "admit", 1)
    if admit_receipt["body"].get("reservation_receipt") != reserve_receipt["id"]:
        raise E2EFailure(f"{model} admission receipt did not reference its reservation")

    try:
        model_evidence = run_model(model, prompt, run_id, run_dir, catalog)
    except (E2EFailure, subprocess.SubprocessError, OSError) as error:
        # subprocess.run kills and waits for a timed-out child before raising.
        # This releases only the logical slot; it makes no remote-capacity claim.
        reason = str(error) if isinstance(error, E2EFailure) else f"{model} process failed ({type(error).__name__})"
        recovery = [str(binary), "zen-charge-candidate", "submit", str(store), str(fail_path), "--json"]
        statement = [str(binary), "zen-charge-candidate", "statement", str(store), "--json"]
        try:
            fail = candidate_call(binary, "submit", store, fail_path)
            if fail.get("status") not in {"accepted", "duplicate"}:
                raise E2EFailure("candidate did not acknowledge slot release")
            receipt = checked_receipt(fail, "fail", 0)
            if receipt["body"].get("binding") != binding:
                raise E2EFailure("candidate failure receipt had the wrong binding")
            write_json(run_dir / f"{order_slug}-fail-response.json", fail)
        except (E2EFailure, subprocess.SubprocessError, OSError) as cleanup_error:
            unresolved = {"status": "unresolved", "reason": reason, "store": str(store),
                          "failure_request": str(fail_path), "recovery_command": recovery,
                          "statement_command": statement}
            try:
                write_json(run_dir / f"{order_slug}-recovery.json", unresolved)
            except OSError:
                pass  # Even if storage is unavailable, stderr retains exact recovery instructions.
            raise E2EFailure(
                f"{reason}; slot release is unresolved. Store: {store}. "
                f"Inspect: {shlex.join(statement)} . Retry the original failure operation: {shlex.join(recovery)}"
            ) from cleanup_error
        raise E2EFailure(f"{reason}; logical slot released, admission charge retained, "
                         f"no outcome charge booked. Store: {store}") from error

    artifact = model_evidence["artifact"]
    outcome_command = command(binding, "outcome", evidence, artifact)
    outcome = submit(binary, store, run_dir, f"{order_slug}-outcome", outcome_command)
    outcome_receipt = checked_receipt(outcome, "outcome", 499)
    if outcome_receipt["body"].get("admission_receipt") != admit_receipt["id"]:
        raise E2EFailure(f"{model} outcome receipt did not reference its admission")
    if outcome_receipt["body"].get("artifact_hash") != artifact_digest(artifact):
        raise E2EFailure(f"{model} outcome receipt did not bind the accepted artifact hash")

    replay = submit(binary, store, run_dir, f"{order_slug}-outcome-replay", outcome_command)
    if replay.get("status") != "duplicate" or replay.get("receipt") != outcome_receipt:
        raise E2EFailure(f"{model} exact replay did not return its original receipt")
    renamed = dict(evidence)
    renamed.update({key: "mutated-" + value for key, value in evidence.items()})
    renamed_replay = submit(binary, store, run_dir, f"{order_slug}-renamed-evidence-replay",
                            command(binding, "outcome", renamed, artifact))
    if renamed_replay.get("status") != "duplicate" or renamed_replay.get("receipt") != outcome_receipt:
        raise E2EFailure(f"{model} changed subordinate IDs bypassed idempotent replay")

    mutated = dict(binding)
    mutated["order_id"] = binding["order_id"] + "-mutated"
    before_mutation = candidate_call(binary, "statement", store)
    rejection = submit(binary, store, run_dir, f"{order_slug}-mutated-order-id",
                       command(mutated, "outcome", evidence, artifact), expected=6)
    if rejection.get("code") not in {"UNAUTHORIZED_ORDER", "ORDER", "AUTHORIZATION", "BINDING"}:
        raise E2EFailure(f"{model} slightly mutated order ID was not rejected by authorization")
    after_mutation = candidate_call(binary, "statement", store)
    if after_mutation != before_mutation:
        raise E2EFailure(f"{model} mutated order ID changed the candidate statement")
    return {
        "model": model,
        "provider": model.split("/", 1)[0],
        "order_id": binding["order_id"],
        "admission_atoms": "1",
        "outcome_atoms": "499",
        "total_atoms": "500",
        "outcome_replay_status": replay["status"],
        "renamed_evidence_replay_status": renamed_replay["status"],
        "mutated_order_id_rejected": True,
        "reported_cost": model_evidence["reported_cost"],
        "receipt_ids": {
            "reserve": reserve_receipt["id"],
            "admit": admit_receipt["id"],
            "outcome": outcome_receipt["id"],
        },
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ledger", type=Path, required=True)
    parser.add_argument("--build-manifest", type=Path, required=True,
                        help="manifest emitted by check-opencode-zen-charge-e2e.sh")
    parser.add_argument("--zen-model", default=DEFAULT_ZEN_MODEL)
    parser.add_argument("--local-model", default=DEFAULT_LOCAL_MODEL)
    args = parser.parse_args()
    binary = args.ledger.resolve()
    if not binary.is_file():
        raise E2EFailure(f"candidate CLI binary does not exist: {binary}")
    source_commit = subprocess.run(["git", "rev-parse", "HEAD"], cwd=ROOT,
                                  capture_output=True, text=True, timeout=20, check=True).stdout.strip()
    source_tree = subprocess.run(["git", "rev-parse", "HEAD^{tree}"], cwd=ROOT,
                                 capture_output=True, text=True, timeout=20, check=True).stdout.strip()
    status = subprocess.run(["git", "status", "--porcelain"], cwd=ROOT,
                            capture_output=True, text=True, timeout=20, check=True).stdout
    if status:
        raise E2EFailure("source worktree must be clean before provider E2E so binary provenance is exact")
    binary_sha256 = hashlib.sha256(binary.read_bytes()).hexdigest()
    rustc_version = subprocess.run(["rustc", "--version"], capture_output=True,
                                   text=True, timeout=20, check=True).stdout.strip()
    manifest_path = args.build_manifest.resolve()
    try:
        build_manifest = json.loads(manifest_path.read_text())
    except (OSError, json.JSONDecodeError) as error:
        raise E2EFailure("provider E2E build manifest is missing or invalid") from error
    expected_manifest = {
        "source_commit": source_commit,
        "source_tree": source_tree,
        "rustc_version": rustc_version,
        "feature_profile": "--no-default-features --features zen-charge-candidate",
        "candidate_binary_sha256": binary_sha256,
        "candidate_binary_bytes": binary.stat().st_size,
    }
    if build_manifest != expected_manifest:
        raise E2EFailure("provider E2E binary does not match the clean-source build manifest")
    build_manifest_sha256 = hashlib.sha256(manifest_path.read_bytes()).hexdigest()
    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + uuid.uuid4().hex[:8]
    work_root = ROOT / "work/opencode-provider-charge-e2e"
    work_root.mkdir(parents=True, exist_ok=True)
    run_dir = Path(tempfile.mkdtemp(prefix=run_id + "-", dir=work_root))
    os.chmod(run_dir, 0o700)
    started_at = utc_now()
    catalogs = {
        args.zen_model: preflight_model(args.zen_model),
        args.local_model: preflight_model(args.local_model),
    }

    store = run_dir / "store"
    init = candidate_call(binary, "init", store, SETUP)
    write_json(run_dir / "init-response.json", init)
    if len(init.get("orders", [])) != 2 or not isinstance(init.get("deliverable_input"), str):
        raise E2EFailure("candidate fixture did not provide the expected two synthetic orders")
    if init.get("net_atoms") != "0":
        raise E2EFailure("new synthetic candidate store did not start at zero atoms")
    prompt = init["deliverable_input"] + " Return only a JSON array of the names in alphabetical order; no prose or Markdown."
    providers = []
    providers.append(run_order(binary, store, run_dir, init, 0, args.zen_model,
                               catalogs[args.zen_model], prompt, run_id))
    interim = candidate_call(binary, "statement", store)
    write_json(run_dir / "statement-after-zen.json", interim)
    if interim.get("net_atoms") != "500" or interim.get("payment_collected") is not False:
        raise E2EFailure("statement after Zen order did not show the expected unpaid 500-atom balance")
    providers.append(run_order(binary, store, run_dir, init, 1, args.local_model,
                               catalogs[args.local_model], prompt, run_id))
    statement = candidate_call(binary, "statement", store)
    if statement.get("net_atoms") != "1000" or statement.get("complete") is not True:
        raise E2EFailure("final statement did not show two completed 500-atom synthetic orders")
    if statement.get("payment_collected") is not False or statement.get("provider_cost_accounted") is not False:
        raise E2EFailure("candidate statement claimed payment or provider cost accounting")
    if statement.get("slot_owner") is not None:
        raise E2EFailure("candidate did not release its logical slot after accepted outcomes")
    for expected_id in ("order-1", "order-2"):
        order = next((item for item in statement["orders"] if item["binding"]["order_id"] == expected_id), None)
        if order is None or order.get("phase") != "completed" or order.get("net_atoms") != "500":
            raise E2EFailure(f"final statement did not reconcile {expected_id}")

    write_json(run_dir / "statement.json", statement)
    result = {
        "schema": "opencode-provider-charge-e2e-result/1",
        "status": "passed",
        "synthetic": True,
        "candidate_status": CANDIDATE_STATUS,
        "run_id": run_id,
        "started_at_utc": started_at,
        "finished_at_utc": utc_now(),
        "opencode_version": subprocess.run(["opencode", "--version"], capture_output=True,
                                            text=True, timeout=20, check=True).stdout.strip(),
        "source": {
            "commit": source_commit,
            "tree": source_tree,
            "worktree_clean": True,
            "rustc_version": rustc_version,
            "feature_profile": "--no-default-features --features zen-charge-candidate",
            "candidate_binary_sha256": binary_sha256,
            "build_manifest_sha256": build_manifest_sha256,
        },
        "provider_runs": providers,
        "model_requests": len(providers),
        "completed_orders": len(providers),
        "per_order_atoms": {"admission": "1", "outcome": "499", "total": "500"},
        "net_atoms": statement["net_atoms"],
        "total_usd": "10.00",
        "payment_collected": statement["payment_collected"],
        "provider_cost_accounted": statement["provider_cost_accounted"],
        "logical_slot_released": statement["slot_owner"] is None,
        "statement_file": "statement.json",
    }
    write_json(run_dir / "result.json", result)
    print(json.dumps({"status": result["status"], "run_dir": str(run_dir),
                      "models": [item["model"] for item in providers],
                      "orders": len(providers), "net_atoms": statement["net_atoms"],
                      "usd": result["total_usd"], "payment_collected": False}, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (E2EFailure, subprocess.TimeoutExpired) as error:
        print(f"E2E failed: {error}", file=sys.stderr)
        sys.exit(1)
