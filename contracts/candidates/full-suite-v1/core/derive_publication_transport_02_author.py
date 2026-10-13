#!/usr/bin/env python3
"""Author-only derivation from literal inputs and normative recipes, no reviewer/production imports.
Default reproduces saved author result read-only. --write first creates author result.
"""
import base64,copy,hashlib,json,sys
from pathlib import Path
P=Path(__file__).resolve().parent
load=lambda n:json.loads((P/n).read_text())
def cj(x):
 if isinstance(x,dict):return '{'+','.join(cj(k)+':'+cj(x[k]) for k in sorted(x,key=lambda k:k.encode('utf-16-be')))+'}'
 if isinstance(x,list):return '['+','.join(map(cj,x))+']'
 return json.dumps(x,ensure_ascii=False,separators=(',',':'),allow_nan=False)
def jb(x):return cj(x).encode()
def sha(x):return hashlib.sha256(x).hexdigest()
def h(d,x,v=1):return 'sha256:'+sha(f'bean-counter/{d}/{v}\0'.encode()+jb(x))
def ordered(xs):return sorted(xs,key=jb)
def unique(xs):return ordered({cj(x):x for x in xs}.values())
def refs_in(x):
 if isinstance(x,dict):
  if set(x)=={'kind','id','digest'}:return [x]
  return [r for v in x.values() for r in refs_in(v)]
 if isinstance(x,list):return [r for v in x for r in refs_in(v)]
 return []
def dr(kind,body):
 z=h('workflow-document',[body['schema'],body]);return {'kind':kind,'id':'wdoc_'+z[7:],'digest':z}
f=load('publication-transport-02-fixture-input.json');basis=f['trusted_local_basis'];scope=basis['scope'];store=basis['logical_store_id'];source=basis['source'];actor=basis['principal_id']
assert sha((P/f['source_file']).read_bytes())==f['source_file_sha256']
source_input=load(f['source_file']);docs=source_input['documents'];by_kind={};sources=[];lookup={};records={};committed_docs=set();operations=[]
for b in docs:
 k=b['schema'].removeprefix('ledger-').removesuffix('/1');r=dr(k,b);sources.append(r);lookup[r['id']]=(r,b);by_kind.setdefault(k,[]).append((r,b))
sources=ordered(sources);policy_ref,policy=by_kind['workflow-policy'][0];product_ref,product=by_kind['workflow-product'][0];template_ref,template=by_kind['workflow-slot-template'][0];compiler_ref,compiler=by_kind['workflow-compiler-descriptor'][0]
# Symbolic unit/true IR directly from approved table dictionary, not production compiler output.
schema_refs=unique(product['fact_schemas']+[policy['context_schema_ref']]);slots=[]
for sr in sorted(schema_refs,key=lambda r:r['digest']):
 for field in sorted(lookup[sr['id']][1]['fields'],key=lambda z:z['name']):slots.append({'index':str(len(slots)),'schema_ref':sr,'field':field})
activity={a['name']:a for a in product['activities']};plan_activity=[]
for a in sorted(product['activities'],key=lambda a:a['name']):plan_activity.append({'activity':a['name'],'primitive':a['primitive'],'fact_schema_ref':a['fact_schema'],'source_slot':a['source_slot'],**({'funding_leg':a['funding_leg']} if 'funding_leg' in a else {})})
inst=[];ex=[];order=[]
for i,rule in enumerate(policy['rules']):
 assert rule['when']=={'op':'true'} and rule['formula']['kind']=='unit' and rule['phase']=='base'
 field=rule['formula']['quantity_field'];sr=activity[rule['activity']]['fact_schema'];idx=next(x['index'] for x in slots if x['schema_ref']==sr and x['field']['name']==field)
 inst.append({'opcode':'evaluate_rule','ordinal':str(i),'policy_ref':policy_ref,'rule':rule,'fact_slots':[idx],'binding_fact_slots':[],'basis_instructions':[]});order.append({'policy_ref':policy_ref,'rule_id':rule['rule_id']});ex.append({'ordinal':str(i),'policy_ref':policy_ref,'rule_or_family':rule['rule_id'],'fields':[field],'primitive':'unit'})
