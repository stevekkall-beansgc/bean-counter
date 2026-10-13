"""Author-side fixture derivation, NOT independent oracle or runtime compiler."""
import base64,hashlib,json
from pathlib import Path
P=Path(__file__).parent
x=json.loads((P/'publication-fixture-input.json').read_text())
def j(v):return json.dumps(v,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()
def raw(v):return hashlib.sha256(v).hexdigest()
def h(d,v,ver=1):return 'sha256:'+raw(('bean-counter/'+d+'/'+str(ver)).encode()+b'\0'+j(v))
def ordered(v):return sorted(v,key=j)
refs={};docs={}
kind={'facts':'workflow-fact-schema','context':'workflow-fact-schema','template':'workflow-slot-template','compiler':'workflow-compiler-descriptor','product':'workflow-product','policy':'workflow-policy'}
def subst(v):
 if isinstance(v,dict):
  if set(v)=={'fixture_ref'}:return refs[v['fixture_ref']]
  return {k:subst(w) for k,w in v.items()}
 if isinstance(v,list):return [subst(w) for w in v]
 return v
def docref(k,b):
 d=h('workflow-document',[b['schema'],b]);return {'kind':k,'id':'wdoc_'+d[7:],'digest':d}
for name in ['facts','context','template','compiler','product','policy']:
 b=subst(x['definitions'][name])
 if name=='compiler':b.update({k:'sha256:'+raw(v.encode()) for k,v in x['descriptor_digest_inputs'].items()})
 docs[name]=b;refs[name]=docref(kind[name],b)
sources=ordered(list(refs.values()))
rule=docs['policy']['rules'][0]
plan={'schema':'ledger-workflow-compiled-plan/1','compiler_ref':refs['compiler'],'compiler_semantics':'ledger-policy-compiler/1','evaluator':'ledger-workflow-evaluator/1','grammar':'ledger-policy-grammar/1','limits_profile':'workflow-limits/1','source_refs':sources,'activity_table':[{'activity':'fixture.report:completed','primitive':'work','fact_schema_ref':refs['facts'],'source_slot':'fixture.report:worker','funding_leg':'fixture.report:customer'}],'fact_slots':[{'index':'0','schema_ref':refs['facts'],'field':docs['facts']['fields'][0]}],'link_paths':[],'component_slots':docs['template']['components'],'rule_order':[{'policy_ref':refs['policy'],'rule_id':rule['rule_id']}],'typed_instructions':[{'opcode':'evaluate_rule','ordinal':'0','policy_ref':refs['policy'],'rule':rule,'fact_slots':['0'],'binding_fact_slots':[],'basis_instructions':[]}],'dependency_edges':[],'worst_case':{'fields':'1088','nodes':'256','hops':'16','loaded_refs':'4096','actions':'128','explanations':'256','bytes':'4194304','compile_work':'1048576','runtime_work':'65536'},'explanation_map':[{'ordinal':'0','policy_ref':refs['policy'],'rule_or_family':rule['rule_id'],'fields':['fixture.report:pages'],'primitive':'unit'}]}
pd=h('workflow-plan',plan);pr={'kind':'workflow-compiled-plan','id':'wplan_'+pd[7:],'digest':pd}
locators=ordered([{'role':'inline_document','inline_document':b} for b in docs.values()])
registry=json.loads((P/'registry-contract.json').read_text())
anchors=ordered([{'role':'inline_document','record_ref':refs[name],'relation':'publication_source','descriptor_refs':ordered([refs['facts'],refs['context']]) if name=='policy' else []} for name in docs])
descriptors=ordered([refs['facts'],refs['context'],refs['compiler']])
resolution={'schema':'ledger-operation-descriptors/1','registry_digest':'sha256:'+raw(j(registry)),'operation_kind':'policy.publish','payload_schema':'ledger-workflow-policy-publication/1','locator_digest':h('descriptor-locators',['policy.publish','ledger-workflow-policy-publication/1',locators]),'anchors':anchors,'descriptors':descriptors,'normalization_profile':'ledger-operation-normalization/2'}
resolution['closure_digest']=h('descriptor-closure',resolution)
key={'logical_store_id':x['store'],'scope':x['scope'],'source':x['source'],'operation_id':x['operation_id']}
request={'schema':'ledger-operation/1','key':key,'kind':'policy.publish','contract':'workflow/1','payload':{'schema':'ledger-workflow-policy-publication/1','mode':'inline','documents':ordered(list(docs.values())),'root_policy_ref':refs['policy'],'expected_plan_digest':pd}}
reqhash=h('operation-request',[resolution['closure_digest'],request],2)
evidence={'schema':'ledger-operation-request-evidence/2','normalization_profile':'ledger-operation-normalization/2','original_request_bytes_base64':base64.b64encode(j(request)).decode(),'original_request_bytes_sha256':raw(j(request)),'normalized_request':request,'request_hash':reqhash,'schema_bundle_digest':h('schema-bundle',descriptors),'descriptor_resolution':resolution}
pro=x['provenance_inputs'];pub={'schema':'ledger-workflow-publication/1','root_policy_ref':refs['policy'],'source_refs':sources,'compiled_plan_ref':pr,'compiler_ref':refs['compiler'],'provenance':{'source_sha':pro['source_sha'],'binary_sha256':raw(pro['binary_sha256_utf8'].encode()),'build_manifest_digest':'sha256:'+raw(pro['build_manifest_digest_utf8'].encode())}}
pubref={'kind':'workflow-publication','id':'wpub_'+h('workflow-publication',[x['store'],x['scope'],refs['policy']])[7:],'digest':h('workflow-record',['workflow-publication',x['scope'],pub])}
basis=x['trusted_authority_fixture'];auth={'schema':'ledger-workflow-local-authority-snapshot/1','basis':basis,'basis_digest':h('local-authority-basis',basis),'verification_profile':'local-private-directory-owner/1','verified_at':x['accepted_at']};ar=docref('workflow-authority',auth)
resultrefs=ordered([pr,pubref])
op={'schema':'ledger-workflow-operation-record/1','key':key,'kind':'policy.publish','contract':'workflow/1','request_evidence':evidence,'authority_snapshot':{'schema':'ledger-workflow-publication-authority/1','principal_id':x['principal'],'source':x['source'],'authority_refs':[ar],'rights':['admin'],'validated_at':x['accepted_at']},'accepted_at':x['accepted_at'],'commit_sequence':x['commit_sequence'],'result_refs':resultrefs}
opref={'kind':'workflow-operation','id':'wop_'+h('workflow-operation',key)[7:],'digest':h('workflow-record',['workflow-operation',x['scope'],op])}
manifest={'schema':'ledger-workflow-publication-manifest/1','key':key,'members':ordered(sources+[ar,pr,pubref,opref])};md=h('workflow-manifest',manifest);mr={'kind':'workflow-manifest','id':'wmanifest_'+md[7:],'digest':md}
receipt={'schema':'ledger-operation-receipt/2','key':key,'request_hash':reqhash,'request_hash_profile':'bean-counter/operation-request/2','normalization_profile':'ledger-operation-normalization/2','schema_bundle_digest':evidence['schema_bundle_digest'],'descriptor_closure_digest':resolution['closure_digest'],'original_request_bytes_sha256':evidence['original_request_bytes_sha256'],'result_kind':'administrative','result_refs':resultrefs,'accepted_at':x['accepted_at'],'commit_sequence':x['commit_sequence'],'manifest_hash':md};receipt['receipt_hash']=h('operation-receipt',receipt,2);rr={'kind':'workflow-receipt','id':'wreceipt_'+receipt['receipt_hash'][7:],'digest':receipt['receipt_hash']}
def env(ref,body):return {**ref,'logical_store_id':x['store'],'scope':x['scope'],'body':body}
records=ordered([env(refs[k],v) for k,v in docs.items()]+[env(pr,plan),env(pubref,pub),env(ar,auth),env(opref,op),env(mr,manifest),env(rr,receipt)])
outputs={'source_documents':docs,'source_refs':refs,'compiled_plan':plan,'compiled_plan_ref':pr,'descriptor_resolution':resolution,'normalized_request':request,'request_evidence':evidence,'authority_snapshot':auth,'authority_ref':ar,'publication':pub,'publication_ref':pubref,'operation_record':op,'operation_ref':opref,'manifest':manifest,'manifest_ref':mr,'receipt':receipt,'receipt_ref':rr,'records':records}
checks={k:{'canonical_utf8_hex':j(v).hex(),'raw_sha256':raw(j(v))} for k,v in outputs.items()}
(P/'publication-author-vectors.json').write_text(json.dumps({'authorship':'candidate author comparator only; independent reviewer derivation required','outputs':outputs,'byte_checks':checks},indent=2)+'\n')
print('author publication fixture derived: six definitions, plan, authority, publication, operation, manifest, receipt; no runtime proof')
