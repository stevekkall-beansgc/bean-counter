from pathlib import Path
import json,copy,hashlib,base64,sys
from jsonschema import Draft202012Validator
P=Path(__file__).resolve().parent
I=json.loads((P/'fixture-input.json').read_text());S=json.loads((P/'supplier-scoped-consent.schema.json').read_text())
def cp(x):return copy.deepcopy(x)
def j(x):return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def sha(b):return hashlib.sha256(b).hexdigest()
def H(d,x):return sha(('bean-counter/'+d+'/1\0').encode()+j(x))
def doc(k,b):
 h=H('workflow-document',[b['schema'],b]);return {'kind':k,'id':'wdoc_'+h,'digest':'sha256:'+h}
def output(b,r):return {'body':b,'canonical_hex':j(b).hex(),'raw_sha':sha(j(b)),'ref':r}
def at(x,path):
 for z in path:x=x[z]
 return x
def patch(x,path,v):
 for z in path[:-1]:x=x[z]
 x[path[-1]]=cp(v)
def uniq(xs):return [json.loads(a) for a in sorted(set(j(x) for x in xs))]
def valid(b,d):return Draft202012Validator({'$defs':S['$defs'],'$ref':'#/$defs/'+d}).is_valid(b)
def resolve(x,done,projection=None):
 if isinstance(x,list):return [resolve(v,done,projection) for v in x]
 if isinstance(x,dict):
  if set(x)=={'$record'}:return cp(done[x['$record']]['ref'])
  if set(x)=={'$terms_digest'}:return 'sha256:'+H('workflow-acceptance-terms',done[x['$terms_digest']]['body'])
  if set(x)=={'$projection'}:return cp(projection)
  return {k:resolve(v,done,projection) for k,v in x.items()}
 return x
def project(plan,policy):
 ins=plan['typed_instructions'];selected=[x for x in ins if x['opcode']=='evaluate_rule' and x['rule']['binding_slot']==I['supplier_slot']]
 # Independent small fixture's closed rule-only graph, not a full production graph verifier.
 needed={x['ordinal'] for x in selected}
 changed=True
 while changed:
  old=set(needed)
  for x in ins:
   if x['ordinal'] in needed:needed.update(x.get('basis_instructions',[]))
  for e in plan['dependency_edges']:
   if e['consumer'] in needed:needed.add(e['producer'])
  changed=old!=needed
 selected=[x for x in ins if x['ordinal'] in needed]
 def key(x):return ['rule',x['rule']['binding_slot'],x['rule']['activity'],x['rule']['rule_id']]
 km={x['ordinal']:key(x) for x in selected};fm={x['index']:{k:v for k,v in x.items() if k!='index'} for x in plan['fact_slots']}
 nodes=[{'key':key(x),'kind':'rule','rule':cp(x['rule']),'facts':uniq([fm[q] for q in x['fact_slots']]),'binding_facts':uniq([fm[q] for q in x['binding_fact_slots']]),'basis_keys':uniq([km[q] for q in x['basis_instructions']])} for x in selected]
 acts={x['rule']['activity'] for x in selected};comps={x['rule']['component_key'] for x in selected};components=[x for x in plan['component_slots'] if x['key'] in comps];roots={x['root_key'] for x in components};components=[x for x in plan['component_slots'] if x['key'] in comps|roots]
 activities=[x for x in plan['activity_table'] if x['activity'] in acts]
 return {'evaluator':plan['evaluator'],'grammar':plan['grammar'],'currency':policy['currency'],'scale':policy['scale'],'rounding':policy['rounding'],'mode':policy['mode'],'execution_order':[key(x) for x in selected],'nodes':uniq(nodes),'dependency_edges':uniq([{'producer':km[e['producer']],'consumer':km[e['consumer']]} for e in plan['dependency_edges'] if e['consumer'] in needed]),'activities':uniq(activities),'components':uniq(components),'delayed_costs':uniq([x for x in policy['delayed_costs'] if x['component_key'] in comps]),'source_contract_refs':uniq([policy['product_ref'],policy['context_schema_ref']]+[a['fact_schema_ref'] for a in activities]+[f['schema_ref'] for n in nodes for f in n['facts']+n['binding_facts']]),'stages':[]}
PROJ=project(I['full_plan'],I['policy'])
def build(rows,projection=PROJ):
 done={}
 for row in rows:
  b=resolve(row['body'],done,projection)
  for path in row['set_paths']:at(b,path).sort(key=j)
  ident=row['identity'];r=doc(row['kind'],b) if ident['type']=='document' else {'kind':row['kind'],'id':ident['prefix']+H(ident['domain'],resolve(ident['tuple'],done,projection)),'digest':'sha256:'+H('workflow-record',[row['kind'],I['scope'],b])}
  done[row['name']]=output(b,r)
 return done
