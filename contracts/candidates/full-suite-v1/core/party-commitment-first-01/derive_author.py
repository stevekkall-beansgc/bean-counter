#!/usr/bin/env python3
"""Independent author conditional derivation; authored after root's input-first seal.
Never imports independent reviewer code/results or any product code.
"""
import base64,copy,hashlib,json
from pathlib import Path
import jsonschema
P=Path(__file__).resolve().parent
J=lambda x:json.dumps(x,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()
SHA=lambda b:hashlib.sha256(b).hexdigest()
def H(d,x):return 'sha256:'+SHA(('bean-counter/'+d+'/1').encode()+b'\0'+J(x))
def load(n):return json.loads((P/n).read_text())
F=load('fixture-input.json'); SC=load('party-commitment-first.schema.json')
def resolve(x,d):
 if isinstance(x,list):return [resolve(y,d) for y in x]
 if isinstance(x,dict):
  if len(x)==1:
   k,v=next(iter(x.items()))
   if k=='$record':return copy.deepcopy(d[v]['ref'])
   if k=='$record_id':return d[v]['ref']['id']
   if k=='$body':return copy.deepcopy(d[v]['body'])
   if k=='$field':return copy.deepcopy(d[v[0]]['body'][v[1]])
   if k=='$terms_digest':return H('workflow-acceptance-terms',d[v]['body'])
   if k=='$digest':return d[v]['ref']['digest']
   if k=='$raw_sha':return d[v]['raw_sha256']
   if k=='$id':return v[1]+H(v[0],resolve(v[2],d))[7:]
  return {k:resolve(v,d) for k,v in x.items()}
 return x
def sets(xs):return [json.loads(k) for k in sorted({J(x) for x in xs})]
def build(rows):
 d={}
 for r in rows:
  b=resolve(r['body'],d)
  for path in r['set_paths']:
   at=b
   for k in path[:-1]:at=at[k]
   at[path[-1]]=sets(at[path[-1]])
  i=r['identity']
  if i['type']=='document':dg=H('workflow-document',[b['schema'],b]);rid='wdoc_'+dg[7:]
  elif i['type']=='plan':dg=H('workflow-plan',b);rid='wplan_'+dg[7:]
  else:dg=H('workflow-record',[r['kind'],F['scope'],b]);rid=i['prefix']+H(i['domain'],resolve(i['tuple'],d))[7:]
  d[r['name']]={'body':b,'canonical_hex':J(b).hex(),'raw_sha256':SHA(J(b)),'ref':{'kind':r['kind'],'id':rid,'digest':dg}}
 return d
def valid(k,b):return not any(jsonschema.Draft202012Validator({'$ref':'#/$defs/'+k,'$defs':SC['$defs']}).iter_errors(b))
def patch(x,path,v):
 for k in path[:-1]:x=x[k]
 x[path[-1]]=copy.deepcopy(v)
def closure(d,roots):
 lookup={J(v['ref']):v['body'] for v in d.values()};seen={};external={};commitments={};queue=list(roots)
 def refs(x,commit=False):
  if isinstance(x,dict):
   if set(x)=={'kind','id','digest'}:
    if commit:commitments[J(x)]=x
    else:queue.append(x)
   else:
    for v in x.values():refs(v,commit)
  elif isinstance(x,list):
   for v in x:refs(v,commit)
 while queue:
  ref=queue.pop();key=J(ref)
  if key in seen:continue
  seen[key]=ref
  if key not in lookup:external[key]=ref;continue
  b=copy.deepcopy(lookup[key])
  if b.get('schema')=='ledger-workflow-party-evidence/2' and b['metadata']['purpose']=='party_consent':
   assert valid('PartyEvidence2',b)
   refs(b['metadata']['consent'].pop('basis'),True)
  refs(b)
 return {'roots':sets(roots),'reachable_refs':sets(seen.values()),'external_refs':sets(external.values()),'commitment_refs':sets(commitments.values())}
def shapes_material(rows,d):
 for r in rows:
  if not valid(r['definition'],d[r['name']]['body']):return False
  b=d[r['name']]['body']
  if r['definition'] in ['PartyEvidence1','PartyEvidence2']:
   try:
    raw=base64.b64decode(b['content_base64'],validate=True)
    if not 1<=len(raw)<=4096 or base64.b64encode(raw).decode()!=b['content_base64'] or SHA(raw)!=b['content_sha256']:return False
   except (ValueError,TypeError):return False
 return True
def auth_relations(d,c,attest=False):
 B=lambda n:d[n]['body'];R=lambda n:d[n]['ref'];a=B('platform_authorization');A=a['authorization'];e=B('platform_consent_evidence');E=e['metadata']['consent'];m=B('platform_mapping_evidence');ma=B('platform_mapping_attestation')
 ident={'customer':'customer-B','agreement_id':'B-support','party':'platform-P','acceptor':'platform-acceptor'}
 if any(A[k]!=v or E[k]!=v or e['subject'][k]!=v for k,v in ident.items()):return False
 if A['scope']!=F['scope'] or E['scope']!=F['scope'] or A['roles']!=['bearer','payer'] or E['roles']!=A['roles']:return False
 if a['revision']!='1' or a['status']!='active' or E['authorization_ref']!=R('platform_authorization') or c['original_authorization_ref']!=R('platform_authorization'):return False
 if m['metadata']['authorization']!=A or ma['metadata']['target_evidence_ref']!=R('platform_mapping_evidence') or ma['metadata']['statement']!='operator_verified_party_acceptor_authority' or a['evidence_ref']!=R('platform_mapping_evidence') or a['operator_attestation_ref']!=R('platform_mapping_attestation'):return False
 if not (A['starts_at']<=E['accepted_at']<=c['retention_at']<A['ends_at'] and a['recorded_at']<=E['accepted_at']):return False
 if e['retained_at']!=c['retention_at'] or e['retained_by']!='fixture-admin' or e['scope']!=F['scope'] or e['installation_mode']!=c['mode']:return False
 if not (c['scope']==F['scope'] and c['customer']=='customer-B' and c['agreement_id']=='B-support' and c['mode']=='isolated_synthetic'):return False
 if not valid('PartyCommitmentBasis1',E['basis']) or E['basis']['terms_ref']['kind']!='workflow-acceptance-terms':return False
 if attest:
  t=B('platform_consent_attestation')
  if t['subject']!=e['subject'] or t['metadata']['target_evidence_ref']!=R('platform_consent_evidence') or t['metadata']['statement']!='operator_verified_party_exact_consent' or t['retained_by']!='fixture-admin' or t['installation_mode']!=c['mode'] or t['scope']!=F['scope'] or t['retained_at']!=c['retention_at']:return False
 return True
def acceptance_relations(d,c):
 B=lambda n:d[n]['body'];R=lambda n:d[n]['ref'];s=B('platform_assent');e=B('platform_consent_evidence');E=e['metadata']['consent'];a=B('platform_authorization')['authorization'];t=B('acceptance_terms');binding=B('binding');v=B('consent_verification')
 if not auth_relations(d,c,True):return False
 if s['consent']!=E or s['evidence_ref']!=R('platform_consent_evidence') or s['operator_attestation_ref']!=R('platform_consent_attestation') or s['operator']!='fixture-admin' or s['mode']!='synthetic':return False
 if not a['starts_at']<=c['acceptance_at']<a['ends_at'] or c['retention_at']>c['acceptance_at']:return False
 if v['verified_at']!=c['acceptance_at'] or binding['accepted_at']!=c['acceptance_at'] or binding['accepted_by']!='fixture-admin':return False
 dg=H('workflow-acceptance-terms',t)
 if c['resolved_terms_ref']!=R('acceptance_terms') or c['resolved_terms_digest']!=dg or E['basis']['terms_ref']!=R('acceptance_terms') or E['basis']['terms_digest']!=dg or B('customer_assent')['terms_digest']!=dg:return False
 if t['scope']!=F['scope'] or t['publication_ref']!=R('publication'):return False
 for k,x in t['binding_inputs'].items():
  if binding.get(k)!=x:return False
 if any(binding[k]!=t[k] for k in ['customer','agreement_id','version']):return False
 if binding['policy_ref']!=R('policy') or binding['compiled_plan_ref']!=R('plan') or B('publication')['compiled_plan_ref']!=R('plan') or B('publication')['root_policy_ref']!=R('policy'):return False
 leg=next(x for x in binding['funding_legs'] if x['name']=='support:model')
 if leg['mode']!='platform' or leg['resource_owner']!=E['party'] or leg['payer']!=E['party']:return False
 expected={'publication_ref':R('publication'),'compiled_plan_ref':R('plan'),'app_terms_refs':[R('app_terms')],'acceptance_terms_ref':R('acceptance_terms'),'customer_assent_refs':[R('customer_assent')],'party_assent_refs':[R('platform_assent')],'original_party_authorization_refs':[R('platform_authorization')],'current_party_authorization_refs':[R('platform_authorization')],'verified_by':'fixture-admin','installation_mode':c['mode'],'scope':F['scope'],'customer':'customer-B','agreement_id':'B-support'}
 if any(v[k]!=x for k,x in expected.items()):return False
 if c['closure_omitted_ref'] in closure(d,resolve(F['closure_roots'],d))['reachable_refs']:return False
 return True
BASE=build(F['records']);assert shapes_material(F['records'],BASE)
CASES={}
for case in F['cases']:
 rows=copy.deepcopy(F['records']);byname={r['name']:r for r in rows};c=copy.deepcopy(F['context'])
 for m in case['mutations']:patch(byname[m['record']]['body'],m['path'],resolve(m['value'],BASE))
 c.update(resolve(case['context_patch'],BASE));c['stage']=case['stage'];d=build(rows);c=resolve(c,d)
 out={'accepted':False,'disposition':'refused','derived_refs':[],'audited_data_refs':[],'commitment_refs':[]}
 try:
  ok=shapes_material(rows,d);stage=c['stage'];R=lambda n:d[n]['ref'];cl=None
  if stage.startswith('replay_'):
   ok=ok and all(c[k] for k in ['exact_original_replay','saved_receipt_verified','retry_intent_equal','read_authorized','authorization_origin_complete','mapping_evidence_complete','consent_origin_complete'])
   if stage=='replay_acceptance':ok=ok and c['full_basis_available_at_acceptance'] and acceptance_relations(d,c)
   if ok:out.update(accepted=True,disposition='saved_replay')
  elif stage in ['retain','attest']:
   flags=['local_admin_authorized','read_authorized','clock_monotone','authorization_origin_complete','authorization_present_before_retention','authorization_current_at_retention','mapping_evidence_complete']
   if stage=='attest':flags+=['consent_target_present_before_attestation','consent_origin_complete']
   ok=ok and c['actor']=='fixture-admin' and all(c[k] for k in flags) and auth_relations(d,c,stage=='attest')
   if ok:
    name='platform_consent_evidence' if stage=='retain' else 'platform_consent_attestation';cl=closure(d,[R(name)]);out.update(accepted=True,disposition='retained_commitment' if stage=='retain' else 'retained_attestation',derived_refs=[R(name)])
  elif stage=='accept':
   flags=['local_admin_authorized','read_authorized','clock_monotone','authorization_origin_complete','authorization_present_before_retention','authorization_current_at_retention','authorization_current_at_acceptance','mapping_evidence_complete','consent_target_present_before_attestation','consent_origin_complete','full_basis_available_at_acceptance','complete_economic_validation','current_original_sources_valid','capacity_resource_complete']
   ok=ok and c['actor']=='fixture-admin' and all(c[k] for k in flags) and c['current_authorization_ref']==R('platform_authorization') and acceptance_relations(d,c)
   if ok:cl=closure(d,resolve(F['closure_roots'],d));out.update(accepted=True,disposition='accepted_terms',derived_refs=sets([R('binding'),R('consent_verification'),R('platform_assent')]))
  if cl:out.update(audited_data_refs=cl['reachable_refs'],commitment_refs=cl['commitment_refs'])
 except (KeyError,TypeError,ValueError,AssertionError,StopIteration):pass
 CASES[case['name']]=out
NEG={}
for i,n in enumerate(F['schema_negatives']):
 b=copy.deepcopy(BASE[n['from_record']]['body'])
 if n.get('payload_projection'):b={k:v for k,v in b.items() if k in ['subject','media_type','content_base64','content_sha256','metadata']};b['schema']='ledger-workflow-party-evidence-retain/2'
 if 'path'in n:patch(b,n['path'],resolve(n['patch'],BASE))
 else:b.update(resolve(n['patch'],BASE))
 NEG[str(i)]={'accepted':valid(n['definition'],b)}
assert all(not x['accepted'] for x in NEG.values())
OUT={'records':BASE,'terms_digest':H('workflow-acceptance-terms',BASE['acceptance_terms']['body']),'closure':closure(BASE,resolve(F['closure_roots'],BASE)),'retention_closure':closure(BASE,[BASE['platform_consent_evidence']['ref']]),'cases':CASES,'schema_negatives':NEG,'metadata':{n+'_jcs_sha256':SHA(J(load(file))) for n,file in [('profile','profile.json'),('registry','registry.json'),('schema','party-commitment-first.schema.json')]},'qualification':F['qualification']}
if __name__=='__main__':
 (P/'fixture-expected.json').write_text(json.dumps(OUT,indent=2)+'\n')
 print(json.dumps({'records':len(BASE),'cases':len(CASES),'accepts':[k for k,v in CASES.items() if v['accepted']],'sha256':SHA((P/'fixture-expected.json').read_bytes())},indent=2))
