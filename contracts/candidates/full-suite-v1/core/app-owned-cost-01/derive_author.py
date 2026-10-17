#!/usr/bin/env python3
"""Independent author derivation from pinned literal inputs; no product execution."""
import base64, copy, hashlib, json
from decimal import Decimal
from fractions import Fraction
from pathlib import Path
import jsonschema
P=Path(__file__).resolve().parent

def j(x):return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def sha(x):return hashlib.sha256(x).hexdigest()
def h(domain,x):return 'sha256:'+sha(('bean-counter/'+domain+'/1').encode()+b'\0'+j(x))
def load(n):return json.loads((P/n).read_text())
F=load('fixture-input.json'); SC=load('app-owned-cost.schema.json')
def resolve(x,done):
 if isinstance(x,list):return [resolve(v,done) for v in x]
 if isinstance(x,dict):
  if len(x)==1:
   k,v=next(iter(x.items()))
   if k=='$record':return copy.deepcopy(done[v]['ref'])
   if k=='$body':return copy.deepcopy(done[v]['body'])
   if k=='$record_id':return done[v]['ref']['id']
   if k=='$field':return copy.deepcopy(done[v[0]]['body'][v[1]])
   if k=='$raw_sha':return done[v]['raw_sha256']
   if k=='$digest':return done[v]['ref']['digest']
   if k=='$terms_digest':return h('workflow-acceptance-terms',done[v]['body'])
   if k=='$id':return v[1]+h(v[0],resolve(v[2],done))[7:]
  return {k:resolve(v,done) for k,v in x.items()}
 return x

def build(rows):
 done={}
 for r in rows:
  b=resolve(r['body'],done)
  for path in r['set_paths']:
   q=b
   for key in path[:-1]:q=q[key]
   q[path[-1]]=sorted(q[path[-1]],key=j)
   assert len({j(x) for x in q[path[-1]]})==len(q[path[-1]])
  typ=r['identity']['type'];identity=r['identity']
  if typ=='document':dg=h('workflow-document',[b['schema'],b]);rid='wdoc_'+dg[7:]
  elif typ=='plan':dg=h('workflow-plan',b);rid='wplan_'+dg[7:]
  else:rid=identity['prefix']+h(identity['domain'],resolve(identity['tuple'],done))[7:];dg=h('workflow-record',[r['kind'],F['scope'],b])
  done[r['name']]={'body':b,'canonical_hex':j(b).hex(),'raw_sha256':sha(j(b)),'ref':{'kind':r['kind'],'id':rid,'digest':dg}}
 return done

def validation(name,b):
 return not list(jsonschema.Draft202012Validator({'$ref':'#/$defs/'+name,'$defs':SC['$defs']}).iter_errors(b))

def path_patch(x,path,value,delete=False):
 for k in path[:-1]:x=x[k]
 if delete:del x[path[-1]]
 else:x[path[-1]]=value

def decimal(x):return Fraction(Decimal(x))
def amount(x):return int(x['atoms'])
def valid_material(x):
 try:
  b=base64.b64decode(x['content_base64'],validate=True)
  return 0<len(b)<=4096 and base64.b64encode(b).decode()==x['content_base64'] and sha(b)==x['content_sha256']
 except (ValueError,TypeError):return False

