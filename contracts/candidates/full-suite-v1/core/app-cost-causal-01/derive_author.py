#!/usr/bin/env python3
"""Author's independent conditional derivation, written after root input seal."""
import copy,hashlib,json
from datetime import datetime,timedelta,timezone
from decimal import Decimal
from fractions import Fraction
from pathlib import Path
import jsonschema
P=Path(__file__).resolve().parent
def load(n):return json.loads((P/n).read_text())
def j(v):return json.dumps(v,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()
def sha(b):return hashlib.sha256(b).hexdigest()
def h(domain,v):return 'sha256:'+sha(('bean-counter/'+domain+'/1').encode()+b'\0'+j(v))
F=load('fixture-input.json');SC=load('app-cost-causal.schema.json')
def resolve(v,done):
 if isinstance(v,list):return [resolve(x,done) for x in v]
 if isinstance(v,dict):
  if len(v)==1:
   k,a=next(iter(v.items()))
   if k=='$record':return copy.deepcopy(done[a]['ref'])
   if k=='$record_id':return done[a]['ref']['id']
   if k=='$body':return copy.deepcopy(done[a]['body'])
   if k=='$field':return copy.deepcopy(done[a[0]]['body'][a[1]])
   if k=='$raw_sha':return done[a]['raw_sha256']
   if k=='$digest':return done[a]['ref']['digest']
   if k=='$terms_digest':return h('workflow-acceptance-terms',done[a]['body'])
   if k=='$id':return a[1]+h(a[0],resolve(a[2],done))[7:]
  return {k:resolve(a,done) for k,a in v.items()}
 return v

def doc(body,kind):
 dg=h('workflow-document',[body['schema'],body]);return {'body':body,'canonical_hex':j(body).hex(),'raw_sha256':sha(j(body)),'ref':{'kind':kind,'id':'wdoc_'+dg[7:],'digest':dg}}
def record(body,kind,prefix,domain,seed):
 return {'body':body,'canonical_hex':j(body).hex(),'raw_sha256':sha(j(body)),'ref':{'kind':kind,'id':prefix+h(domain,seed)[7:],'digest':h('workflow-record',[kind,F['scope'],body])}}
def build(rows):
 done={}
 for row in rows:
  b=resolve(row['body'],done)
  for path in row['set_paths']:
   q=b
   for k in path[:-1]:q=q[k]
   v=q[path[-1]];assert len({j(x) for x in v})==len(v);q[path[-1]]=sorted(v,key=j)
  i=row['identity']
  if i['type']=='document':out=doc(b,row['kind'])
  elif i['type']=='plan':
   dg=h('workflow-plan',b);out={'body':b,'canonical_hex':j(b).hex(),'raw_sha256':sha(j(b)),'ref':{'kind':row['kind'],'id':'wplan_'+dg[7:],'digest':dg}}
  else:out=record(b,row['kind'],i['prefix'],i['domain'],resolve(i['tuple'],done))
  done[row['name']]=out
 return done

def valid(name,body):return not list(jsonschema.Draft202012Validator({'$ref':'#/$defs/'+name,'$defs':SC['$defs']}).iter_errors(body))
def patch_at(x,path,value,delete=False):
 for k in path[:-1]:x=x[k]
 if delete:del x[path[-1]]
 else:x[path[-1]]=value

def dt(v):return datetime.strptime(v,'%Y-%m-%dT%H:%M:%S.%fZ').replace(tzinfo=timezone.utc)
def tm(v):return v.strftime('%Y-%m-%dT%H:%M:%S.%fZ')
def plus(v,us):
 n=int(us);assert n>=0;return tm(dt(v)+timedelta(microseconds=n))

def shared(d):
 B=lambda n:d[n]['body'];R=lambda n:d[n]['ref']
 dec=B('app_terms')['app_cost_terms'][0];dep=B('dependency');work=B('work');binding=B('binding')
 return (dec==B('policy')['app_delayed_costs'][0] and dec['correction_anchor']=='initial_cost_accepted_at' and dep['kind']=='delayed_cost' and 'family_slot' not in dep and dep['target_ref']==R('work') and dep['original_terms_ref']==R('binding') and dep['ancestor_decision_id']=='wdecision_'+h('workflow-decision',[work['claim_id'],'work'])[7:] and dep['dependent_decision_id']=='wdecision_'+h('workflow-decision',[F['scope'],R('work')['id'],'component:model','app_cost'])[7:] and binding['policy_ref']==R('policy') and binding['compiled_plan_ref']==R('plan') and work['payload']['policy_ref']==R('policy') and work['payload']['binding_ref']==R('binding') and B('plan')['source_refs']==[R('policy')])

def fresh_ok(d,c):
 try:
  B=lambda n:d[n]['body'];R=lambda n:d[n]['ref'];decl=B('app_terms')['app_cost_terms'][0];ini=B('initial_cost_decision');dep=B('dependency');claim=B('claim')
  if not shared(d):return False
  if c['operation'] not in ['quantity.correct','work.reverse'] or c['mode']!='fresh' or c['accepted_at']>c['root_correction_until'] or c['status']!='active' or c['current_revision']!='2' or c['expected_revision']!=c['current_revision']:return False
  if c['scope']!=F['scope'] or c['customer']!='customer-B' or c['source']!='urn:fixture:cost-source' or c['actor']!='fixture-cost-principal' or c['source_slot']!=decl['correction_source_slot'] or c['source_slot']!='support:cost-source' or not {'correct','read'}<=set(c['rights']) or c['required_operation']!='cost.correct':return False
  if not all(c[k] for k in ['original_and_current_source_active','root_original_current_authorized','parent_component_affects_dependency','exact_origin_reconstructed','consent_and_original_mapping_complete','complete_affected_closure','all_ancestor_capacity_held','whole_resource_bound_complete','head_cas_matches','original_current_ceilings_hold']):return False
  for key,name in [('original_work_ref','work'),('binding_ref','binding'),('app_terms_ref','app_terms'),('dependency_ref','dependency'),('claim_ref','claim'),('current_evidence_ref','final_2'),('current_consumption_ref','consumption_2'),('initial_cost_decision_ref','initial_cost_decision')]:
   if c[key]!=R(name):return False
  t=resolve(F['authority_template'],d);q=resolve(F['closure_check_template'],d)
  if c['current_grant_ref']!=t['current_grant_ref'] or c['root_ref']!=t['causal_root_ref'] or c['current_decision_ref']!=q['current_decision_ref'] or c['current_cost_revision_ref']!=q['current_cost_revision_ref']:return False
  if c['component_slot']!='component:model' or c['component_key']!='support:model-retail' or c['root_decision_id']!=dep['ancestor_decision_id']:return False
  if claim['dependency_ref']!=R('dependency') or claim['subject']['original_work_ref']!=R('work') or claim['subject']['original_binding_ref']!=R('binding') or claim['subject']['component_slot']!=c['component_slot'] or claim['subject']['component_key']!=c['component_key']:return False
  if ini['revision']!='1' or ini['transition']!='cost_finalize' or ini['status']!='active' or ini['kind']!='cost' or ini['subject_ref']!=R('claim') or ini['original_terms_ref']!=R('binding') or ini['decision_id']!=dep['dependent_decision_id']:return False
  W=B('work')['accepted_at'];T=ini['accepted_at'];deadline=plus(T,decl['correction_within_us']);plus(plus(W,decl['finalize_within_us']),decl['correction_within_us'])
  if not W<=T<=plus(W,decl['finalize_within_us']) or c['accepted_at']<T:return False
  return c['accepted_at']<=deadline or ('causal_reversal' in decl and valid('AppCausalClause1',decl['causal_reversal']))
 except (KeyError,TypeError,ValueError,AssertionError,OverflowError):return False

BASE=build(F['records']);PENDING=build(F['pending_records'])
for rows,data in [(F['records'],BASE),(F['pending_records'],PENDING)]:
 for r in rows:assert valid(r['definition'],data[r['name']]['body']),r['name']
CASES={}
for case in F['cases']:
 rows=copy.deepcopy(F['records']);rmap={r['name']:r for r in rows};c=copy.deepcopy(F['context'])
 for p in case['patches']:patch_at(c if p['target']=='context' else rmap[p['target']]['body'],p['path'],resolve(p.get('value'),BASE),p.get('delete',False))
 data=build(rows);c=resolve(c,data);ok=all(valid(r['definition'],data[r['name']]['body']) for r in rows)
 out={'accepted':False,'disposition':'refused','authority':None,'closure_check':None,'inverse_atoms':[],'retained':None}
 if ok and c['mode']=='replay' and all(c[k] for k in ['exact_origin_reconstructed','saved_receipt_verified','retry_intent_equal','read_authorized']):out.update(accepted=True,disposition='saved_replay')
 elif ok and fresh_ok(data,c):
  a=resolve(F['authority_template'],data);decl=data['app_terms']['body']['app_cost_terms'][0];T=data['initial_cost_decision']['body']['accepted_at'];deadline=plus(T,decl['correction_within_us']);basis='original_standalone_window' if c['accepted_at']<=deadline else 'accepted_app_causal_clause'
  a.update(actor=c['actor'],source=c['source'],current_grant_ref=c['current_grant_ref'],source_slot=c['source_slot'],scope=c['scope'],customer=c['customer'],observed_at=c['accepted_at'],causal_root_ref=c['root_ref'],causal_root_operation=c['operation'],initial_cost_decision_ref=data['initial_cost_decision']['ref'],initial_cost_accepted_at=T,standalone_correction_until=deadline,deadline_basis=basis)
  if 'causal_reversal' in decl:a['causal_reversal']=copy.deepcopy(decl['causal_reversal'])
  else:a.pop('causal_reversal',None)
  assert valid('AppCausalAuthority1',a);auth=doc(a,'workflow-economic-authority');q=resolve(F['closure_check_template'],data);q.update(dependent_authority_ref=auth['ref'],causal_root_operation=c['operation'],deadline_basis=basis,initial_cost_decision_ref=data['initial_cost_decision']['ref']);assert valid('AppCostClosureCheck2',q)
  out.update(accepted=True,disposition='fresh_causal_inverse',authority=auth,closure_check=q,inverse_atoms=[str(-int(v)) for v in reversed(c['current_action_atoms'])],retained={'claim_ref':data['claim']['ref'],'evidence_ref':c['current_evidence_ref'],'consumption_ref':c['current_consumption_ref'],'cost_atoms':c['retained_cost_atoms'],'independent_service_outcome_atoms':c['independent_service_outcome_atoms'],'status':'reversed','active_action_atoms':[]})
 CASES[case['name']]=out

TIMING={}
for case in F['timing_cases']:
 c=dict(F['timing_base']);c.update(case['patch']);W=c['work_accepted_at'];Fby=plus(W,c['finalize_within_us']);M=plus(Fby,c['correction_within_us']);T=c['initial_cost_accepted_at'];until=plus(T,c['correction_within_us']) if T else None;validT=T is None or W<=T<=Fby;A=c['accepted_at'];op=c['operation'];disp='refused';fresh=c['current_rights_valid'] and c['clock_monotone']
 if op=='replay':
  if T and validT and c['original_receipt_valid'] and c['incoming_intent_equal'] and c['read_authorized']:disp='saved_replay'
 elif op=='cost.finalize' and T is None and W<=A<=Fby and fresh:T=A;until=plus(T,c['correction_within_us']);disp='finalized'
 elif op=='cost.correct' and T and validT and T<=A<=until and fresh:disp='corrected'
 elif op=='wait' and T is None:disp='waiting' if A<=Fby else 'expired_unbilled'
 elif op=='root_close_pending' and T is None and c.get('root_closure_authorized') and fresh:disp='pending_closed'
 TIMING[case['name']]={'disposition':disp,'finalize_by':Fby,'initial_cost_accepted_at':T,'correction_until':until,'prework_correction_maximum':M}

PCASES={}
for case in F['pending_cases']:
 c=copy.deepcopy(F['pending_context']);c.update(resolve(case['patch'],PENDING));c=resolve(c,PENDING);a=resolve(F['pending_authority_template'],PENDING);R=lambda n:PENDING[n]['ref'];B=lambda n:PENDING[n]['body']
 ok=shared(PENDING) and c['operation'] in ['quantity.correct','work.reverse'] and c['accepted_at']<=c['root_correction_until'] and c['root_ref']==a['causal_root_ref'] and c['root_decision_id']==B('dependency')['ancestor_decision_id'] and c['component_slot']=='component:model' and c['financial_observation_ref']==a['financial_observation_ref'] and c['source']==a['source'] and c['source_slot']==B('app_terms')['app_cost_terms'][0]['correction_source_slot'] and {'correct','read'}<=set(c['rights']) and all(c[k] for k in ['original_current_authorized','actual_audited_absence','root_closure_complete','capacity_resource_complete','head_cas_matches','new_root_permanently_excludes_this_dependency'])
 ok=ok and all(c[k]==R(n) for k,n in [('work_ref','work'),('dependency_ref','dependency'),('binding_ref','binding'),('terms_ref','app_terms')])
 out={'accepted':bool(ok),'authority':None,'closure_check':None,'retirement':None}
 if ok:
  a.update(observed_at=c['accepted_at'],causal_root_operation=c['operation']);decl=B('app_terms')['app_cost_terms'][0]
  if 'causal_reversal' in decl:a['causal_reversal']=copy.deepcopy(decl['causal_reversal'])
  else:a.pop('causal_reversal',None)
  assert valid('AppPendingCausalAuthority1',a);authority=doc(a,'workflow-economic-authority');q=resolve(F['pending_closure_check_template'],PENDING);q.update(dependent_authority_ref=authority['ref'],causal_root_operation=c['operation']);assert valid('AppPendingCostClosureCheck1',q)
  b={'schema':'ledger-workflow-capacity-retirement/1','dependency_ref':R('dependency'),'closure_state_refs':[c['closed_root_ref']],'reason':'no_future_or_active_dependency','authority_ref':authority['ref'],'operation_key':c['operation_key']};assert valid('CapacityRetirement',b)
  out.update(authority=authority,closure_check=q,retirement=record(b,'workflow-capacity-retirement','wretire_','workflow-capacity-retirement',[b['dependency_ref'],b['operation_key']]))
 PCASES[case['name']]=out

NEG=[]
for n in F['schema_negatives']:
 if 'body' in n:b=copy.deepcopy(n['body'])
 else:
  b=copy.deepcopy(BASE[n['base_record']]['body'])
  for k in n['path']:b=b[k]
  b.update(n['patch'])
 NEG.append({'definition':n['definition'],'refused':not valid(n['definition'],b)})
def rat(v):return Fraction(Decimal(v))
def round_away(v):
 sign=-1 if v<0 else 1;v=abs(v);q,r=divmod(v.numerator,v.denominator);return sign*(q+(r*2>=v.denominator))
mi=F['money_inputs'];scale=10**mi['scale'];ratio=Fraction(int(mi['multiplier']['numerator']),int(mi['multiplier']['denominator']));old=round_away((rat(mi['final_cost'])*ratio+rat(mi['markup']))*scale);new=round_away((rat(mi['replacement_cost'])*ratio+rat(mi['markup']))*scale);MONEY={}
for n,s in [('original',mi['service']),('policy_variant',mi['policy_variant_service'])]:
 service=round_away(rat(s)*scale);premium=round_away(Fraction(service)*rat(mi['success_percent'])/100);discount=round_away(Fraction(service)*rat(mi['replacement_outcome_percent'])/100)
 MONEY[n]={'service_atoms':str(service),'cost_atoms':str(old),'premium_atoms':str(premium),'discount_atoms':str(discount),'premium_inverse_atoms':str(-premium),'cost_inverse_atoms':str(-old),'replacement_cost_atoms':str(new),'cost_delta_atoms':str(new-old),'totals_atoms':[str(service+old+premium),str(service+old+discount),str(service+new+discount)],'supplier_posting_count':0}
olddefs=load('../app-owned-cost-01/app-owned-cost.schema.json')['$defs'];assert all(SC['$defs'][n]==v for n,v in olddefs.items())
reg=load('registry.json');assert all(r['kind'] in reg['record_kinds'] for r in F['records']+F['pending_records']);assert 'workflow-verification' in reg['record_kinds']
assert all(n['refused'] for n in NEG)
output={'schema':'app-cost-causal-author-expected/1','records':BASE,'pending_records':PENDING,'terms_digest':h('workflow-acceptance-terms',BASE['acceptance_terms']['body']),'cases':CASES,'timing_cases':TIMING,'pending_cases':PCASES,'schema_negatives':NEG,'money':MONEY,'causal_money':{'inverse_atoms':str(-int(F['context']['retained_cost_atoms'])),'retained_expense_atoms':F['context']['retained_cost_atoms'],'unchanged_service_outcome_atoms':F['context']['independent_service_outcome_atoms']},'metadata_jcs_sha256':{n:sha(j(load(n))) for n in ['profile.json','registry.json','app-cost-causal.schema.json']},'copied_definition_count':len(olddefs),'qualification':'conditional authority/bytes/math only; no runtime origin, capacity, resource or financial admission proof'}
if __name__=='__main__':
 (P/'fixture-expected.json').write_text(json.dumps(output,indent=2)+'\n');print(json.dumps({'records':len(BASE),'pending_records':len(PENDING),'cases':len(CASES),'accepted_cases':[k for k,v in CASES.items() if v['accepted']],'timing_cases':len(TIMING),'pending_accepted':[k for k,v in PCASES.items() if v['accepted']],'schema_negatives_refused':len(NEG),'expected_sha256':sha((P/'fixture-expected.json').read_bytes())},indent=2))
