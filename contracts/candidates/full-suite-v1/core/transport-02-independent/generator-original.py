"""Independent PUB-TRANSPORT-02 source-only oracle. No author assembler/output or production imports."""
from pathlib import Path
from hashlib import sha256
from copy import deepcopy
import json,base64
P=Path(__file__).parent
S=Path('/Users/stephenkall/beans/worktrees/bean-counter-waves12-contracts-20261012/contracts/candidates/full-suite-v1/core')
def c(x):
 if isinstance(x,dict):return '{'+','.join(c(k)+':'+c(x[k]) for k in sorted(x,key=lambda k:k.encode('utf-16-be')))+'}'
 if isinstance(x,list):return '['+','.join(c(v) for v in x)+']'
 return json.dumps(x,ensure_ascii=False,separators=(',',':'))
def raw(b):return sha256(b).hexdigest()
def h(d,x,v=1):return 'sha256:'+raw(('bean-counter/'+d+'/'+str(v)).encode()+b'\0'+c(x).encode())
def sort(x):return sorted(x,key=lambda y:c(y).encode())
def unique(x):return sort(list({c(y):y for y in x}.values()))
def n(x):return len(c(x).encode())
def docref(b):
 kind=b['schema'].removeprefix('ledger-').split('/')[0]
 if kind=='workflow-local-authority-snapshot':kind='workflow-authority'
 if kind=='workflow-definition-fragment-record':kind='workflow-definition-fragment'
 d=h('workflow-document',[b['schema'],b]);return {'kind':kind,'id':'wdoc_'+d[7:],'digest':d}
def special(k,p,d):return {'kind':k,'id':p+d[7:],'digest':d}
src=json.loads((S/'publication-transport-02-fixture-input.json').read_bytes())
assert raw((S/src['source_file']).read_bytes())==src['source_file_sha256']
assert raw((S/src['build_input_file']).read_bytes())==src['build_input_sha256']
large=json.loads((S/src['source_file']).read_bytes());source_docs=large['documents'];source_refs=unique([docref(b) for b in source_docs]);byid={docref(b)['id']:b for b in source_docs}
assert len(source_refs)==len(source_docs)==21
policy=next(b for b in source_docs if b['schema']=='ledger-workflow-policy/1');product=byid[policy['product_ref']['id']];template=byid[policy['slot_template_ref']['id']];policyref=docref(policy);compilerref=policy['compiler_ref']
# Closed selected true/unit compiler construction, independent from production.
assert not product['links'] and not policy['families'] and not policy['stages'] and not policy['delayed_costs']
active=unique([a['fact_schema'] for a in product['activities']]+[policy['context_schema_ref']]);slots=[]
for r in sorted(active,key=lambda r:r['digest']):
 for f in sorted(byid[r['id']]['fields'],key=lambda f:f['name']):slots.append({'index':str(len(slots)),'schema_ref':r,'field':f})
acts=sorted(product['activities'],key=lambda a:a['name']);actmap={a['name']:a for a in acts};instructions=[];explain=[];ruleorder=[]
for i,r in enumerate(policy['rules']):
 assert r['phase']=='base' and r['when']=={'op':'true'} and r['formula']['kind']=='unit'
 field=r['formula']['quantity_field'];schema=actmap[r['activity']]['fact_schema'];idx=next(s['index'] for s in slots if s['schema_ref']==schema and s['field']['name']==field)
 instructions.append({'opcode':'evaluate_rule','ordinal':str(i),'policy_ref':policyref,'rule':r,'fact_slots':[idx],'binding_fact_slots':[],'basis_instructions':[]})
 ruleorder.append({'policy_ref':policyref,'rule_id':r['rule_id']});explain.append({'ordinal':str(i),'policy_ref':policyref,'rule_or_family':r['rule_id'],'fields':[field],'primitive':'unit'})
