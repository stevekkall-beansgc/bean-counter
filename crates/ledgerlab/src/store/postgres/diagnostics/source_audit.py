#!/usr/bin/env python3
"""Locate locked driver source, record evidence, and return the TLS stop gate.

Exit 2 means the reviewed incompatibility was reproduced; it is not a passing
product conformance result. No network or driver-source mutation is performed.
"""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tomllib


def main():
    manifest = Path(sys.argv[1]).resolve()
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--manifest-path", str(manifest),
        "--format-version", "1", "--locked", "--offline",
    ]))
    packages = {p["name"]: p for p in metadata["packages"]}
    for name in ("sqlx", "sqlx-core", "sqlx-postgres"):
        assert packages[name]["version"] == "0.9.0", name
    roots = {
        name: Path(packages[name]["manifest_path"]).parent
        for name in ("sqlx", "sqlx-core", "sqlx-postgres")
    }
    files = [
        ("sqlx", "Cargo.toml"),
        ("sqlx-core", "Cargo.toml"),
        ("sqlx-core", "src/net/tls/tls_rustls.rs"),
        ("sqlx-postgres", "src/options/mod.rs"),
        ("sqlx-postgres", "src/options/connect.rs"),
        ("sqlx-postgres", "src/connection/mod.rs"),
        ("sqlx-postgres", "src/connection/establish.rs"),
        ("sqlx-postgres", "src/connection/tls.rs"),
        ("sqlx-postgres", "src/transaction.rs"),
    ]
    sources = []
    for package, path in files:
        data = (roots[package] / path).read_bytes()
        sources.append({"package": package, "version": "0.9.0", "path": path,
                        "sha256": hashlib.sha256(data).hexdigest()})
    tls = (roots["sqlx-core"] / "src/net/tls/tls_rustls.rs").read_text()
    public_roots = 'RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned())'
    assert public_roots in tls
    init = tls.index("let mut cert_store = import_root_certs();")
    add = tls.index("cert_store.add(cert)", init)
    assert "if let Some(ca) = tls_config.root_cert_path" in tls[init:add]
    assert "cert_store =" not in tls[init + len("let mut cert_store ="):add]
    assert "cert_store.clear" not in tls[init:add]

    core_features = tomllib.loads((roots["sqlx-core"] / "Cargo.toml").read_text())["features"]
    facade_features = tomllib.loads((roots["sqlx"] / "Cargo.toml").read_text())["features"]
    assert "webpki-roots" in core_features["_tls-rustls-ring-webpki"]
    assert "webpki-roots" in core_features["_tls-rustls-aws-lc-rs"]
    assert "rustls-native-certs" in core_features["_tls-rustls-ring-native-roots"]
    assert facade_features["tls-rustls-ring"] == ["tls-rustls-ring-webpki"]
    assert "tls-rustls-no-roots" not in facade_features
    assert "tls-rustls-no-roots" not in core_features

    # Check the graph actually used by these probes, not merely manifest intent.
    core_id = packages["sqlx-core"]["id"]
    features = next(n["features"] for n in metadata["resolve"]["nodes"] if n["id"] == core_id)
    assert "_tls-rustls-ring-webpki" in features
    assert "webpki-roots" in features
    assert "rustls-native-certs" not in features
    assert "_tls-native-tls" not in features
    assert "sqlx-mysql" not in packages
    assert "sqlx-sqlite" not in packages

    report = {
        "status": "BLOCKED",
        "reason": "SQLx 0.9.0 supported Rustls features cannot express PEM-only trust",
        "source_sha256": sources,
        "selected_sqlx_core_features": features,
        "resolved_versions": {name: packages[name]["version"] for name in (
            "sqlx", "sqlx-core", "sqlx-postgres", "rustls", "rustls-webpki",
            "webpki-roots", "ring", "tokio", "tokio-rustls", "rcgen",
        )},
        "real_postgres_tests": "NOT RUN; no PG18/17 server available",
        "trust_isolation": "SOURCE-PROVEN INCOMPATIBLE; not claimed by local handshake tests",
    }
    (manifest.parent / "source-audit.json").write_text(json.dumps(report, indent=2) + "\n")
    print("BLOCKED: supplied PEM adds to bundled public roots; no supported no-roots feature.")
    print("Source fingerprints and resolved features: work/postgres-driver-gate/source-audit.json")
    return 2


if __name__ == "__main__":
    sys.exit(main())
