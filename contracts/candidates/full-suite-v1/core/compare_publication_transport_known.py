#!/usr/bin/env python3
"""Compare author's pre-derived unchanged records to sealed reviewer PARTIAL oracle."""
import hashlib,json,sys
from pathlib import Path
P=Path(__file__).resolve().parent
def cj(x):
 if isinstance(x,dict):return '{'+','.join(cj(k)+':'+cj(x[k]) for k in sorted(x,key=lambda k:k.encode('utf-16-be')))+'}'
 if isinstance(x,list):return '['+','.join(map(cj,x))+']'
 return json.dumps(x,ensure_ascii=False,separators=(',',':'))
known=Path(sys.argv[1]);assert hashlib.sha256(known.read_bytes()).hexdigest()=='aeb25eeed32a48364e340ce3525c8d2d6e1971cf8fb65bab167d23023d86a38d'
k=json.loads(known.read_text());a=json.loads((P/'publication-transport-02-author.json').read_text());records={x['id']:x for x in a['records']}
for x in k['documents']:
 y=records[x['ref']['id']];assert {p:y[p] for p in ['kind','id','digest']}==x['ref'];assert y['body']==x['body'];assert cj(y['body'])==x['canonical_utf8'];assert cj(y['body']).encode().hex()==x['canonical_hex'];assert len(cj(y['body']).encode())==x['body_bytes']
for x,y in zip(k['operations'],a['operations'][:4]):
 assert x['locators']==y['locators'];assert x['request_evidence']['normalized_request']==y['request'];assert x['request_evidence']['original_request_bytes_base64']==y['request_evidence']['original_request_bytes_base64']
assert a['canonical_reads'][0]['response']==k['canonical_read'];assert a['canonical_reads'][0]['wire_bytes']==k['canonical_read_wire_bytes']
print('PASS sealed four document bodies/Refs/UTF8/hex, four requests/locators, exact canonical read; corrected registry changes descriptors/evidence; full27-operation independent comparison pending')