def joins(d,c,op):
 B=lambda n:d[n]['body'];R=lambda n:d[n]['ref']
 try:
  obs=B('observation');one=B('final_1');work=B('work');binding=B('binding');terms=B('app_terms');policy=B('policy');acc=B('acceptance_terms');declaration=terms['app_cost_terms'][0]
  if not all(c[k] for k in ['original_work_complete','source_original_authorized','source_current_authorized','platform_fullterms_verified','customer_fullterms_verified','projection_joins_complete','history_complete','resource_and_capacity_complete','attributable_final_material','comparison_truthful']):return False
  if any(c[k] for k in ['call_key_occupied_by_other','component_key_occupied_by_other','provider_key_occupied_by_other','evidence_consumed_by_other']):return False
  if c['logical_store_id']!=F['logical_store_id'] or c['scope']!=F['scope'] or c['target_status']!='active':return False
  subject={'logical_store_id':F['logical_store_id'],'scope':F['scope'],'customer':'customer-B','agreement_id':'B-support','original_work_ref':R('work'),'original_binding_ref':R('binding'),'component_slot':'component:model','component_key':'support:model-retail','app_call':{'source_namespace':work['source_namespace'],'call_id':'call-1'}}
  if obs['subject']!=subject or work['payload']['binding_ref']!=R('binding') or work['payload']['status']!='succeeded':return False
  if obs['source']!='urn:fixture:worker' or work['source_namespace']!='urn:fixture:worker':return False
  if obs['usage_field']!=declaration['usage_field'] or obs['unit']!=declaration['usage_unit'] or obs['observed_at']!=work['payload']['occurred_at']:return False
  if decimal(obs['quantity'])!=decimal(work['payload']['facts'][obs['usage_field']]) or decimal(obs['quantity'])<0 or decimal(obs['quantity'])>decimal(terms['quantity_ceilings'][0]['maximum']):return False
  if declaration!=policy['app_delayed_costs'][0] or terms['funding_leg']!='support:customer' or declaration['resource_funding_leg']!='support:model':return False
  legs={x['name']:x for x in binding['funding_legs']}
  if legs['support:model']['mode']!='platform' or legs['support:model']['resource_owner']!='platform-P' or legs['support:model']['payer']!='platform-P' or 'supplier_binding_slot' in legs['support:model']:return False
  if legs['support:customer']['payer']!='customer-B':return False
  def proof(n):
   v=B(n+'_verification');a=B(n);subs=[R('work'),R('binding')]+([] if n=='observation' else [R('observation')])
   return v['attestation_ref']==R(n) and v['attestation_raw_sha256']==d[n]['raw_sha256'] and v['attestation_digest']==R(n)['digest'] and v['verified_source']==a['source'] and v['subject_binding_refs']==sorted(subs,key=j) and a.get('issued_at',a.get('observed_at'))<=v['trusted_received_at']<=c['now']
  if not valid_material(obs['material']) or not proof('observation'):return False
  def final(n):
   f=B(n);q=f['quantity_comparison'];a=amount(f['amount']);formula=policy['rules'][1]['formula']
   if f['subject']!=subject or f['observation_ref']!=R('observation') or f['source']!='urn:fixture:cost-source' or f['finality']!='final_attributable_amount':return False
   if a<0 or a>min(amount(declaration['maximum_final_amount']),amount(formula['ceiling'])):return False
   if q['kind']=='comparable' and (q['unit']!=obs['unit'] or not 0<=decimal(q['quantity'])<=decimal(obs['quantity'])):return False
   if not valid_material(f['material']) or not proof(n):return False
   return True
  if not final('final_1') or one['cost_revision']!='1' or 'prior_final_evidence_ref' in one:return False
  if op=='correct':
   two=B('final_2')
   if not final('final_2') or c['current_revision']!='1' or two['cost_revision']!=str(int(c['current_revision'])+1) or two['prior_final_evidence_ref']!=c['current_evidence_ref'] or c['current_evidence_ref']!=R('final_1'):return False
  td=h('workflow-acceptance-terms',acc);pa=B('platform_assent');cs=pa['consent'];ce=B('platform_consent_evidence');auth=B('platform_authorization');cv=B('consent_verification')
  if B('customer_assent')['terms_digest']!=td or cs['basis']!={'kind':'acceptance_terms','terms_ref':R('acceptance_terms'),'terms_digest':td}:return False
  if ce['metadata']['consent']!=cs or cs['authorization_ref']!=R('platform_authorization') or cs['party']!='platform-P' or cs['roles']!=['bearer','payer']:return False
  if cs['acceptor']!=auth['authorization']['acceptor'] or auth['status']!='active' or auth['authorization']['party']!=cs['party'] or auth['authorization']['roles']!=cs['roles']:return False
  if not auth['authorization']['starts_at']<=cs['accepted_at']<auth['authorization']['ends_at'] or not cs['accepted_at']<=ce['retained_at']<=c['now']<auth['authorization']['ends_at']:return False
  if cv['publication_ref']!=R('publication') or cv['compiled_plan_ref']!=R('plan') or B('publication')['compiled_plan_ref']!=R('plan') or cv['acceptance_terms_ref']!=R('acceptance_terms') or cv['app_terms_refs']!=[R('app_terms')]:return False
  if cv['customer_assent_refs']!=[R('customer_assent')] or cv['party_assent_refs']!=[R('platform_assent')] or cv['original_party_authorization_refs']!=[R('platform_authorization')] or cv['current_party_authorization_refs']!=[R('platform_authorization')]:return False
  for name in ['app_terms','acceptance_terms','customer_assent','consent_verification']:
   b=B(name)
   if b['scope']!=F['scope'] or b['customer']!='customer-B' or b['agreement_id']!='B-support':return False
  return True
 except (KeyError,ValueError,TypeError,ArithmeticError):return False