plan={'schema':'ledger-workflow-compiled-plan/1','compiler_ref':compilerref,'compiler_semantics':'ledger-policy-compiler/1','evaluator':product['evaluator'],'grammar':product['grammar'],'limits_profile':product['limits_profile'],'source_refs':source_refs,'activity_table':[{'activity':a['name'],'primitive':a['primitive'],'fact_schema_ref':a['fact_schema'],'source_slot':a['source_slot'],'funding_leg':a['funding_leg']} for a in acts],'fact_slots':slots,'link_paths':[],'component_slots':sorted(template['components'],key=lambda a:a['key']),'rule_order':ruleorder,'typed_instructions':instructions,'dependency_edges':[],'worst_case':{'fields':'1088','nodes':'256','hops':'16','loaded_refs':'4096','actions':'128','explanations':'256','bytes':'4194304','compile_work':'1048576','runtime_work':'65536'},'explanation_map':explain}
planref=special('workflow-compiled-plan','wplan_',h('workflow-plan',plan))
# Exact synthetic build input bytes, distinct from any real build claim.
build=json.loads((S/src['build_input_file']).read_bytes());entries=[{'path':f['path'],'bytes':str(len(f['utf8'].encode())),'sha256':raw(f['utf8'].encode())} for f in build['source']['files']];entries.sort(key=lambda x:x['path'].encode('utf-16-be'))
inventory={'schema':'ledger-build-source-inventory/1','recipe':'bean-counter-build-inputs/1','entries':entries}
snapshot={k:build['source'][k] for k in ['base_commit','state']};snapshot.update(inventory_digest=h('build-source-inventory',inventory),file_count=len(entries),total_bytes=str(sum(int(e['bytes']) for e in entries)))
recipe=deepcopy(build['recipe']);recipe.update(cargo_lock_digest='sha256:'+next(e['sha256'] for e in entries if e['path']=='Cargo.lock'),compiler_ref=compilerref)
buildinputs={'schema':'ledger-workflow-build-inputs/1','source':snapshot,'recipe':recipe}
manifestbody={'schema':'ledger-workflow-build-manifest/1','build_inputs':buildinputs,'executable':{'sha256':raw(build['executable_utf8'].encode()),'bytes':str(len(build['executable_utf8'].encode())),'observation':'stable-current-executable-file/1'}}
buildref=docref(manifestbody);projection={'source_sha':snapshot['base_commit'],'binary_sha256':manifestbody['executable']['sha256'],'build_manifest_digest':'sha256:'+raw(c(manifestbody).encode())}
basis=src['trusted_local_basis'];store=basis['logical_store_id'];scope=basis['scope'];documents={};records={};operations=[]
registry={k:'sha256:'+raw(c(json.loads((S/f).read_bytes())).encode()) for k,f in [('source',src['retention_registry']),('publication',src['retained_publication_registry'].split()[0])]}
def add(ref,body):
 envelope={'kind':ref['kind'],'logical_store_id':store,'scope':scope,'id':ref['id'],'digest':ref['digest'],'body':body}
 if ref['id'] in records:assert records[ref['id']]==envelope
 records[ref['id']]=envelope

def deps(b):
 refs=[]
 if 'previous' in b:refs.append(b['previous'])
 if b['schema']=='ledger-workflow-product/1':refs+=b['fact_schemas']+[a['fact_schema'] for a in b['activities']]
 if b['schema']=='ledger-workflow-policy/1':refs+=[b[k] for k in ['product_ref','context_schema_ref','slot_template_ref','compiler_ref']]
 return unique(refs)
def closure(r):
 assert r['id'] in documents and docref(documents[r['id']])==r
 refs=[r]
 for d in deps(documents[r['id']]):refs+=closure(d)
 return unique(refs)