plan={'schema':'ledger-workflow-compiled-plan/1','compiler_ref':compiler_ref,'compiler_semantics':'ledger-policy-compiler/1','evaluator':'ledger-workflow-evaluator/1','grammar':'ledger-policy-grammar/1','limits_profile':'workflow-limits/1','source_refs':sources,'activity_table':plan_activity,'fact_slots':slots,'link_paths':[],'component_slots':sorted(template['components'],key=lambda x:x['key']),'rule_order':order,'typed_instructions':inst,'dependency_edges':[],'worst_case':{'fields':'1088','nodes':'256','hops':'16','loaded_refs':'4096','actions':'128','explanations':'256','bytes':'4194304','compile_work':'1048576','runtime_work':'65536'},'explanation_map':ex}
pd=h('workflow-plan',plan);plan_ref={'kind':'workflow-compiled-plan','id':'wplan_'+pd[7:],'digest':pd}
# Build fixture is synthetic, with exact independently inspectable source bytes, not real provenance.
assert sha((P/f['build_input_file']).read_bytes())==f['build_input_sha256']
bf=load(f['build_input_file']);files=bf['source']['files'];inventory={'schema':'ledger-build-source-inventory/1','recipe':'bean-counter-build-inputs/1','entries':[{'path':v['path'],'bytes':str(len(v['utf8'].encode())),'sha256':sha(v['utf8'].encode())} for v in sorted(files,key=lambda z:z['path'].encode('utf-16-be'))]}
recipe={**bf['recipe'],'cargo_lock_digest':'sha256:'+next(v['sha256'] for v in inventory['entries'] if v['path']=='Cargo.lock'),'compiler_ref':compiler_ref}
build={'schema':'ledger-workflow-build-manifest/1','build_inputs':{'schema':'ledger-workflow-build-inputs/1','source':{'base_commit':bf['source']['base_commit'],'state':bf['source']['state'],'inventory_digest':h('build-source-inventory',inventory),'file_count':len(files),'total_bytes':str(sum(int(v['bytes']) for v in inventory['entries']))},'recipe':recipe},'executable':{'sha256':sha(bf['executable_utf8'].encode()),'bytes':str(len(bf['executable_utf8'].encode())),'observation':'stable-current-executable-file/1'}}
build_ref=dr('workflow-build-manifest',build)
def keep(r,b):
 if r['id'] in records:assert records[r['id']]['body']==b
 records[r['id']]={**r,'logical_store_id':store,'scope':scope,'body':b}
 lookup[r['id']]=(r,b);return r

def closure(start):
 seen={};pending=list(start)
 while pending:
  r=pending.pop()
  if r['id'] in seen:continue
  rr,b=lookup[r['id']];assert rr==r
  seen[r['id']]=r;pending.extend(refs_in(b))
 return unique(seen.values())
