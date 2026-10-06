#!/usr/bin/env python3
"""Probe the real synthetic helper's verification under Python optimization.

The wrapper executes the real engine and changes only returned synthetic JSON,
never ledger storage. A changed receipt, posting, or backup snapshot must prevent
the helper from reporting success even with PYTHONOPTIMIZE enabled.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ledger", type=Path)
    parser.add_argument("--helper", type=Path, default=Path(__file__).resolve().parents[1]
                        / "examples/integration/run-synthetic.sh")
    args = parser.parse_args()
    ledger, helper = args.ledger.resolve(strict=True), args.helper.resolve(strict=True)
    results = []
    with tempfile.TemporaryDirectory(prefix="synthetic-verification-") as temporary:
        root = Path(temporary).resolve(strict=True)
        for optimize in ("", "1", "2"):
            for fault in ("none", "receipt", "posting", "snapshot"):
                case = root / f"opt-{optimize or 'off'}-{fault}"
                case.mkdir(mode=0o700)
                count = case / "calls.json"
                wrapper = case / "ledger response probe"
                wrapper.write_text(
                    "#!/usr/bin/env python3\nimport json,pathlib,subprocess,sys\n"
                    f"ledger={str(ledger)!r}; fault={fault!r}; count=pathlib.Path({str(count)!r})\n"
                    "args=sys.argv[1:]\n"
                    "result=subprocess.run([ledger,*args],capture_output=True,text=True)\n"
                    "output=result.stdout\n"
                    "if result.returncode == 0 and '--json' in args:\n"
                    "    value=json.loads(output)\n"
                    "    calls=json.loads(count.read_text()) if count.exists() else {}\n"
                    "    if 'accept' in args and any(a.endswith('work-success.json') for a in args):\n"
                    "        calls['success']=calls.get('success',0)+1\n"
                    "        if fault == 'receipt' and calls['success'] == 2:\n"
                    "            value['receipt']['body']['target']='synthetic-corrupt-target'\n"
                    "    if 'statement' in args:\n"
                    "        if fault == 'posting':\n"
                    "            value['entries'][0]['postings'][0]['body']['amount']['atoms']='999'\n"
                    "        if fault == 'snapshot' and any(a.endswith('billing-backup') for a in args):\n"
                    "            calls['backup']=calls.get('backup',0)+1\n"
                    "            if calls['backup'] == 2: value['snapshot_hash']='synthetic-corrupt-snapshot'\n"
                    "    count.write_text(json.dumps(calls))\n"
                    "    output=json.dumps(value)+'\\n'\n"
                    "sys.stdout.write(output); sys.stderr.write(result.stderr)\n"
                    "sys.exit(result.returncode)\n",
                    encoding="utf-8")
                wrapper.chmod(0o700)
                env = dict(os.environ)
                if optimize:
                    env["PYTHONOPTIMIZE"] = optimize
                else:
                    env.pop("PYTHONOPTIMIZE", None)
                run = subprocess.run(["sh", str(helper), str(wrapper), str(case / "store"),
                                      str(case / "results")], env=env, text=True,
                                     capture_output=True, timeout=120)
                diagnostic = {"receipt": "receipt changed across identical retry",
                              "posting": "expected",
                              "snapshot": "backup snapshot changed"}.get(fault)
                passed = (run.returncode == 0 and "Synthetic full-path check passed" in run.stdout
                          if fault == "none" else run.returncode != 0
                          and "Synthetic full-path check passed" not in run.stdout
                          and (diagnostic in run.stderr or "AssertionError" in run.stderr))
                results.append({"optimize": optimize or "off", "fault": fault,
                                "exit_code": run.returncode, "passed": passed,
                                "stderr": run.stderr if not passed else ""})
    report = {"schema": "bean-counter-synthetic-verification-regression/1",
              "ledger": str(ledger), "helper": str(helper),
              "passed": all(result["passed"] for result in results), "cases": results}
    print(json.dumps(report, indent=2))
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