def descriptors(r):return [x for x in closure(r) if x['kind'] in ['workflow-fact-schema','workflow-compiler-descriptor']]
for req0,t in zip(src['requests'],src['timeline']):
 req=deepcopy(req0);pending={};source_pending=None;fragments=[];is_pub='fixture_assemble' in req
 if is_pub:
  req={'schema':'ledger-operation/1','key':{'logical_store_id':store,'scope':scope,'source':basis['source'],'operation_id':req0['operation_id']},'kind':'policy.publish','contract':'workflow/1','payload':{'schema':'ledger-workflow-policy-publication/1','mode':'retained','document_refs':source_refs,'root_policy_ref':policyref,'expected_plan_digest':planref['digest']}}
  assert all(r['id'] in documents and docref(documents[r['id']])==r for r in source_refs)
  assert closure(policyref)==source_refs
  pub={'schema':'ledger-workflow-publication/1','root_policy_ref':policyref,'source_refs':source_refs,'compiled_plan_ref':planref,'compiler_ref':compilerref,'provenance':projection}
  pubref=special('workflow-publication','wpub_',h('workflow-record',['workflow-publication',scope,pub]));pubref['id']='wpub_'+h('workflow-publication',[store,scope,policyref])[7:]
  results=unique([pubref,planref]);pending={r['id']:(r,b) for r,b in [(pubref,pub),(planref,plan),(buildref,manifestbody)]};member=source_refs+[pubref,planref,buildref]
  ds=unique(active+[compilerref]);loc=sort([{'role':'inline_document','ref':r} for r in source_refs]);anchors=sort([{'role':'inline_document','record_ref':r,'relation':'publication_source','descriptor_refs':active if r==policyref else []} for r in source_refs]);reg=registry['publication'];alias=False
 else:
  p=req['payload'];ps=p['schema'];reg=registry['source'];loc=[];anchors=[];ds=[]
  if ps=='ledger-workflow-definition-fragment/1':
   decoded=base64.b64decode(p['bytes_base64'],validate=True);assert base64.b64encode(decoded).decode()==p['bytes_base64'];total=int(p['document_bytes']);part=int(p['part']);assert part in [0,1] and 1<=total<=262144;assert len(decoded)==min(131072,total-part*131072)>0
   b={'schema':'ledger-workflow-definition-fragment-record/1','logical_store_id':store,'scope':scope,'source':basis['source'],**{k:v for k,v in p.items() if k!='schema'}};result=docref(b);member=[result]
  else:
   if ps=='ledger-workflow-definition-finalize/1':
    fragments=p['fragment_refs'];assert len(fragments) in [1,2] and fragments==unique(fragments);chunks=[]
    for r in fragments:
     assert r['id'] in documents and r==docref(documents[r['id']]);f=documents[r['id']];assert f['logical_store_id']==store and f['scope']==scope and f['source']==basis['source'] and f['upload_id']==p['upload_id'];chunks.append(f)
    chunks.sort(key=lambda x:int(x['part']));assert [x['part'] for x in chunks]==[str(i) for i in range(len(chunks))]
    for k in ['document_schema','canonical_document_sha256','document_bytes']:assert len({x[k] for x in chunks})==1
    combined=b''.join(base64.b64decode(x['bytes_base64'],validate=True) for x in chunks);b=json.loads(combined);assert c(b).encode()==combined and len(combined)==int(chunks[0]['document_bytes']) and raw(combined)==chunks[0]['canonical_document_sha256'] and b['schema']==chunks[0]['document_schema']
   else:assert ps=='ledger-workflow-definition-retain/1';b=p['document']
   result=docref(b);assert n(b)<=262144
   for r in deps(b):assert r['id'] in documents and r==docref(documents[r['id']])
   # Single validated candidate lookup; cannot shadow a committed different body.
   if result['id'] in documents:assert documents[result['id']]==b
   source_pending=result['id'] not in documents
   documents[result['id']]=b
   member=closure(result)+fragments
   for role,r,relation in [('inline_document',result,'publication_source')]+[('policy',r,'direct') for r in deps(b)]+[('evidence',r,'evidence_input') for r in fragments]:
    these=[] if role=='evidence' else descriptors(r);loc.append({'role':role,'ref':r});anchors.append({'role':role,'record_ref':r,'relation':relation,'descriptor_refs':these});ds+=these
   if source_pending:del documents[result['id']]
  alias=result['id'] in documents;results=[result];pending[result['id']]=(result,b);loc=unique(loc);anchors=unique(anchors);ds=unique(ds)
 key=req['key'];assert key['operation_id']==t['operation_id'];assert key['logical_store_id']==store and key['scope']==scope and key['source']==basis['source']
 authority={'schema':'ledger-workflow-local-authority-snapshot/1','basis':basis,'basis_digest':h('local-authority-basis',basis),'verification_profile':'local-private-directory-owner/1','verified_at':t['verified_at']};ar=docref(authority);pending[ar['id']]=(ar,authority);member.append(ar)
 res={'schema':'ledger-operation-descriptors/1','registry_digest':reg,'operation_kind':req['kind'],'payload_schema':req['payload']['schema'],'locator_digest':h('descriptor-locators',[req['kind'],req['payload']['schema'],loc]),'anchors':anchors,'descriptors':ds,'normalization_profile':'ledger-operation-normalization/2'};res['closure_digest']=h('descriptor-closure',res)
 rawreq=c(req).encode();assert len(rawreq)<=262144
 ev={'schema':'ledger-operation-request-evidence/2','normalization_profile':'ledger-operation-normalization/2','original_request_bytes_base64':base64.b64encode(rawreq).decode(),'original_request_bytes_sha256':raw(rawreq),'normalized_request':req,'request_hash':h('operation-request',[res['closure_digest'],req],2),'schema_bundle_digest':h('schema-bundle',ds),'descriptor_resolution':res}
 op={'schema':'ledger-workflow-operation-record/1','key':key,'kind':req['kind'],'contract':'workflow/1','request_evidence':ev,'authority_snapshot':{'schema':'ledger-workflow-publication-authority/1','principal_id':basis['principal_id'],'source':basis['source'],'authority_refs':[ar],'rights':['admin'],'validated_at':t['accepted_at']},'accepted_at':t['accepted_at'],'commit_sequence':t['commit_sequence'],'result_refs':results}
 oref=special('workflow-operation','wop_',h('workflow-record',['workflow-operation',scope,op]));oref['id']='wop_'+h('workflow-operation',key)[7:]
 m={'schema':'ledger-workflow-publication-manifest/1' if is_pub else 'ledger-workflow-definition-manifest/1','key':key,'members':unique(member+[oref])};mr=special('workflow-manifest','wmanifest_',h('workflow-manifest',m))
 receipt={'schema':'ledger-operation-receipt/2','key':key,'request_hash':ev['request_hash'],'request_hash_profile':'bean-counter/operation-request/2','normalization_profile':'ledger-operation-normalization/2','schema_bundle_digest':ev['schema_bundle_digest'],'descriptor_closure_digest':res['closure_digest'],'original_request_bytes_sha256':ev['original_request_bytes_sha256'],'result_kind':'semantic_alias' if alias else 'administrative','result_refs':results,'accepted_at':t['accepted_at'],'commit_sequence':t['commit_sequence'],'manifest_hash':mr['digest']};rd=h('operation-receipt',receipt,2);receipt['receipt_hash']=rd;rr=special('workflow-receipt','wreceipt_',rd)
 new=[(r,b) for ident,(r,b) in pending.items() if ident not in documents];db=sum(n(b) for _,b in new);wrap=sum(n(b) for b in [key,op,m,receipt]);delta={'units':1,'new_documents':len(new),'document_bytes':db,'wrapper_and_key_bytes':wrap,'WB_delta':db+wrap}
 for r,b in pending.values():
  if r['id'] in documents:assert documents[r['id']]==b
  documents[r['id']]=b;add(r,b)
 for r,b in [(oref,op),(mr,m),(rr,receipt)]:add(r,b)
 operations.append({'name':key['operation_id'],'normalized_request':req,'locators':loc,'descriptor_resolution':res,'request_evidence':ev,'result_refs':results,'operation_ref':oref,'manifest_ref':mr,'receipt_ref':rr,'members':m['members'],'delta':delta})