D=build(I['records']);checks=[]
def check(n,b):checks.append({'name':n,'passed':bool(b)})
for row in I['records']:
 b=D[row['name']]['body'];v=valid(b,row['definition']);check('schema_'+row['name'],v)
 if not v:
  es=list(Draft202012Validator({'$defs':S['$defs'],'$ref':'#/$defs/'+row['definition']}).iter_errors(b));print(row['name'],str(es[0])[:1800]);raise SystemExit(2)
check('projection_schema',valid(PROJ,'SupplierPlanProjection'));check('conditional_plan_schema',valid(resolve(I['full_plan'],D),'CompiledPlan4'))
variants=[]
for v in I['projection_variants']:
 z={'policy':cp(I['policy']),'plan':cp(I['full_plan']),'acceptance':cp(I['acceptance_projection'])}
 for pa in v['patches']:patch(z[pa['target']],pa['path'],pa['value'])
 if 'prepend_instruction' in v:
  for x in z['plan']['typed_instructions']:
   x['ordinal']=str(int(x['ordinal'])+1);x['basis_instructions']=[str(int(q)+1) for q in x['basis_instructions']]
  for e in z['plan']['dependency_edges']:
   e['producer']=str(int(e['producer'])+1);e['consumer']=str(int(e['consumer'])+1)
  z['plan']['typed_instructions'].insert(0,cp(v['prepend_instruction']));z['plan']['component_slots'].insert(0,cp(v['prepend_component']));z['policy']['rules'].insert(0,cp(v['prepend_instruction']['rule']));z['plan']['rule_order'].insert(0,{'policy_ref':{'$record':'policy'},'rule_id':v['prepend_instruction']['rule']['rule_id']})
 pr=resolve(project(z['plan'],z['policy']),D);cb=cp(D['commitment']['body']);cb['program']=pr;cb['acceptance']=resolve(z['acceptance'],D);rr=doc('workflow-offer',cb)
 vo={'name':v['name'],'projection':pr,'commitment':output(cb,rr),'provider_basis_reusable':rr==D['commitment']['ref']}
 if vo['provider_basis_reusable']:
  rows=cp(I['records']);next(r for r in rows if r['name']=='policy')['body']=z['policy'];vd=build(rows,pr);vo['publication']=vd['publication'];vo['acceptance_terms']=vd['acceptance_terms'];vo['terms_digest']='sha256:'+H('workflow-acceptance-terms',vd['acceptance_terms']['body']);vo['customer_platform_reconsent_required']=vo['terms_digest']!='sha256:'+H('workflow-acceptance-terms',D['acceptance_terms']['body'])
 variants.append(vo)
check('retail_price_reuse',next(x for x in variants if x['name']=='retail_price_only')['provider_basis_reusable']);check('unrelated_insert_reuse',next(x for x in variants if x['name']=='unrelated_retail_instruction_insert')['provider_basis_reusable'])
# Conditional exact identity/typed evidence verifier; not a product verifier.
def accepts(done,c,rows):
 try:
  bs={n:r['body'] for n,r in done.items()}
  if not all(valid(bs[r['name']],r['definition']) for r in rows):return False
  if not all(c[k] for k in ['current_admin_verified','mapping_origin_verified','consent_origin_verified','source_plan_complete','current_source_rights','original_source_rights','customer_fullterms_verified','projection_authority_complete']):return False
  if c['logical_store_id']!=c['expected_store']:return False
  if c['installation_mode']!='isolated_synthetic':return False
  if bs['consent_verification']['compiled_plan_ref']!=bs['publication']['compiled_plan_ref'] or bs['consent_verification']['publication_ref']!=done['publication']['ref']:return False
  for who,party,roles in [('provider','supplier-P',['provider']),('platform','platform-P',['bearer','payer'])]:
   auth=bs[who+'_authorization'];spec=auth['authorization'];a=bs[who+'_assent'];cs=a['consent'];me=bs[who+'_mapping_evidence'];ma=bs[who+'_mapping_attestation'];ce=bs[who+'_consent_evidence'];ca=bs[who+'_consent_attestation'];expectedsubject={'customer':'customer-B','agreement_id':'B-support','party':party,'acceptor':spec['acceptor']}
   if c[who+'_current_ref']!=done[who+'_authorization']['ref'] or auth['status']!='active':return False
   if spec['roles']!=roles or cs['roles']!=roles or spec['party']!=party or cs['party']!=party or cs['acceptor']!=spec['acceptor']:return False
   if any(x['scope']!=I['scope'] or x['customer']!='customer-B' or x['agreement_id']!='B-support' for x in [spec,cs]):return False
   if cs['authorization_ref']!=done[who+'_authorization']['ref']:return False
   if a['mode']!='synthetic' or auth['installation_mode']!='isolated_synthetic':return False
   if not (spec['starts_at']<=cs['accepted_at']<=ce['retained_at']<=c['now']<spec['ends_at']):return False
   if not(me['retained_at']<=ma['retained_at']<=auth['recorded_at']<=cs['accepted_at'] and ce['retained_at']<=ca['retained_at']<=c['now']):return False
   for x in [me,ma,ce,ca]:
    if x['subject']!=expectedsubject or x['scope']!=I['scope'] or x['installation_mode']!='isolated_synthetic':return False
    raw=base64.b64decode(x['content_base64'],validate=True)
    if len(raw)>4096 or base64.b64encode(raw).decode()!=x['content_base64'] or sha(raw)!=x['content_sha256']:return False
   if me['metadata']!={'purpose':'party_authorization','authorization':spec}:return False
   if ma['metadata']!={'purpose':'party_operator_attestation','target_evidence_ref':done[who+'_mapping_evidence']['ref'],'statement':'operator_verified_party_acceptor_authority'}:return False
   if ce['metadata']!={'purpose':'party_consent','consent':cs}:return False
   if ca['metadata']!={'purpose':'party_operator_attestation','target_evidence_ref':done[who+'_consent_evidence']['ref'],'statement':'operator_verified_party_exact_consent'}:return False
   if not(me['retained_by']==ma['retained_by']==auth['recorded_by'] and ce['retained_by']==ca['retained_by']==a['operator']):return False
   basis={'kind':'supplier_commitment','commitment_ref':done['commitment']['ref'],'offer_ref':done['offer']['ref'],'supplier_terms_ref':done['supplier_terms']['ref']} if who=='provider' else {'kind':'acceptance_terms','terms_ref':done['acceptance_terms']['ref'],'terms_digest':'sha256:'+H('workflow-acceptance-terms',bs['acceptance_terms'])}
   if cs['basis']!=basis:return False
  return True
 except (KeyError,TypeError,ValueError):return False
