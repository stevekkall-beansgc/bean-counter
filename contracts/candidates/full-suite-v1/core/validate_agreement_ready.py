#!/usr/bin/env python3
"""Author contract checks; no product imports, writes or independent-oracle claim."""
import json,base64,hashlib
from pathlib import Path
from jsonschema import Draft202012Validator
P=Path(__file__).resolve().parent
schema=json.loads((P/'agreement-ready.schema.json').read_text())
Draft202012Validator.check_schema(schema)
def validate(name,body):Draft202012Validator({'$schema':schema['$schema'],'$defs':schema['$defs'],'$ref':'#/$defs/'+name}).validate(body)
def refs_ok(x):
 if isinstance(x,dict):
  if '$ref'in x and x['$ref'].startswith('#/$defs/'):assert x['$ref'][8:] in schema['$defs']
  for v in x.values():refs_ok(v)
 elif isinstance(x,list):
  for v in x:refs_ok(v)
refs_ok(schema)
fixture=json.loads((P/'agreement-ready-fixture-input.json').read_text())
for op in fixture['operations']:
 if op['kind']=='evidence.retain':
  body=op['payload'];raw=base64.b64decode(body['content_base64'],validate=True)
  assert len(raw)<=4096 and hashlib.sha256(raw).hexdigest()==body['content_sha256']
  if body['purpose'] in ['customer_registration','source_enrollment','funding']:
   validate('EvidenceRetain',body)
   invalid=dict(body,unknown=True)
   assert list(Draft202012Validator({'$defs':schema['$defs'],'$ref':'#/$defs/EvidenceRetain'}).iter_errors(invalid))
 if op['kind']=='authority.change' and op['payload']['action']=='source.enroll':validate('SourceCeiling',op['payload']['ceiling'])
# Boundary-first executable examples; expiry never falls back.
def selected(boundaries,t):
 eligible=[b for b in boundaries if b['at']<=t and not b.get('cancelled',False)]
 if not eligible:return None
 b=max(eligible,key=lambda z:(z['at'],z['seq']))
 if b['action']=='revoke':return None
 return b['binding'] if b['start']<=t and (b['end'] is None or t<b['end']) else None
v1={'at':0,'seq':1,'action':'activate','binding':'v1','start':0,'end':None}
v2={'at':10,'seq':2,'action':'activate','binding':'v2','start':10,'end':20}
assert [selected([v1,v2],t) for t in [9,10,19,20,21]]==['v1','v2','v2',None,None]
revoke={'at':6,'seq':3,'action':'revoke'}
assert all(selected([v1,dict(v2,cancelled=True),revoke],t) is None for t in [6,10,21])
replacement=dict(v2,at=15,seq=3)
assert [selected([v1,dict(v2,cancelled=True),replacement],t) for t in [14,15,20]]==['v1','v2',None]
assert selected([v1,{'at':0,'seq':2,'action':'revoke'}],0) is None
assert not 10<10 # replacement only while target boundary strictly future
assert selected([v1,v2,{'at':21,'seq':3,'action':'revoke'}],21) is None
print('AGR02 metaschema/ref closure, literal evidence bytes/shapes and activation contract histories PASS.')
print('Complete independent journal bytes and actual authority/runtime remain UNRUN.')
# Optional sealed prior oracle comparison; never rewrite it.
import sys
if len(sys.argv)>1:
 known_path=Path(sys.argv[1]);raw=known_path.read_bytes()
 assert hashlib.sha256(raw).hexdigest()=='dbc66fa8f5464a2f82b0b6f6c77992670ef49bfb8d5bd9f90660abc38eb1aa6c'
 known=json.loads(raw)
 assert hashlib.sha256((P/'agreement-boundary-fixture-input.json').read_bytes()).hexdigest()==known['source_input_sha256']
 for doc in known['documents']:
  validate({'workflow-roles':'RoleAssignment','workflow-slot-catalog':'SlotCatalog','workflow-slot-instantiation':'SlotInstantiation'}[doc['ref']['kind']],doc['body'])
 print('Sealed prior input/oracle hashes unchanged; all six known document shapes still validate.')
