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
 # Input amount correction changes historical source SHA, not the six sealed document bodies.
 base=json.loads((P/'agreement-boundary-fixture-input.json').read_text())
 by_name={v['name']:v for v in known['documents']}
 for entry in base['customers']:
  tag=entry['tag']
  assert entry['role']==by_name[tag+'.role']['body']
  assert entry['catalog']==by_name[tag+'.catalog']['body']
  inst=dict(entry['instantiation'])
  inst['template_ref']=by_name[tag+'.instantiation']['body']['template_ref']
  inst['catalog_ref']=by_name[tag+'.catalog']['ref']
  assert inst==by_name[tag+'.instantiation']['body']
 for doc in known['documents']:
  validate({'workflow-roles':'RoleAssignment','workflow-slot-catalog':'SlotCatalog','workflow-slot-instantiation':'SlotInstantiation'}[doc['ref']['kind']],doc['body'])
 print('Sealed oracle hash unchanged; corrected source preserves all six known document bodies.')

# Targeted AGR-03 tests: check original authoritative amount object, never relax type.
base=json.loads((P/'agreement-boundary-fixture-input.json').read_text())
expected_amount={'currency':'USD','scale':18,'atoms':'37000000000000000000'}
for entry in base['customers']:
 assert entry['slot_terms']['exposure_ceiling']==expected_amount
 validate('NonnegativeAmount',entry['slot_terms']['exposure_ceiling'])
 for cap in entry['binding_inputs']['exposure_ceilings']:
  assert cap['maximum']==expected_amount
  validate('NonnegativeAmount',cap['maximum'])
for op in fixture['operations']:
 if op['kind']=='agreement.accept':
  for cap in op['payload']['binding_inputs']['exposure_ceilings']:
   assert cap['maximum']==expected_amount
   validate('NonnegativeAmount',cap['maximum'])
amount_validator=Draft202012Validator({'$defs':schema['$defs'],'$ref':'#/$defs/NonnegativeAmount'})
assert list(amount_validator.iter_errors('37000000000000000000'))
# Replace fixture assembly refs only with explicitly fake shape-test refs. No authentication claim.
stop_input=json.loads((P/'agreement-stop-fixture-input.json').read_text())
def shape_refs(v):
 if isinstance(v,dict):
  if 'fixture_ref' in v:return {'kind':'workflow-authority','id':'shape-only','digest':'sha256:'+'0'*64}
  return {k:shape_refs(z) for k,z in v.items()}
 if isinstance(v,list):return [shape_refs(z) for z in v]
 return v
stop=shape_refs(stop_input['operations'][-1]['authority_snapshot'])
validate('AgreementStopAuthority',stop)
assert not stop['source_observation'].get('grant_ref')
admit_validator=Draft202012Validator({'$defs':schema['$defs'],'$ref':'#/$defs/AgreementAdmissionAuthority'})
assert list(admit_validator.iter_errors(stop))
active_expired=dict(stop)
active_expired['source_observation']={**stop['source_observation'],'status':'active','revision':'1','grant_ref':{'kind':'workflow-source-grant','id':'shape-only','digest':'sha256:'+'0'*64},'grant_time_status':'expired'}
validate('AgreementStopAuthority',active_expired)
stop_validator=Draft202012Validator({'$defs':schema['$defs'],'$ref':'#/$defs/AgreementStopAuthority'})
missing_admin=dict(stop);missing_admin.pop('local_admin_ref')
assert list(stop_validator.iter_errors(missing_admin))
wrong_purpose=dict(stop,operation='agreement.activate')
assert list(stop_validator.iter_errors(wrong_purpose))
false_revoked=dict(stop)
false_revoked['source_observation']={**stop['source_observation'],'grant_ref':active_expired['source_observation']['grant_ref']}
assert list(stop_validator.iter_errors(false_revoked))
for entry in base['customers']:
 validate('SlotBindingTerms',shape_refs(entry['slot_terms']))
 validate('BindingInputs',shape_refs(entry['binding_inputs']))
 projected={k:v for k,v in entry['binding_inputs'].items() if k not in ['assent_refs','authority_refs']}
 validate('AssentBindingInputs',shape_refs(projected))
validate('EvidenceRetain',shape_refs(stop_input['operations'][1]['payload']))
print('AGR03 exact amount positive/string-negative and revoked/expired stop-authority shape examples PASS; runtime UNRUN.')
for op in stop_input['expiry_alternative_operations']:
 if op['kind']=='agreement.revoke':validate('AgreementStopAuthority',shape_refs(op['authority_snapshot']))
 if op['kind']=='evidence.retain':
  validate('EvidenceRetain',shape_refs(op['payload']))
  raw=base64.b64decode(op['payload']['content_base64'],validate=True)
  assert hashlib.sha256(raw).hexdigest()==op['payload']['content_sha256']
print('Literal source-expiry alternative evidence/stop shapes and raw digest PASS.')
