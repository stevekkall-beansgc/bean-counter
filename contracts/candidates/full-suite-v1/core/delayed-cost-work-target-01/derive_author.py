from pathlib import Path
import json,hashlib,copy,sys
import jsonschema
P=Path(sys.argv[1]); I=json.loads((P/'fixture-input.json').read_text()); S=json.loads((P/'delayed-cost-work-target.schema.json').read_text())
def j(x):return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def raw(x):return hashlib.sha256(j(x)).hexdigest()
def h(d,x):return hashlib.sha256(('bean-counter/'+d+'/1\0').encode()+j(x)).hexdigest()
def ident(pre,d,x):return pre+h(d,x)
D={}
def resolve(x):
 if isinstance(x,dict):
  if set(x)=={'$derived'}:return copy.deepcopy(D[x['$derived']])
  return {k:resolve(v)for k,v in x.items()}
 if isinstance(x,list):return [resolve(v)for v in x]
 return x
checks=[]
def check(n,b):
 checks.append({'name':n,'passed':bool(b)})
 if not b:raise AssertionError(n)
def valid(body,definition):
 q={'$schema':S['$schema'],'$defs':S['$defs'],'$ref':'#/$defs/'+definition};return jsonschema.Draft202012Validator(q).is_valid(body)
def record(body,kind,id):
 return {'body':body,'canonical_hex':j(body).hex(),'raw_sha':raw(body),'ref':{'kind':kind,'id':id,'digest':'sha256:'+h('workflow-record',[kind,I['scope'],body])}}
def dependency(body):return record(body,'workflow-dependency',ident('wdep_','workflow-dependency',[body['ancestor_decision_id'],body['dependent_decision_id'],body['original_terms_ref']]))
def root(r):return 'sha256:'+h('workflow-dependency-set',[r])
w=I['work_template'];p=w['payload'];k=w['key'];D['claim_id']=ident('wclaim_','workflow-work-claim',[k['logical_store_id'],k['scope'],w['source_namespace'],p['business_operation_id'],'completion']);D['work_id']=ident('wevent_','workflow-event',[k['logical_store_id'],k['scope'],k['source'],p['external_event_id']]);w=resolve(w);work=record(w,'workflow-work',D['work_id']);D['work_ref']=work['ref'];D['facts_digest']='sha256:'+h('workflow-claim-facts',[p[k]for k in ['customer','chain','status','activity','product_ref','policy_ref','fact_schema_ref','facts','links','binding_ref','invocation_refs','evidence_refs']]);cl=resolve(I['claim_template']);claim=record(cl,'workflow-work-claim',D['claim_id']);D['claim_ref']=claim['ref'];D['ancestor_id']=ident('wdecision_','workflow-decision',[D['claim_id'],'work']);D['cost_id']=ident('wdecision_','workflow-decision',[I['scope'],D['claim_id'],I['supplier_component_slot'],'cost']);dep=dependency(resolve(I['dependency_template']));D['dependency_ref']=dep['ref'];D['dependency_root']=root(dep['ref']);cap=record(resolve(I['capacity_template']),'workflow-correction-capacity',ident('wcapacity_','workflow-correction-capacity',[D['ancestor_id'],'1']));context=resolve(I['context'])
for n,r,d in [('work',work,'WorkRecord'),('claim',claim,'WorkClaim'),('dependency',dep,'DelayedCostWorkDependency1'),('capacity',cap,'CorrectionCapacity')]:check('schema_'+n,valid(r['body'],d))
caseout=[]
for case in I['cases']:
 c={'context':copy.deepcopy(context),'dependency':copy.deepcopy(dep['body']),'claim':copy.deepcopy(cl)}
 for path,value in case['patch'].items():
  keys=path.split('.');o=c
  for key in keys[:-1]:o=o[key]
  o[keys[-1]]=resolve(value)
 dc=dependency(c['dependency']);cc=record(c['claim'],'workflow-work-claim',D['claim_id'])
 flags=['original_declaration_present','prebill_reservation','authenticated_originals','all_ancestor_capacity_proved','original_and_current_rights'];relations=['store','scope','work_ref','work_claim_id','work_claim_ref','binding_ref','invocation_ref','supplier_component_slot','ancestor_decision_id','dependent_decision_id'];accepted=valid(c['dependency'],'DelayedCostWorkDependency1') and valid(c['claim'],'WorkClaim') and all(c['context'][x]==context[x]for x in relations) and c['claim']==cl and c['dependency']==dep['body'] and all(c['context'][x] is True for x in flags)
 result={'name':case['name'],'stage':case.get('stage','prebill'),'accepted':accepted,'candidate_dependency_ref':dc['ref'],'candidate_claim_ref':cc['ref']}
 if accepted:result.update(retained_dependency_ref=dep['ref'],retained_target_ref=work['ref'])
 caseout.append(result);check('case_'+case['name'],accepted==case['name'].startswith('valid_'))
