#!/usr/bin/env python3
"""Author-side pure contract vector derivation; never a production verifier."""
from pathlib import Path
from fractions import Fraction
import copy,hashlib,json
HERE=Path(__file__).resolve().parent

def jcs(x):return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def sha(b):return hashlib.sha256(b).hexdigest()
def H(domain,x):return sha(('bean-counter/'+domain+'/1\0').encode()+jcs(x))
def clone(x):return copy.deepcopy(x)
def doc(kind,b):
 h=H('workflow-document',[b['schema'],b]);return {'kind':kind,'id':'wdoc_'+h,'digest':'sha256:'+h}
def resolve(x,done,selfid=None):
 if isinstance(x,list):return [resolve(z,done,selfid) for z in x]
 if isinstance(x,dict):
  if set(x)=={'$record'}:return clone(done[x['$record']]['ref'])
  if set(x)=={'$id'}:return done[x['$id']]['ref']['id']
  if set(x)=={'$digest'}:return done[x['$digest']]['ref']['digest']
  if set(x)=={'$raw_sha'}:return done[x['$raw_sha']]['raw_sha']
  if x=={'$self_id':True}:
   assert selfid is not None;return selfid
  return {k:resolve(v,done,selfid) for k,v in x.items()}
 return x
def at(x,path):
 for z in path:x=x[z]
 return x
def patch(x,path,value):
 for z in path[:-1]:x=x[z]
 x[path[-1]]=clone(value)
def output_record(body,reference):return {'body':body,'canonical_hex':jcs(body).hex(),'ref':reference,'raw_sha':sha(jcs(body))}
def rounded(f):
 sign=-1 if f<0 else 1;f=abs(f);q,r=divmod(f.numerator,f.denominator);return sign*(q+(2*r>=f.denominator))
def atoms(s):
 x=Fraction(s)*10**18;assert x.denominator==1;return x.numerator