reads=[]
for r,b in [(docref(template),template),(planref,plan)]:
 br=c(b).encode();v={'schema':'ledger-workflow-document-read/2','ref':r,'canonical_bytes_base64':base64.b64encode(br).decode(),'canonical_bytes_sha256':raw(br),'body_bytes':str(len(br))};assert n(v)<8388608;reads.append(v)
retry=next(o for o in operations if o['name']==src['original_retry']['operation_id']);status={'schema':'ledger-operation-status/1','status':'committed','receipt':records[retry['receipt_ref']['id']]['body'],'request_evidence':retry['request_evidence']}
out={'status':'INDEPENDENT_SYNTHETIC_PUB_TRANSPORT_02_CANDIDATE_NO_RUNTIME_PASS','input_sha256':raw((S/'publication-transport-02-fixture-input.json').read_bytes()),'registries':registry,'inventory':inventory,'build_inputs':buildinputs,'envelopes':sort(list(records.values())),'operations':operations,'canonical_reads':reads,'original_retry':{'new_operations':0,'WB_delta':0,'status_result':status},'summary':{'records':len(records),'documents':len(documents),'operations':len(operations),'source_bytes':sum(n(b) for b in source_docs),'plan_bytes':n(plan),'plan_ref':planref,'WB_total':sum(o['delta']['WB_delta'] for o in operations)}}
T=P/'publication-transport-02-expected.json';T.write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'sha256':raw(T.read_bytes()),**out['summary']},indent=2))