alts={}
for name,r in [('invocation',I['anchors']['invocation']),('work_claim',D['claim_ref'])]:
 body=copy.deepcopy(dep['body']);body['target_ref']=r;dr=dependency(body);dr['dependency_root']=root(dr['ref']);alts[name]=dr;check('stable_id_'+name,dr['ref']['id']==dep['ref']['id']);check('different_digest_'+name,dr['ref']['digest']!=dep['ref']['digest']);check('different_root_'+name,dr['dependency_root']!=D['dependency_root'])
neg=[]
for item in I['schema_negatives']:
 body=copy.deepcopy(dep['body']);body.update(item['patch']);v=valid(body,'DelayedCostWorkDependency1');neg.append({'name':item['name'],'valid':v});check('schema_negative_'+item['name'],not v)
metadata={f:raw(json.loads((P/f).read_text()))for f in ['profile.json','registry.json','delayed-cost-work-target.schema.json']};reg=json.loads((P/'registry.json').read_text())
for f in ['profile','registry']:check(f+'_schema',jsonschema.Draft202012Validator(json.loads((P/(f+'.schema.json')).read_text())).is_valid(json.loads((P/(f+'.json')).read_text())))
old=Path(json.loads((P/'definition-provenance.json').read_text())['base']);base=json.loads(old.read_text());check('all_old_definitions_unchanged',all(S['$defs'][k]==v for k,v in base['$defs'].items()))
oldreg=json.loads((old.parent/'registry.json').read_text());changed=set(json.loads((P/'profile.json').read_text())['changed_operation_recipes']);
for a,b in zip(oldreg['entries'],reg['entries']):
 expected=copy.deepcopy(a)
 if a['kind'] in changed:expected['recipe_id']=a['kind']+'/delayed-work-target-1'
 check('registry_'+a['kind']+'_'+a['payload_schemas'][0],b==expected)
check('entry_count',len(oldreg['entries'])==len(reg['entries']))
out={'records':{'work':work,'claim':claim,'dependency':dep,'capacity':cap},'identities':{k:D[k]for k in ['claim_id','work_id','ancestor_id','cost_id']},'dependency_root':D['dependency_root'],'cases':caseout,'alternatives':alts,'schema_negatives':neg,'lifecycle':[{'stage':st,'retained_dependency_ref':dep['ref'],'retained_target_ref':work['ref']}for st in I['lifecycle_stages']],'metadata':metadata,'registry_entries':reg['entries']}
(P/'fixture-author-expected.json').write_text(json.dumps(out,indent=2)+'\n');(P/'author-check.json').write_text(json.dumps({'schema':'delayed-work-target-author-check/1','checks':checks,'passed':len(checks),'failed':0,'qualification':'contract DATA only; no product runtime/audit/resource/admission qualification'},indent=2)+'\n');print(len(checks),'author checks')