def descriptor_refs(r):return [x for x in closure([r]) if x['kind'] in ['workflow-fact-schema','workflow-compiler-descriptor']]
reg_digest='sha256:'+sha(jb(load(f['retention_registry'])));pub_reg_digest='sha256:'+sha(jb(load(f['retained_publication_registry'].split(' ')[0])))
total_b=0
for spec,t in zip(f['requests'],f['timeline']):
 before=set(committed_docs);newdocs=[];extra=[]
 if 'fixture_assemble' in spec:
  q={'schema':'ledger-operation/1','key':{'logical_store_id':store,'scope':scope,'source':source,'operation_id':spec['operation_id']},'kind':'policy.publish','contract':'workflow/1','payload':{'schema':'ledger-workflow-policy-publication/1','mode':'retained','document_refs':sources,'root_policy_ref':policy_ref,'expected_plan_digest':pd}}
 else:q=copy.deepcopy(spec)
 key=q['key'];payload=q['payload'];kind=q['kind'];at=t['accepted_at'];assert at==t['verified_at'] and key['operation_id']==t['operation_id']
 auth={'schema':'ledger-workflow-local-authority-snapshot/1','basis':basis,'basis_digest':h('local-authority-basis',basis),'verification_profile':'local-private-directory-owner/1','verified_at':at};ar=keep(dr('workflow-authority',auth),auth);newdocs.append(ar)
 loc=[];anchors=[];desc=[]
 def anchor(role,r,relation,ds):
  loc.append({'role':role,'ref':r});anchors.append({'role':role,'record_ref':r,'relation':relation,'descriptor_refs':ordered(ds)})
 if kind=='policy.publish':
  assert all(r['id'] in before for r in sources),'publication source not publicly retained'
  keep(plan_ref,plan);keep(build_ref,build);newdocs.extend([plan_ref,build_ref])
  publication={'schema':'ledger-workflow-publication/1','root_policy_ref':policy_ref,'source_refs':sources,'compiled_plan_ref':plan_ref,'compiler_ref':compiler_ref,'provenance':{'source_sha':bf['source']['base_commit'],'binary_sha256':build['executable']['sha256'],'build_manifest_digest':'sha256:'+sha(jb(build))}}
  pub_ref={'kind':'workflow-publication','id':'wpub_'+h('workflow-publication',[store,scope,policy_ref])[7:],'digest':h('workflow-record',['workflow-publication',scope,publication])};keep(pub_ref,publication);newdocs.append(pub_ref);results=ordered([pub_ref,plan_ref]);extra=[build_ref]
  # Existing retained-publication source locator/anchor recipe, no transport-registry substitution.
  for r in sources:anchor('inline_document',r,'publication_source',schema_refs if r==policy_ref else [])
  desc=ordered(schema_refs+[compiler_ref]);rd=pub_reg_digest
 else:
  rd=reg_digest
  if payload['schema']=='ledger-workflow-definition-fragment/1':
   b={**payload,'schema':'ledger-workflow-definition-fragment-record/1','logical_store_id':store,'scope':scope,'source':source};result=keep(dr('workflow-definition-fragment',b),b)
  else:
   if payload['schema']=='ledger-workflow-definition-retain/1':b=payload['document']
   else:
    fr=payload['fragment_refs'];assert all(r['id'] in before for r in fr)
    parts=sorted([lookup[r['id']][1] for r in fr],key=lambda v:int(v['part']));raw=b''.join(base64.b64decode(v['bytes_base64'],validate=True) for v in parts);assert len(raw)==int(parts[0]['document_bytes']) and sha(raw)==parts[0]['canonical_document_sha256'];b=json.loads(raw);assert jb(b)==raw;extra=fr
    for r in fr:anchor('evidence',r,'evidence_input',[])
   deps=unique(refs_in(b));assert all(r['id'] in before for r in deps),'source dependency not publicly retained'
   result=keep(dr(b['schema'].removeprefix('ledger-').removesuffix('/1'),b),b)
   anchor('inline_document',result,'publication_source',descriptor_refs(result))
   for r in deps:anchor('policy',r,'direct',descriptor_refs(r))
   desc=unique([r for a in anchors for r in a['descriptor_refs']])
  results=[result];newdocs.append(result)
 loc=unique(loc);anchors=unique(anchors)
 resolution={'schema':'ledger-operation-descriptors/1','registry_digest':rd,'operation_kind':kind,'payload_schema':payload['schema'],'locator_digest':h('descriptor-locators',[kind,payload['schema'],loc]),'anchors':anchors,'descriptors':desc,'normalization_profile':'ledger-operation-normalization/2'};resolution['closure_digest']=h('descriptor-closure',resolution)
 reqbytes=jb(q);assert len(reqbytes)<=262144
 evidence={'schema':'ledger-operation-request-evidence/2','normalization_profile':'ledger-operation-normalization/2','original_request_bytes_base64':base64.b64encode(reqbytes).decode(),'original_request_bytes_sha256':sha(reqbytes),'normalized_request':q,'request_hash':h('operation-request',[resolution['closure_digest'],q],2),'schema_bundle_digest':h('schema-bundle',desc),'descriptor_resolution':resolution}
 authority={'schema':'ledger-workflow-publication-authority/1','principal_id':actor,'source':source,'authority_refs':[ar],'rights':['admin'],'validated_at':at}
 op={'schema':'ledger-workflow-operation-record/1','key':key,'kind':kind,'contract':'workflow/1','request_evidence':evidence,'authority_snapshot':authority,'accepted_at':at,'commit_sequence':t['commit_sequence'],'result_refs':results}
 op_ref={'kind':'workflow-operation','id':'wop_'+h('workflow-operation',key)[7:],'digest':h('workflow-record',['workflow-operation',scope,op])};keep(op_ref,op)
 members=unique(closure(results+[ar]+extra)+[op_ref]);manifest={'schema':'ledger-workflow-publication-manifest/1' if kind=='policy.publish' else 'ledger-workflow-definition-manifest/1','key':key,'members':members};md=h('workflow-manifest',manifest);mr={'kind':'workflow-manifest','id':'wmanifest_'+md[7:],'digest':md};keep(mr,manifest)
 resultkind='semantic_alias' if all(r['id'] in before for r in results) else 'administrative'
 receipt={'schema':'ledger-operation-receipt/2','key':key,'request_hash':evidence['request_hash'],'request_hash_profile':'bean-counter/operation-request/2','normalization_profile':'ledger-operation-normalization/2','schema_bundle_digest':evidence['schema_bundle_digest'],'descriptor_closure_digest':resolution['closure_digest'],'original_request_bytes_sha256':evidence['original_request_bytes_sha256'],'result_kind':resultkind,'result_refs':results,'accepted_at':at,'commit_sequence':t['commit_sequence'],'manifest_hash':md};receipt['receipt_hash']=h('operation-receipt',receipt,2);rr={'kind':'workflow-receipt','id':'wreceipt_'+receipt['receipt_hash'][7:],'digest':receipt['receipt_hash']};keep(rr,receipt)
 new_unique=unique([r for r in newdocs if r['id'] not in before]);docbytes=sum(len(jb(lookup[r['id']][1])) for r in new_unique);wrapperbytes=sum(map(len,[jb(key),jb(op),jb(manifest),jb(receipt)]));delta=docbytes+wrapperbytes;total_b+=delta
 committed_docs.update(r['id'] for r in new_unique)
 operations.append({'operation_id':key['operation_id'],'request':q,'locators':loc,'descriptor_resolution':resolution,'request_evidence':evidence,'authority_ref':ar,'operation_ref':op_ref,'manifest_ref':mr,'receipt_ref':rr,'result_refs':results,'profile_b':{'units':1,'new_document_refs':new_unique,'new_document_bytes':docbytes,'wrapper_and_key_bytes':wrapperbytes,'payload_bytes':delta,'cumulative_payload_bytes':total_b}})
