#!/usr/bin/env python3
"""Check only the explicitly scoped PUB-TRANSPORT-02 snapshot hashes."""
import hashlib,json,pathlib
p=pathlib.Path(__file__).resolve().parent
f=json.loads((p/'publication-transport-02-encoding-freeze.json').read_text())
for name,expected in f['sha256'].items():assert hashlib.sha256((p/name).read_bytes()).hexdigest()==expected,name
print('PUB-TRANSPORT-02 selected encoding snapshot hashes match; runtime/full Wave gates remain pending')
