#!/usr/bin/env python3
"""One saved-input billing demo and one fresh-process replay. No admission."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import sys

EXAMPLE = "non_admitting_billing_journey"
CAPTURE_SHA = "805d3d9636cb0249180577f4e93c9b170088121aab4e6ec4262bff15444f4f05"
WATCHDOG_SHA = "dbd886789278553136c9e48b7e6067241a6d2f1dee5f74e588fc56ae17478802"
REPORT_LIMIT = 4 * 1024 * 1024


def sha(path):
    digest = hashlib.sha256()
    with Path(path).open("rb") as source:
        for block in iter(lambda: source.read(65536), b""):
            digest.update(block)
    return digest.hexdigest()


def write_json(path, body):
    with Path(path).open("x") as output:
        json.dump(body, output, indent=2, sort_keys=True)
        output.write("\n")


def load(path, maximum=REPORT_LIMIT):
    if Path(path).stat().st_size > maximum:
        raise ValueError("demo report byte bound")
    def unique(pairs):
        body = {}
        for key, value in pairs:
            if key in body:
                raise ValueError("duplicate demo artifact key")
            body[key] = value
        return body
    return json.loads(Path(path).read_bytes(), object_pairs_hook=unique)


def infrastructure(directory):
    directory = directory.resolve(strict=True)
    for name, expected in (("capture.py", CAPTURE_SHA), ("watchdog.py", WATCHDOG_SHA)):
        if sha(directory / name) != expected:
            raise ValueError("reviewed infrastructure hash changed")
    sys.path.insert(0, str(directory))
    spec = importlib.util.spec_from_file_location("reviewed_demo_capture", directory / "capture.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def build(args):
    root = args.worktree.resolve(strict=True)
    output = args.output.resolve()
    if output.exists() or output.is_relative_to(root):
        raise ValueError("new external capsule required")
    infra = infrastructure(args.infrastructure)
    env = infra.minimal_environment(root, output)
    commit = infra.observe(["git", "rev-parse", "HEAD"], root, env)
    if commit != args.source_commit or not re.fullmatch("[0-9a-f]{40}", commit) or infra.observe(["git", "status", "--porcelain"], root, env):
        raise ValueError("clean exact source required")
    helper = infra.helpers(root)
    files = helper.selected(root)
    for name in ("run.py", "README.md", "input.json"):
        path = root / "examples/non-admitting-billing-journey" / name
        if path.is_symlink() or not path.is_file():
            raise ValueError("regular saved demo source required")
        files[path.relative_to(root).as_posix()] = path.read_bytes()
    if not helper.clean_source(root, files, commit, env):
        raise ValueError("selected demo inputs differ from commit")
    inventory, framed = helper.inventory(files)
    sysroot = Path(infra.observe(["rustc", "--print", "sysroot"], root, env)).resolve(strict=True)
    tools = {name: (sysroot / "bin" / name).resolve(strict=True) for name in ("rustc", "cargo")}
    versions = {name: infra.observe([str(path), "--version"], root, env) for name, path in tools.items()}
    if not versions["rustc"].startswith("rustc 1.98.1 ") or not versions["cargo"].startswith("cargo 1.98.1 "):
        raise ValueError("pinned toolchain required")
    toolchain = {name: {"path": str(path), "sha256": sha(path), "version": versions[name]} for name, path in tools.items()}
    target = next(line[6:] for line in infra.observe([str(tools["rustc"]), "-vV"], root, env).splitlines() if line.startswith("host: "))
    if target != "aarch64-apple-darwin":
        raise ValueError("this reviewed capture supports the existing macOS target")
    env["RUSTC"] = str(tools["rustc"])
    output.mkdir(parents=True)
    source = output / "source"
    source.mkdir()
    for name, raw in files.items():
        path = source / name
        path.parent.mkdir(parents=True, exist_ok=True)
        with path.open("xb") as saved:
            saved.write(raw)
        path.chmod(0o444)
    write_json(output / "inventory.json", inventory)
    command = [str(tools["cargo"]), "build", "--locked", "--offline", "--package", "ledgerlab",
               "--example", EXAMPLE, "--target", target, "--target-dir", str(output / "target"), "--message-format=json"]
    result = infra.run_owned(command, source, env, output / "build.log", infra.Policy(wall_seconds=240, rss_bytes=None))
    write_json(output / "build-result.json", result)
    if result["exit_code"] != 0 or result["stop"]:
        return 1
    captured = helper.selected(source)
    for name in files:
        if name.startswith("examples/non-admitting-billing-journey/"):
            captured[name] = (source / name).read_bytes()
    if captured != files or any(sha(item["path"]) != item["sha256"] for item in toolchain.values()):
        raise ValueError("source/toolchain changed during build")
    binary = output / "target" / target / "debug/examples" / EXAMPLE
    if not binary.is_file() or binary.is_symlink() or not os.access(binary, os.X_OK):
        raise ValueError("owned example binary absent")
    ready = {"schema": "bean-counter-demo-ready/1", "source_commit": commit, "source_inventory_digest": framed,
             "binary": {"path": str(binary), "sha256": sha(binary)}, "toolchain": toolchain,
             "environment": env, "runner_sha256": sha(__file__), "input_sha256": sha(source / "examples/non-admitting-billing-journey/input.json"),
             "files": {name: sha(output / name) for name in ("inventory.json", "build-result.json", "build.log")}}
    write_json(output / "ready.json", ready)
    print(json.dumps({"status": "BUILD_ONLY", "ready_sha256": sha(output / "ready.json"), "binary_sha256": sha(binary)}))
    return 0


def amount(atoms, scale):
    # Decimal display only: no prices, totals or rounding are recomputed here.
    if not isinstance(atoms, str) or not re.fullmatch("-?(0|[1-9][0-9]*)", atoms):
        raise ValueError("canonical amount string required")
    negative = atoms.startswith("-")
    digits = atoms.removeprefix("-").zfill(scale + 1)
    text = digits if scale == 0 else digits[:-scale] + "." + digits[-scale:]
    text = text.rstrip("0").rstrip(".") if scale else text
    return ("-" if negative else "") + text


def check(report, expected):
    if report["schema"] != "bean-counter-non-admitting-billing-report/1" or report["classification"] != "synthetic_local_acknowledgement" or report["financial_admission"] is not False:
        raise ValueError("demo boundary changed")
    body = report["computed"]
    if body["currency"] != "USD" or body["scale"] != 18:
        raise ValueError("demo book changed")
    base = body["base"]
    for field, name in (("retail_total_atoms", "original_retail_basis"), ("model_component_atoms", "retail_model_component"), ("cost_observation_atoms", "cost_observation")):
        if base[field] != expected[name]["atoms"]:
            raise ValueError("independent original amount mismatch: " + field)
    retail = [x["amount_atoms"] for x in base["components"] if x["book"] in ("Retail", "retail")]
    if sorted(retail) != sorted([expected["base"]["atoms"], expected["retail_model_component"]["atoms"]]):
        raise ValueError("independent named retail components mismatch")
    if len(body["alternatives"]) != 2 or {x["name"] for x in body["alternatives"]} != {"success", "failure"}:
        raise ValueError("two exclusive alternatives required")
    for alternative in body["alternatives"]:
        named = expected[alternative["name"]]
        if (alternative["delta_atoms"] != named["adjustment"]["atoms"] or alternative["total_atoms"] != named["total"]["atoms"]
            or alternative["basis_atoms"] != expected["original_retail_basis"]["atoms"] or alternative["target"] != body["base_target"]
            or alternative["policy_terms_ref"] != body["policy_terms_ref"] or not alternative["explanations"]):
            raise ValueError("independent outcome/link/explanation mismatch")
    required = {"wrong_target", "wrong_evidence", "wrong_unit", "exclusive_second_claim", "changed_original_artifact", "changed_retained_output"}
    if set(body["negative_checks"]) != required or any(value is not True for value in body["negative_checks"].values()):
        raise ValueError("required refusal proof missing")
    return body


def present(body, output):
    scale = body["scale"]
    base = body["base"]
    lines = ["# Saved-policy billing journey", "", "Synthetic local acknowledgement; no financial admission or customer charge.", "",
             "Original policy/terms: `" + str(body["policy_terms_ref"]) + "`", "",
             "Retained acknowledgement: `" + str(body["acknowledgement_ref"]) + "`", "",
             "| Original component | Book | Computed USD |", "|---|---|---:|"]
    for component in base["components"]:
        lines.append(f'| {component["component"]} | {component["book"]} | {amount(component["amount_atoms"], scale)} |')
    lines += ["", "Original component explanations from the pricing engine:", "", "```json",
              json.dumps(base["components"], indent=2, sort_keys=True), "```", ""]
    lines += ["", "Provider observation is separate evidence and is excluded from the payable total.", "",
              "Base retail total: **USD " + amount(base["retail_total_atoms"], scale) + "**.", "",
              "The following are alternative histories for the same successfully completed target; they are not stacked.", ""]
    for branch in body["alternatives"]:
        lines += [f'**{branch["name"].title()}**: adjustment USD {amount(branch["delta_atoms"], scale)}; total **USD {amount(branch["total_atoms"], scale)}**.', "",
                  "Linked target: `" + str(branch["target"]) + "`; frozen family: `" + str(branch["family"]) + "`.", "",
                  "Original outcome percentage: `" + str(branch["percentage"]["numerator"]) + "/" + str(branch["percentage"]["denominator"]) + "%`.", "",
                  "Production explanations:", "", "```json", json.dumps(branch["explanations"], indent=2, sort_keys=True), "```", ""]
    lines += ["Fresh-process restart recomputed identical complete amounts, identities and explanations from saved inputs.", "",
              "This concrete demo does not qualify R2/Wave 2, financial admission, authenticated assent/provider facts, full-profile resource bounds or future correction capacity."]
    with output.open("x") as report:
        report.write("\n".join(lines) + "\n")


def run(args):
    output = args.capsule.resolve(strict=True)
    if sha(output / "ready.json") != args.ready_sha256:
        raise ValueError("root-pinned ready manifest changed")
    ready = load(output / "ready.json")
    if ready["runner_sha256"] != sha(__file__):
        raise ValueError("reviewed demo runner changed")
    for name, expected in ready["files"].items():
        if sha(output / name) != expected:
            raise ValueError("captured metadata changed")
    binary = Path(ready["binary"]["path"])
    if binary.is_symlink() or not binary.resolve(strict=True).is_relative_to(output / "target") or sha(binary) != ready["binary"]["sha256"]:
        raise ValueError("captured binary changed")
    input_path = output / "source/examples/non-admitting-billing-journey/input.json"
    if sha(input_path) != ready["input_sha256"]:
        raise ValueError("saved source input changed")
    infra = infrastructure(args.infrastructure)
    if sha(args.expected) != args.expected_sha256:
        raise ValueError("independent expectation file changed")
    expected = load(args.expected)
    with (output / "DEMO-DISPATCHED").open("x") as marker:
        marker.write("one two-process journey; no automatic retry\n")
    reports = []
    archive = output / "archive"
    for mode in ("start", "replay"):
        command = [str(binary), mode]
        if mode == "start":
            command += ["--input", str(input_path)]
        command += ["--artifacts", str(archive)]
        result = infra.run_owned(command, output / "source", ready["environment"], output / (mode + ".log"), infra.Policy())
        write_json(output / (mode + "-result.json"), result)
        if result["stop"] or result["exit_code"] != 0:
            return 1
        report = load(output / (mode + ".log"))
        if report["mode"] != mode or (mode == "replay" and report["replay_verified"] is not True):
            raise ValueError("fresh/replay protocol mismatch")
        reports.append(check(report, expected))
    if reports[0] != reports[1]:
        raise ValueError("restart computed identities/amounts/explanations changed")
    if sha(binary) != ready["binary"]["sha256"] or sha(input_path) != ready["input_sha256"] or sha(args.expected) != args.expected_sha256:
        raise ValueError("binary/input changed during journey")
    present(reports[1], output / "REPORT.md")
    write_json(output / "OUTCOME.json", {"status": "NON_ADMITTING_DEMO_PASS", "source_commit": ready["source_commit"],
        "binary_sha256": ready["binary"]["sha256"], "expected_sha256": sha(args.expected), "fresh_process_pids": [load(output / (mode + "-result.json"))["pid"] for mode in ("start", "replay")],
        "complete_computed_equality": True, "financial_admission": False, "automatic_retry": False,
        "files": {name: sha(output / name) for name in ("start.log", "replay.log", "start-result.json", "replay-result.json", "REPORT.md")}})
    print((output / "REPORT.md").read_text())
    return 0


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    builder = sub.add_parser("build")
    builder.add_argument("--worktree", type=Path, required=True)
    builder.add_argument("--source-commit", required=True)
    builder.add_argument("--output", type=Path, required=True)
    execution = sub.add_parser("run")
    execution.add_argument("--capsule", type=Path, required=True)
    execution.add_argument("--ready-sha256", required=True)
    execution.add_argument("--expected", type=Path, required=True)
    execution.add_argument("--expected-sha256", required=True)
    for child in (builder, execution):
        child.add_argument("--infrastructure", type=Path, required=True)
    args = parser.parse_args()
    raise SystemExit(build(args) if args.operation == "build" else run(args))