cases=[]
for ca in I['conditional_cases']:
 rows=cp(I['records']);ctx=cp(I['context'])
 for pa in ca['patches']:
  target=ctx if pa['target']=='context' else next(r for r in rows if r['name']==pa['target'])['body'];patch(target,pa['path'],resolve(pa['value'],D))
 dd=build(rows);c=resolve(ctx,dd);yes=accepts(dd,c,rows);z={'name':ca['name'],'accepted':yes,'effects':[]}
 if yes:
  b=cp(dd['consent_verification']['body']);b['verified_by']=c['actor'];b['verified_at']=c['now'];z['verification']=output(b,doc('workflow-verification',b))
 else:z['verification']=None
 cases.append(z);check('case_'+ca['name'],yes==(ca['name'] in ['valid_original','valid_new_admin_historical_operator']))
neg=[]
for n in I['schema_negatives']:
 b=cp(D[n['record']]['body']);patch(b,n['path'],n['value']);definition=next(r['definition'] for r in I['records'] if r['name']==n['record']);yes=valid(b,definition);neg.append({'record':n['record'],'body':b,'schema_valid':yes});check('negative_'+n['record'],not yes)
old=json.loads((P.parent/'delayed-cost-work-target-01/delayed-cost-work-target.schema.json').read_text())['$defs'];check('old_definitions_unchanged',all(S['$defs'][k]==v for k,v in old.items()))
for f,ss in [('profile.json','profile.schema.json'),('registry.json','registry.schema.json')]:check(f,Draft202012Validator(json.loads((P/ss).read_text())).is_valid(json.loads((P/f).read_text())))
out={'schema':'supplier-scoped-consent-author-expected/1','qualification':'Synthetic conditional contract DATA only, no runtime/authority/source/capacity/resource qualification','records':D,'projection_variants':variants,'cases':cases,'schema_negatives':neg,'acceptance_terms_digest':'sha256:'+H('workflow-acceptance-terms',D['acceptance_terms']['body']),'metadata':{name:'sha256:'+sha(j(json.loads((P/file).read_text()))) for name,file in [('profile','profile.json'),('registry','registry.json'),('schema','supplier-scoped-consent.schema.json')]}}
check('all_cases_classified',len(cases)==31)
result={'schema':'supplier-scoped-consent-author-check/1','checks':checks,'passed':sum(x['passed'] for x in checks),'failed':sum(not x['passed'] for x in checks),'product_execution':False}
outdir=Path(sys.argv[1]) if len(sys.argv)>1 else P;outdir.mkdir(exist_ok=True)
(outdir/'fixture-author-expected.json').write_text(json.dumps(out,indent=2)+'\n');(outdir/'author-check.json').write_text(json.dumps(result,indent=2)+'\n')
print(result['passed'],result['failed']);assert not result['failed']
