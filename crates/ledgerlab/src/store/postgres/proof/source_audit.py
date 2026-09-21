#!/usr/bin/env python3
"""Audit the locked proof's active dependencies and reviewed connection sources.

No downloads, package/source edits, credentials or database access. This is a
feature/source boundary check, not a general-purpose vulnerability scanner.
"""
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

manifest = Path(sys.argv[1]).resolve()
metadata = json.loads(subprocess.check_output([
    'cargo', 'metadata', '--manifest-path', str(manifest),
    '--format-version', '1', '--locked', '--offline',
]))
packages = {p['id']: p for p in metadata['packages']}
by_name = {p['name']: p for p in metadata['packages']}
nodes = {n['id']: n for n in metadata['resolve']['nodes']}
pins = {'tokio-postgres': '0.7.18', 'tokio-postgres-rustls': '0.14.0',
        'rustls': '0.23.45', 'rustls-webpki': '0.103.15', 'webpki-roots': '1.0.9',
        'tokio': '1.53.1', 'tokio-rustls': '0.26.5', 'ring': '0.17.14',
        'postgres-protocol': '0.6.12', 'postgres-types': '0.2.14', 'rcgen': '0.14.10'}
for name, version in pins.items():
    assert by_name[name]['version'] == version, name
for name in by_name:
    assert not any(name == d or name.startswith(d + '-') for d in (
        'sqlx', 'native-tls', 'openssl', 'rustls-native-certs', 'security-framework',
        'aws-lc', 'libsqlite3', 'ledgerlab-core', 'ledgerlab-testkit',
    )), name
connector_features = nodes[by_name['tokio-postgres-rustls']['id']]['features']
assert connector_features == ['ring'], connector_features
assert 'ring' in nodes[by_name['rustls']['id']]['features']

files = {
    'tokio-postgres': ['Cargo.toml', 'src/config.rs', 'src/connect.rs',
        'src/connect_raw.rs', 'src/connect_tls.rs', 'src/transaction.rs',
        'src/transaction_builder.rs', 'src/cancel_query.rs', 'src/cancel_query_raw.rs',
        'src/cancel_token.rs', 'src/query.rs'],
    'tokio-postgres-rustls': ['Cargo.toml', 'src/lib.rs'],
    'rustls': ['src/client/builder.rs', 'src/webpki/anchors.rs', 'src/webpki/server_verifier.rs'],
}
source_hashes = []
source = {}
for name, paths in files.items():
    root = Path(by_name[name]['manifest_path']).parent
    for relative in paths:
        data = (root / relative).read_bytes()
        source[name, relative] = data.decode()
        source_hashes.append({'package': name, 'version': by_name[name]['version'],
                             'path': relative, 'sha256': hashlib.sha256(data).hexdigest()})
config = source['tokio-postgres', 'src/config.rs']
constructor = config.split('pub fn new() -> Config {', 1)[1].split('/// Sets the user', 1)[0]
assert not re.search(r'\benv\b|whoami|pgpass|read_to_string', constructor)
assert 'options: None' in constructor and 'ssl_mode: SslMode::Prefer' in constructor
raw = source['tokio-postgres', 'src/connect_raw.rs']
assert 'Some(user) => Cow::Borrowed(user)' in raw
assert 'None => Cow::Owned(whoami::username()' in raw
connector = source['tokio-postgres-rustls', 'src/lib.rs'].split('#[cfg(test)]\nmod tests', 1)[0]
assert 'ServerName::try_from(self.0.hostname)' in connector
assert 'self.0.connector.connect(hostname, stream)' in connector
assert 'config: Arc::new(config)' in connector
assert 'connector: Arc::clone(&self.config).into()' in connector
assert 'if SslMode::Require == mode' in source['tokio-postgres', 'src/connect_tls.rs']
assert 'self.done = true;' in source['tokio-postgres', 'src/transaction.rs']
assert 'RollbackIfNotDone' in source['tokio-postgres', 'src/transaction_builder.rs']
assert 'self.client.__private_api_rollback(None)' in source['tokio-postgres', 'src/transaction_builder.rs']

own = Path(__file__).parent
for filename in ('connect.rs', 'tls.rs', 'tx.rs'):
    text = (own / filename).read_text()
    assert not re.search(r'\b(?:std::env|std::fs|NoTls|dangerous|set_certificate_verifier|Prefer)\b', text), filename
assert '.ssl_mode(SslMode::Require)' in (own / 'connect.rs').read_text()
assert 'RootCertStore::empty()' in (own / 'tls.rs').read_text()

report = {
    'status': 'PASS: supported connector and explicit trust/environment boundaries',
    'resolved_pins': pins,
    'source_sha256': source_hashes,
    'packages': [{'name': p['name'], 'version': p['version'], 'license': p['license'],
                  'features': nodes.get(p['id'], {}).get('features', [])}
                 for p in sorted(packages.values(), key=lambda p: (p['name'], p['version']))],
    'limits': ['Synthetic protocol peers, not real PostgreSQL',
               'No native platform/read-only-root certification',
               'Root membership proof replaces public-CA-issued server fixture',
               'Separate dated RustSec review documented in ADR 022'],
}
(manifest.parent / 'source-audit.json').write_text(json.dumps(report, indent=2) + '\n')
print(f"PASS: {len(packages)} resolved packages; exact pins, TLS features and source boundaries.")
print('Source hashes, features and license inventory: work/postgres-driver-proof/source-audit.json')