reads=[]
for r in [template_ref,plan_ref]:
 b=lookup[r['id']][1];raw=jb(b);response={'schema':'ledger-workflow-document-read/2','ref':r,'canonical_bytes_base64':base64.b64encode(raw).decode(),'canonical_bytes_sha256':sha(raw),'body_bytes':str(len(raw))};assert len(jb(response))<=8388608
 reads.append({'request':{'schema':'ledger-read/1','kind':'policy.document','payload':{'ref':r,'representation':'canonical_bytes'}},'response':response,'wire_bytes':len(jb(response))})
original=next(x for x in operations if x['operation_id']==f['original_retry']['operation_id'])
output={'status':'AUTHOR_SOURCE_ONLY_CHAIN_NOT_INDEPENDENT_FREEZE_OR_RUNTIME','source_input_sha256':sha((P/'publication-transport-02-fixture-input.json').read_bytes()),'records':ordered(records.values()),'operations':operations,'canonical_reads':reads,'original_retry':{'receipt_ref':original['receipt_ref'],'request_evidence':original['request_evidence'],'status':{'schema':'ledger-operation-status/1','status':'committed','receipt':lookup[original['receipt_ref']['id']][1],'request_evidence':original['request_evidence']},'units':0,'payload_bytes':0},'build_inventory':inventory,'plan_measurement':{'body_bytes':len(jb(plan)),'digest':pd,'source_bytes':sum(len(jb(b)) for b in docs)},'totals':{'retained_envelopes':len(records),'operations':len(operations),'documents':len(committed_docs),'payload_bytes':total_b}}
path=P/'publication-transport-02-author.json'
if '--write' in sys.argv:path.write_text(json.dumps(output,indent=2)+'\n')
else:assert output==json.loads(path.read_text()),'author regeneration differs'
print(json.dumps({'status':'AUTHOR_REPRODUCTION_ONLY','totals':output['totals'],'plan':output['plan_measurement'],'output_sha256':sha(path.read_bytes())},sort_keys=True))
