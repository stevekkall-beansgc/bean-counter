#!/usr/bin/env python3
"""Read-only catalog/descriptor verification. Author derivation, not independent proof."""
import hashlib,json
from pathlib import Path
p=Path(__file__).resolve().parent
j=lambda v:json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
h=lambda v:'sha256:'+hashlib.sha256(v).hexdigest()
load=lambda n:json.loads((p/n).read_text())
x=load('descriptor-expected.json'); b=x['body']
for field,file in [('limits_digest','limits.json'),('mode_matrix_digest','mode-matrix.json'),('contract_manifest_digest','contract-profile.json')]:
 assert b[field]==h(j(load(file))),field
c=load('contract-profile.json')
for f in c['encoding_files']+[c['semantic_amendment']]:
 assert h((p.parent/f['path']).read_bytes())==f['sha256'],f['path']
assert j(b).hex()==x['canonical_body_hex']
d=h(b'bean-counter/workflow-document/1\0'+j([b['schema'],b]))
assert x['ref']=={'kind':'workflow-compiler-descriptor','id':'wdoc_'+d[7:],'digest':d}
try:
 from jsonschema import Draft202012Validator
 schema=load('catalog.schema.json');Draft202012Validator.check_schema(schema)
 for f in ['limits.json','mode-matrix.json','contract-profile.json']:Draft202012Validator(schema).validate(load(f))
 root=json.loads((p.parent/'workflow.schema.json').read_text())
 Draft202012Validator({'$schema':root['$schema'],'$defs':root['$defs'],'$ref':'#/$defs/CompilerDescriptor'}).validate(b)
 print('Catalog metaschema, three closed bodies and descriptor shape match.')
except ImportError: print('jsonschema unavailable; shape verification NOT RUN.')
print('Author catalog digests and descriptor bytes match; no runtime support claim.')
