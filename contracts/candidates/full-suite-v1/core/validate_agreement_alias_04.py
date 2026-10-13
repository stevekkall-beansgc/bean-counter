#!/usr/bin/env python3
"""Author source-only shape/history/accounting checks; no runtime or independent proof."""
import copy,json,base64,runpy
from pathlib import Path
from jsonschema import Draft202012Validator
P=Path(__file__).resolve().parent
m=runpy.run_path(str(P/'derive_agreement_alias_04_author.py'),run_name='alias_checks')
s=json.loads((P/'agreement-alias-04.schema.json').read_text());Draft202012Validator.check_schema(s)
checks=0
def ok(v):
 global checks
 assert v;checks+=1
def val(n,x):
 Draft202012Validator({'$defs':s['$defs'],'$ref':'#/$defs/'+n}).validate(x)
 global checks
 checks+=1
def bad(n,x):ok(bool(list(Draft202012Validator({'$defs':s['$defs'],'$ref':'#/$defs/'+n}).iter_errors(x))))
def rejects(fn):
 try:fn()
 except (AssertionError,ValueError):return ok(True)
 raise AssertionError('accepted invalid case')
def branch(actor,request,original,same_key=False):
 # Contract state-machine model only. Actor comes from harness, never request.
 assert actor['current_read']
 if same_key:
  assert m['intent'](request['payload'])==m['intent'](original['normalized_request']['payload'])
  return 'original_retry'
 assert actor['current_administrative_submit'] and actor['mapping_matches']
 if m['intent'](request['payload'])!=m['intent'](original['normalized_request']['payload']):return 'semantic_conflict'
 return 'semantic_alias'
def verify_alias(o,original,envelopes,current_state_ref,heads):
 op=o['operation']['body'];a=op['authority_snapshot'];st=envelopes[current_state_ref['id']]['body'];ob=a['source_observation']
 assert original['operation']['body']['schema']!='ledger-workflow-agreement-alias-operation/1'
 assert a['original_operation_ref']==original['operation']['ref'] and op['result_refs']==original['operation']['body']['result_refs']
 assert o['receipt']['body']['result_refs']==op['result_refs'] and o['heads_before_and_after']==heads
 assert a['operation']==op['kind'] and a['purpose']=='semantic_alias' and a['observed_at']==op['accepted_at']
 assert ob['state_ref']==current_state_ref and ob['enrollment_ref']==st['enrollment_ref'] and ob['revision']==st['revision'] and ob['status']==st['status']
 if st['status']=='revoked':assert 'grant_ref'not in ob
 else:
  assert ob['grant_ref']==st['current_grant_ref'];g=envelopes[ob['grant_ref']['id']]['body'];t=a['observed_at']
  assert ob['grant_time_status']==('not_started'if t<g['starts_at']else'expired'if t>=g['ends_at']else'valid')