def derive():
 inp=json.loads((HERE/'fixture-input.json').read_text());scope=inp['scope'];done={}
 for r in inp['records']:
  ident=r['identity'];ident_id=None
  if ident['type']=='record':ident_id=ident['prefix']+H(ident['domain'],resolve(ident['tuple'],done))
  b=resolve(r['body'],done,ident_id)
  for path in r['set_paths']:at(b,path).sort(key=jcs)
  ref=doc(r['kind'],b) if ident['type']=='document' else {'kind':r['kind'],'id':ident_id,'digest':'sha256:'+H('workflow-record',[r['kind'],scope,b])}
  done[r['name']]=output_record(b,ref)
 out={'schema':'owner-local-cost-author-expected/1','qualification':'Conditional contract vectors only; not independent verification, real authority, capacity or software qualification.','records':done,'acceptance_terms_digest':'sha256:'+H('workflow-acceptance-terms',done['acceptance_terms']['body']),'finalize':resolve(inp['finalize_template'],done)}
 cases=[]
 for case in inp['conditional_cases']:
  c={'assertion':clone(done['final_line']['body']),'verification':clone(done['verification']['body']),'context':resolve(inp['context'],done)}
  for p in case['patches']:patch(c,p['path'],p['value'])
  a,v,ctx=c['assertion'],c['verification'],c['context'];ar=doc('workflow-fact-attestation',a)
  v.update(attestation_ref=ar,attestation_raw_sha256=sha(jcs(a)),attestation_digest=ar['digest']);su=a['subject'];v['subject_binding_refs']=sorted([su['original_binding_ref'],su['original_work_ref'],su['original_invocation_ref'],su['cost_authority_ref']],key=jcs);vr=doc('workflow-evidence-verification',v)
  ok=all(ctx[z] for z in ['original_and_current_cost_rights','authenticated_retention_origin','valid_prework_invocation','actual_owner_custody','accepted_assent','complete_future_capacity','line_unowned','component_unfinalized','verified_provider_original_line','current_expected_revision','complete_explicit_dependents'])
  ok=ok and ctx['current_status']=='active' and ctx['funding_mode']=='platform' and ctx['valuation_mode']=='final_evidence' and ctx['accepted_at']<=ctx['deadline']
  ok=ok and a['source']==ctx['authenticated_source']==v['verified_source'] and v['authenticated_principal_ref']==ctx['authenticated_principal_ref']
  ok=ok and su['logical_store_id']==ctx['store'] and su['scope']==ctx['scope']
  for k,ck in [('original_binding_ref','expected_binding_ref'),('original_work_ref','expected_work_ref'),('original_invocation_ref','expected_invocation_ref'),('cost_authority_ref','expected_cost_authority_ref')]:ok=ok and su[k]==ctx[ck]
  ok=ok and su['line_key']==ctx['expected_line_key'] and su['line_key']['provider_account_identity']==ctx['expected_account'] and a['quantity']==ctx['expected_quantity'] and a['unit']==ctx['expected_unit']
  ok=ok and a['schema']=='ledger-workflow-supplier-final-cost/1' and a['evidence_kind']=='supplier_final_cost' and a['purpose']=='supplier_line_final' and a['final'] is True and 0<=int(a['amount']['atoms'])<=int(ctx['maximum_exposure_atoms']) and a['issued_at']<=v['trusted_received_at']<=ctx['accepted_at']
  cases.append({'name':case['name'],'accepted':bool(ok),'assertion_ref':ar,'verification_ref':vr})
 out['conditional_cases']=cases
 money=[]
 for h in inp['money_histories']:
  svc=atoms(h['service_decimal']);premium=rounded(Fraction(svc)*Fraction(h['premium_percent'])/100);discount=-rounded(Fraction(svc)*Fraction(h['discount_percent'])/100);si=int(h['initial_supplier_atoms']);sn=int(h['corrected_supplier_atoms']);ratio=Fraction(int(h['multiplier']['numerator']),int(h['multiplier']['denominator']));markup=atoms(h['markup_decimal']);ri=rounded(si*ratio+markup);rn=rounded(sn*ratio+markup)
  money.append({'name':h['name'],'service_atoms':str(svc),'premium_atoms':str(premium),'discount_atoms':str(discount),'initial':{'retail_atoms':str(svc+ri+premium),'supplier_atoms':str(si)},'outcome_corrected':{'retail_atoms':str(svc+ri+discount),'supplier_atoms':str(si)},'cost_corrected':{'retail_atoms':str(svc+rn+discount),'supplier_atoms':str(sn)},'outcome_correction_actions':[str(-premium),str(discount)],'supplier_cost_correction_actions':[str(-si),str(sn)],'retail_cost_correction_actions':[str(-ri),str(rn)]})
 out['money_histories']=money
 traces=[]
 for t in inp['high_water_traces']:
  auth=int(t['authorized_atoms']);other=int(t['other_consumed_atoms']);released=int(t['released_atoms']);high={};rows=[]
  for step in t['steps']:
   e=int(step['eligible_atoms']);proposal=dict(high);prior=high.get(step['claim'],0);proposal[step['claim']]=max(prior,e);consumed=sum(proposal.values())+other;held=auth-consumed-released;accept=e>=0 and held>=0
   if accept:high=proposal
   consumed=sum(high.values())+other;held=auth-consumed-released
   rows.append({'claim':step['claim'],'eligible_atoms':str(e),'accepted':accept,'delta_atoms':str(high[step['claim']]-prior) if accept else None,'high_waters':{k:str(v) for k,v in sorted(high.items())},'consumed_atoms':str(consumed),'held_atoms':str(held),'released_atoms':str(released)})
  traces.append({'name':t['name'],'steps':rows})
 out['high_water_traces']=traces
 b=resolve(inp['consumption_template'],done);first=traces[0]['steps'][0]
 for field,amount in [('eligible_amount',first['eligible_atoms']),('high_water',first['high_waters']['line-1']),('consumption_delta',first['delta_atoms'])]:b[field]['atoms']=amount
 cr={'kind':'workflow-cost','id':'wcostconsumption_'+H('workflow-cost-consumption',[done['cost_claim']['ref']['id'],'1']),'digest':'sha256:'+H('workflow-record',['workflow-cost',scope,b])};out['initial_consumption']=output_record(b,cr)
 ids=[];line=done['cost_claim']['body']['line_key']
 for z in inp['identity_variants']:
  q=clone(line);q.update(z.get('key_patch',{}));id='wcostclaim_'+H('workflow-cost-claim',[q[k] for k in ['cost_authority_id','provider_account_identity','original_document_id','original_line_id']]);ids.append({'name':z['name'],'id':id,'same_as_original':id==done['cost_claim']['ref']['id']})
 out['identity_variants']=ids
 changes=[]
 for name,path,value in [('invalid_basis',['consumption_basis'],'cumulative_positive_delta'),('changed_account',['provider_account_identity','provider_account_id'],'other-account')]:
  b=clone(done['supplier_terms']['body']);patch(b,path,value);ref=doc('workflow-slot-terms',b);ac=clone(done['acceptance_terms']['body']);next(z for z in ac['binding_inputs']['bindings'] if z['slot']=='support:supplier')['binding_ref']=ref
  changes.append({'name':name,'supplier_terms':output_record(b,ref),'acceptance_terms':output_record(ac,doc('workflow-acceptance-terms',ac)),'terms_digest':'sha256:'+H('workflow-acceptance-terms',ac)})
 out['consent_diagnostics']=changes
 out['metadata']={n:sha(jcs(json.loads((HERE/n).read_text()))) for n in ['profile.json','registry.json','owner-local-cost.schema.json']}
 return out
if __name__=='__main__':
 x=derive();(HERE/'fixture-author-expected.json').write_text(json.dumps(x,indent=2)+'\n');print('Author fixture generated; independent freeze still required.')
