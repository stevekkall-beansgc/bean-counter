#!/usr/bin/env python3
"""Independent local-recipe conditional vectors. No production eligibility API."""
import base64,copy,hashlib,json
from datetime import datetime,timedelta
from fractions import Fraction
from pathlib import Path
from jsonschema import Draft202012Validator
P=Path(__file__).resolve().parent

def read(n):return json.loads((P/n).read_text())
def j(v):return json.dumps(v,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def raw(b):return hashlib.sha256(b).hexdigest()
def h(d,v,n=1):return 'sha256:'+raw(('bean-counter/'+d+'/'+str(n)).encode()+b'\0'+j(v))
def ordered(xs):return [json.loads(x) for x in sorted(set(j(x) for x in xs))]
def entry(body,ref=None):
 o={'body':body,'canonical_hex':j(body).hex(),'raw_sha256':raw(j(body))}
 if ref is not None:o['ref']=ref
 return o

def derive():
 source=read('fixture-input.json');schema=read('local-execution.schema.json');registry=read('registry.json');profile=read('profile.json');records={}
 def validate(name,value):Draft202012Validator({'$ref':'#/$defs/'+name,'$defs':schema['$defs']}).validate(value)
 def resolve(v):
  if isinstance(v,list):return [resolve(x) for x in v]
  if isinstance(v,dict):
   if set(v)=={'$record'}:return copy.deepcopy(records[v['$record']]['ref'])
   if set(v)=={'$hash'}:return h(v['$hash']['domain'],resolve(v['$hash']['value']))
   return {k:resolve(x) for k,x in v.items()}
  return v
 for row in source['records']:
  body=resolve(row['body']);validate(row['definition'],body);identity=row['identity']
  if identity['type']=='document':
   digest=h('workflow-document',[body['schema'],body]);ident='wdoc_'+digest[7:]
  else:
   ident=identity['prefix']+h(identity['domain'],resolve(identity['tuple']))[7:];digest=h('workflow-record',[row['kind'],source['scope'],body])
  records[row['name']]=entry(body,{'kind':row['kind'],'id':ident,'digest':digest})
 ji=source['journal_input'];payload=resolve(ji['payload']);validate('AppCorrect2',payload)
 envelope={'schema':'ledger-operation/1','key':source['key'],'kind':ji['operation_kind'],'contract':'workflow/1','payload':payload}
 anchors=ordered(resolve(ji['direct_anchors']));locators=ordered([{'role':a['role'],'ref':a['record_ref']} for a in anchors])
 descriptor={'schema':'ledger-operation-descriptors/8','registry_digest':'sha256:'+raw(j(registry)),'operation_kind':ji['operation_kind'],'payload_schema':payload['schema'],'locator_digest':h('descriptor-locators',[ji['operation_kind'],payload['schema'],locators]),'anchors':anchors,'descriptors':[],'normalization_profile':'ledger-operation-normalization/2'}
 descriptor['closure_digest']=h('descriptor-closure',descriptor);validate('LocalDescriptor8',descriptor)
 evidence={'schema':'ledger-operation-request-evidence/2','normalization_profile':'ledger-operation-normalization/2','original_request_bytes_base64':base64.b64encode(j(envelope)).decode(),'original_request_bytes_sha256':raw(j(envelope)),'normalized_request':envelope,'request_hash':h('operation-request',[descriptor['closure_digest'],envelope],2),'schema_bundle_digest':h('schema-bundle',[]),'descriptor_resolution':descriptor};validate('LocalRequestEvidence4',evidence)
 results=ordered([records[name]['ref'] for name in ji['result_records']])
 operation={'schema':'ledger-workflow-economic-operation/10','key':source['key'],'kind':ji['operation_kind'],'contract':'workflow/1','request_evidence':evidence,'authority_snapshot':records['authority']['body'],'received_at':ji['received_at'],'accepted_at':ji['accepted_at'],'commit_sequence':ji['commit_sequence'],'result_refs':results,'closure_profile':'ledger-local-selected-inputs/1'};validate('LocalOperation10',operation)
 opref={'kind':'workflow-operation','id':'wop_'+h('workflow-operation',source['key'])[7:],'digest':h('workflow-record',['workflow-operation',source['scope'],operation])}
 manifest={'schema':'ledger-workflow-economic-manifest/10','key':source['key'],'members':ordered(resolve(ji['selected_input_refs'])+results+[opref]),'closure_profile':'ledger-local-selected-inputs/1'};validate('LocalManifest10',manifest)
 md=h('workflow-manifest',manifest);mref={'kind':'workflow-manifest','id':'wmanifest_'+md[7:],'digest':md}
 receipt={'schema':'ledger-operation-receipt/2','key':source['key'],'request_hash':evidence['request_hash'],'request_hash_profile':'bean-counter/operation-request/2','normalization_profile':'ledger-operation-normalization/2','schema_bundle_digest':evidence['schema_bundle_digest'],'descriptor_closure_digest':descriptor['closure_digest'],'original_request_bytes_sha256':evidence['original_request_bytes_sha256'],'result_kind':'economic','result_refs':results,'accepted_at':ji['accepted_at'],'commit_sequence':ji['commit_sequence'],'manifest_hash':md}
 receipt['receipt_hash']=h('operation-receipt',receipt,2);validate('OperationReceipt',receipt)
 rref={'kind':'workflow-receipt','id':'wreceipt_'+receipt['receipt_hash'][7:],'digest':receipt['receipt_hash']}
 def disposition(patch):
  c=source['context']|patch
  def out(a,d,m,w):return {'accepted':a,'disposition':d,'may_have_committed':m,'writes_expected':w}
  refused=out(False,'refused',False,0)
  if not all(c[k] for k in ['registry_matches','selected_bodies_verified','read_right']):return refused
  if c['retry']:
   if not all(c[k] for k in ['same_intent','original_receipt_verified','original_terms']):return refused
   return out(True,'saved_original',True,0)
  if not all(c[k] for k in ['all_policy_executors','heads_current','current_rights','original_terms','once_only','financial_exposure','complete_dependency_actions','complete_result_effects']):return refused
  if c['pending']:
   if not c['held_claim_absent']:return refused
  else:
   now=datetime.fromisoformat(c['now'].replace('Z','+00:00'));t0=datetime.fromisoformat(c['initial_cost_accepted_at'].replace('Z','+00:00'))
   if now<t0 or now>t0+timedelta(microseconds=int(c['correction_within_us'])):return refused
  if not c['actual_resource_guard']:return refused
  if c['commit_outcome']=='unknown':return out(False,'unknown',True,None)
  return out(True,'committed' if c['reply_delivered'] else 'committed_reply_lost',True,1)
 cases={c['name']:disposition(c['patch']) for c in source['cases']}
 mi=source['money_inputs'];scale=10**mi['scale'];mult=Fraction(int(mi['multiplier']['numerator']),int(mi['multiplier']['denominator']))
 def series(service):
  service=Fraction(service);cost=Fraction(mi['final_cost'])*mult+Fraction(mi['markup']);newcost=Fraction(mi['replacement_cost'])*mult+Fraction(mi['markup']);success=Fraction(mi['success_percent'])/100;replacement=Fraction(mi['replacement_outcome_percent'])/100
  totals=[service+cost+service*success,service+cost+service*replacement,service+newcost+service*replacement];atoms=[]
  for x in totals:
   x*=scale;assert x.denominator==1;atoms.append(x.numerator)
  return {'totals_atoms':[str(x) for x in atoms],'deltas_atoms':[str(atoms[0])]+[str(atoms[n]-atoms[n-1]) for n in range(1,len(atoms))]}
 negatives={}
 for n,neg in enumerate(source['schema_negatives']):
  row=next(r for r in source['records'] if r['name']==neg['record']);body=copy.deepcopy(records[neg['record']]['body']);body[neg['add_field']]=neg['value'];v=Draft202012Validator({'$ref':'#/$defs/'+row['definition'],'$defs':schema['$defs']});assert not v.is_valid(body);negatives[str(n)]={'accepted':False}
 oldneg={}
 for name,body in [('AppAuthority7',records['authority']['body']),('AppOperation9',operation),('AppManifest9',manifest)]:
  assert not Draft202012Validator({'$ref':'#/$defs/'+name,'$defs':schema['$defs']}).is_valid(body);oldneg[name]={'accepted':False}
 return {'records':records,'journal':{'descriptor':entry(descriptor),'evidence':entry(evidence),'operation':entry(operation,opref),'manifest':entry(manifest,mref),'receipt':entry(receipt,rref)},'cases':cases,'money':{'base':series(mi['service']),'variant':series(mi['policy_variant_service'])},'schema_negatives':negatives,'old_shape_negatives':oldneg,'metadata':{'profile_jcs_sha256':raw(j(profile)),'registry_jcs_sha256':raw(j(registry)),'schema_jcs_sha256':raw(j(schema))},'qualification':'conditional_only_no_runtime_admission'}
if __name__=='__main__':
 result=derive();(P/'fixture-expected.json').write_text(json.dumps(result,indent=2)+'\n');print('author conditional output',raw((P/'fixture-expected.json').read_bytes()))