for spec,scenario in zip(m['f']['scenarios'],m['output']['scenarios']):
 base=m['ns']['derive'](spec['base_scenario']);named={r['name']:r['ref']for r in base['records']};env={e['ref']['id']:e for e in scenario['all_member_envelopes']}
 for e in env.values():ok(e['canonical_hex']==m['jb'](e['body']).hex())
 for o in scenario['operations']:
  original=next(z for z in base['operations']if z['name']==o['original_name']);a=o['operation']['body']['authority_snapshot'];key=o['operation']['body']['key']
  for n,x in [('AgreementAliasAuthority',a),('AgreementAliasOperation',o['operation']['body']),('AgreementAliasManifest',o['manifest']['body']),('OperationReceipt',o['receipt']['body']),('RequestEvidence',o['request_evidence']),('DescriptorResolution',o['descriptor_resolution']),('OperationEnvelope',o['normalized_request'])]:val(n,x)
  for k in ['operation','manifest','receipt']:
   x=o[k];val('AgreementAliasRecordEnvelope',dict(x['ref'],logical_store_id=key['logical_store_id'],scope=key['scope'],body=x['body']));ok(x['canonical_hex']==m['jb'](x['body']).hex())
  verify_alias(o,original,env,named[spec['current_source_state_name']],spec['heads_before_and_after']);ok(True)
  members=o['manifest']['body']['members'];ok(members==m['unique'](members));ok(len(members)<=4096 and o['resolved_member_body_bytes']<=8388608)
  ok({r['id']for r in members if r['kind']=='workflow-operation'}=={o['operation']['ref']['id'],original['operation']['ref']['id']})
  ok(not any(r['kind']in['workflow-manifest','workflow-receipt']for r in members))
  ok(sum(len(m['jb'](env[r['id']]['body']))for r in members)==o['resolved_member_body_bytes'])
  ok(o['meter']['wb_delta']==len(m['jb'](a))+sum(len(m['jb'](v))for v in[key,o['operation']['body'],o['manifest']['body'],o['receipt']['body']]))
  ok(o['meter']['new_document_count']==1 and o['meter']['units']==1)
  ok(o['original_key_retry']['units']==o['original_key_retry']['wb_delta']==0)
  ok(o['original_administrative_key_retry']['receipt_ref']==original['receipt']['ref'])
  ok(base64.b64decode(o['request_evidence']['original_request_bytes_base64'])==m['jb'](o['normalized_request']))
  actor=m['f']['trusted_actor'];ok(branch(actor,o['normalized_request'],original)=='semantic_alias')
  for field in ['current_read','current_administrative_submit','mapping_matches']:rejects(lambda field=field:branch(dict(actor,**{field:False}),o['normalized_request'],original))
  ok(branch(dict(actor,current_administrative_submit=False,mapping_matches=False),o['normalized_request'],original,True)=='original_retry')
  changed=copy.deepcopy(o['normalized_request']);changed['payload']['expected_revision']='999';ok(branch(actor,changed,original)=='semantic_conflict');rejects(lambda:branch(actor,changed,original,True))
  bad('AgreementAdmissionAuthority',a);bad('AgreementStopAuthority',a)
  bad('AgreementAliasAuthority',dict(a,purpose='admit'));bad('AgreementAliasAuthority',dict(a,source_grant_refs=[]))
  bad('AgreementAliasOperation',dict(o['operation']['body'],authority_snapshot=original['operation']['body']['authority_snapshot']))
  badop=copy.deepcopy(o['operation']['body']);badop['authority_snapshot']['operation']='agreement.revoke'if badop['kind']!='agreement.revoke'else'agreement.accept';bad('AgreementAliasOperation',badop)
  corrupt=copy.deepcopy(o);corrupt['operation']['body']['result_refs']=[];rejects(lambda:verify_alias(corrupt,original,env,named[spec['current_source_state_name']],spec['heads_before_and_after']))
  corrupt=copy.deepcopy(o);corrupt['heads_before_and_after']['A_activation']='1';rejects(lambda:verify_alias(corrupt,original,env,named[spec['current_source_state_name']],spec['heads_before_and_after']))
  fake_original=copy.deepcopy(original);fake_original['operation']['body']['schema']='ledger-workflow-agreement-alias-operation/1';rejects(lambda:verify_alias(o,fake_original,env,named[spec['current_source_state_name']],spec['heads_before_and_after']))
  corrupt=copy.deepcopy(a)
  if spec['name']=='revoked':corrupt['source_observation']['grant_ref']=a['binding_ref'];bad('AgreementAliasAuthority',corrupt)
  else:
   corrupt['source_observation'].pop('grant_ref');bad('AgreementAliasAuthority',corrupt)
   corrupt=copy.deepcopy(o);corrupt['operation']['body']['authority_snapshot']['source_observation']['grant_time_status']='valid';rejects(lambda:verify_alias(corrupt,original,env,named[spec['current_source_state_name']],spec['heads_before_and_after']))
 ok(scenario['total_wb_delta']==sum(o['meter']['wb_delta']for o in scenario['operations']))
print(json.dumps({'author_checks':checks,'scenarios':2,'alias_operations':6,'runtime':'UNRUN','independent_review':'PENDING'}))