BASE=build(F['records'])
for r in F['records']:assert validation(r['definition'],BASE[r['name']]['body']),r['name']
CASES={}
for cse in F['cases']:
 rows=copy.deepcopy(F['records']);ctx=copy.deepcopy(F['context']);rd={r['name']:r for r in rows}
 for p in cse['patches']:path_patch(ctx if p['record']=='$context' else rd[p['record']]['body'],p['path'],resolve(p.get('value'),BASE),p.get('delete',False))
 data=build(rows);ctx=resolve(ctx,data)
 ok=all(validation(r['definition'],data[r['name']]['body']) for r in rows) and joins(data,ctx,cse['operation'])
 selected='final_2' if cse['operation']=='correct' else 'final_1'
 CASES[cse['name']]={'accepted':ok,'comparison':data[selected]['body']['quantity_comparison']['kind'] if ok else None,'eligible_amount':data[selected]['body']['amount'] if ok else None,'observation_ref':data['observation']['ref'] if ok else None,'final_evidence_ref':data[selected]['ref'] if ok else None,'claim_ref':data['claim']['ref'] if ok else None}

def round_ratio(x):
 sign=-1 if x<0 else 1;x=abs(x);q,r=divmod(x.numerator,x.denominator);return sign*(q+(2*r>=x.denominator))
mi=F['money_inputs'];factor=10**mi['scale'];ratio=Fraction(int(mi['multiplier']['numerator']),int(mi['multiplier']['denominator']));old=round_ratio((decimal(mi['final_cost'])*ratio+decimal(mi['markup']))*factor);new=round_ratio((decimal(mi['replacement_cost'])*ratio+decimal(mi['markup']))*factor)
MONEY={}
for n,s in [('original',mi['service']),('policy_variant',mi['policy_variant_service'])]:
 service=round_ratio(decimal(s)*factor);prem=round_ratio(Fraction(service)*decimal(mi['success_percent'])/100);discount=round_ratio(Fraction(service)*decimal(mi['replacement_outcome_percent'])/100)
 MONEY[n]={'service_atoms':str(service),'cost_atoms':str(old),'premium_atoms':str(prem),'discount_atoms':str(discount),'premium_inverse_atoms':str(-prem),'cost_inverse_atoms':str(-old),'replacement_cost_atoms':str(new),'cost_delta_atoms':str(new-old),'totals_atoms':[str(service+old+prem),str(service+old+discount),str(service+new+discount)],'supplier_posting_count':0}
VARIANTS={}
for var in F['policy_variants']:
 rows=copy.deepcopy(F['records']);rd={r['name']:r for r in rows};path_patch(rd[var['record']]['body'],var['path'],var['value']);rules={x['rule_id']:x for x in rd['policy']['body']['rules']}
 for ins in rd['plan']['body']['typed_instructions']:
  if ins['opcode']=='evaluate_rule':ins['rule']=copy.deepcopy(rules[ins['rule']['rule_id']])
 v=build(rows);td=h('workflow-acceptance-terms',v['acceptance_terms']['body']);VARIANTS[var['name']]={'policy_ref':v['policy']['ref'],'plan_ref':v['plan']['ref'],'publication_ref':v['publication']['ref'],'acceptance_terms':v['acceptance_terms'],'terms_digest':td,'old_customer_assent_mismatch':BASE['customer_assent']['body']['terms_digest']!=td,'old_platform_assent_mismatch':BASE['platform_assent']['body']['consent']['basis']['terms_digest']!=td}
NEG=[];defs={r['name']:r['definition'] for r in F['records']}
for x in F['schema_negatives']:
 b=copy.deepcopy(BASE[x['record']]['body']);path_patch(b,x['path'],x['value']);NEG.append({'record':x['record'],'path':x['path'],'refused':not validation(defs[x['record']],b)})
old=load('../supplier-scoped-consent-01/supplier-scoped-consent.schema.json')['$defs'];assert all(SC['$defs'][k]==v for k,v in old.items())
output={'schema':'app-cost-author-expected/1','records':BASE,'terms_digest':h('workflow-acceptance-terms',BASE['acceptance_terms']['body']),'requests':resolve(F['requests'],BASE),'cases':CASES,'money':MONEY,'policy_variants':VARIANTS,'schema_negatives':NEG,'metadata_jcs_sha256':{n:sha(j(load(n))) for n in ['profile.json','registry.json','app-owned-cost.schema.json']},'copied_definition_count':len(old),'qualification':'conditional fixture only; no runtime admission, graph, origin, complete prospective capacity or resource certificate'}
assert sum(x['accepted'] for x in CASES.values())==5
assert all(x['refused'] for x in NEG)
if __name__=='__main__':
 (P/'fixture-expected.json').write_text(json.dumps(output,indent=2)+'\n')
 print(json.dumps({'records':len(BASE),'cases':len(CASES),'accepted_cases':[n for n,v in CASES.items() if v['accepted']],'schema_negatives_refused':len(NEG),'copied_definitions':len(old),'expected_sha256':sha((P/'fixture-expected.json').read_bytes())},indent=2))
