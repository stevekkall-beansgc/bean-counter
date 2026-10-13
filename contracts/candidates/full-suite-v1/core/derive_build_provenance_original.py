"""Independent PUB-BUILD-01 source-input oracle. No implementation/author output."""
from pathlib import Path
from hashlib import sha256
from copy import deepcopy
import json,base64
P=Path(__file__).parent
S=Path('/Users/stephenkall/beans/worktrees/bean-counter-waves12-contracts-20261012/contracts/candidates/full-suite-v1/core/build-provenance-fixture-input.json')
def c(x):
 if isinstance(x,dict): return '{'+','.join(c(k)+':'+c(x[k]) for k in sorted(x,key=lambda k:k.encode('utf-16-be')))+'}'
 if isinstance(x,list):return '['+','.join(c(v) for v in x)+']'
 return json.dumps(x,ensure_ascii=False,separators=(',',':'))
def n(x):return len(c(x).encode())
def raw(b):return sha256(b).hexdigest()
def h(d,x):return 'sha256:'+raw(('bean-counter/'+d+'/1').encode()+b'\0'+c(x).encode())
def hv2(d,x):return 'sha256:'+raw(('bean-counter/'+d+'/2').encode()+b'\0'+c(x).encode())
def sort(x):return sorted(x,key=lambda v:c(v).encode())
def ref(k,p,d):return {'kind':k,'id':p+d[7:],'digest':d}
def doc(k,b):return ref(k,'wdoc_',h('workflow-document',[b['schema'],b]))
def vec(name,b,r=None):
 v={'name':name,'body':b,'canonical_utf8':c(b),'byte_length':n(b)}
 if r:v['ref']=r
 return v
src=json.loads(S.read_bytes());base=json.loads((P/'publication-runtime-catalog-expected.json').read_bytes());old={v['name']:deepcopy(v) for v in base['vectors']}
entries=[{'path':f['path'],'bytes':str(len(f['utf8'].encode())),'sha256':raw(f['utf8'].encode())} for f in src['source']['files']]
entries.sort(key=lambda e:e['path'].encode('utf-16-be'))
inv={'schema':'ledger-build-source-inventory/1','recipe':'bean-counter-build-inputs/1','entries':entries}
snapshot={k:src['source'][k] for k in ('base_commit','state')};snapshot.update(inventory_digest=h('build-source-inventory',inv),file_count=len(entries),total_bytes=str(sum(int(e['bytes']) for e in entries)))
recipe=deepcopy(src['recipe']);recipe['cargo_lock_digest']='sha256:'+next(e['sha256'] for e in entries if e['path']=='Cargo.lock');recipe['compiler_ref']=json.loads((P/'runtime-support-expected.json').read_bytes())['ref']
inputs={'schema':'ledger-workflow-build-inputs/1','source':snapshot,'recipe':recipe}
manifest={'schema':'ledger-workflow-build-manifest/1','build_inputs':inputs,'executable':{'sha256':raw(src['executable_utf8'].encode()),'bytes':str(len(src['executable_utf8'].encode())),'observation':'stable-current-executable-file/1'}}
br=doc('workflow-build-manifest',manifest);projection={'source_sha':snapshot['base_commit'],'binary_sha256':manifest['executable']['sha256'],'build_manifest_digest':'sha256:'+raw(c(manifest).encode())}
pub=old['publication']['body'];pub['provenance']=projection
key=old['operation']['body']['key'];scope=key['scope']
pr=ref('workflow-publication','wpub_',h('workflow-record',['workflow-publication',scope,pub]));pr['id']=old['publication']['ref']['id']
plan=old['unit-plan']['ref'];results=sort([plan,pr])
op=old['operation']['body'];op['result_refs']=results
ore=ref('workflow-operation','wop_',h('workflow-record',['workflow-operation',scope,op]));ore['id']=old['operation']['ref']['id']
member=[v['ref'] for name,v in old.items() if name not in ('publication','operation','manifest','receipt')]+[br,pr,ore]
vm={'schema':'ledger-workflow-publication-manifest/1','key':key,'members':sort(member)};mr=ref('workflow-manifest','wmanifest_',h('workflow-manifest',vm))
receipt=old['receipt']['body'];receipt.pop('receipt_hash');receipt.update(result_refs=results,manifest_hash=mr['digest']);rd=hv2('operation-receipt',receipt);receipt['receipt_hash']=rd;rr=ref('workflow-receipt','wreceipt_',rd)
records=[vec(name,v['body'],v['ref']) for name,v in old.items() if name not in ('publication','operation','manifest','receipt')]+[vec('build-manifest',manifest,br),vec('publication',pub,pr),vec('operation',op,ore),vec('manifest',vm,mr),vec('receipt',receipt,rr)]
docs=[v for v in records if v['name'] not in ('operation','manifest','receipt')];db=sum(v['byte_length'] for v in docs);wb=sum(n(x) for x in (key,op,vm,receipt))
# Independent new-key semantic alias, later verification time; original pub/build retained.
ak=deepcopy(key);ak['operation_id']='publish-002';at='2026-10-12T20:00:01.000000Z'
request=deepcopy(base['normalized_request']);request['key']=ak;rawrequest=c(request).encode();ev=deepcopy(base['request_evidence']);ev.update(normalized_request=request,original_request_bytes_base64=base64.b64encode(rawrequest).decode(),original_request_bytes_sha256=raw(rawrequest),request_hash=hv2('operation-request',[ev['descriptor_resolution']['closure_digest'],request]))
auth=deepcopy(old['authority-snapshot']['body']);auth['verified_at']=at;ar=doc('workflow-authority',auth)
aop=deepcopy(op);aop.update(key=ak,request_evidence=ev,accepted_at=at,commit_sequence='2');aop['authority_snapshot']['validated_at']=at;aop['authority_snapshot']['authority_refs']=[ar]
aor=ref('workflow-operation','wop_',h('workflow-record',['workflow-operation',scope,aop]));aor['id']='wop_'+h('workflow-operation',ak)[7:]
am={'schema':vm['schema'],'key':ak,'members':sort([r for r in vm['members'] if r['kind'] not in ('workflow-authority','workflow-operation')]+[ar,aor])};amr=ref('workflow-manifest','wmanifest_',h('workflow-manifest',am))
arcp=deepcopy(receipt);arcp.pop('receipt_hash');arcp.update(key=ak,request_hash=ev['request_hash'],original_request_bytes_sha256=ev['original_request_bytes_sha256'],result_kind='semantic_alias',accepted_at=at,commit_sequence='2',manifest_hash=amr['digest']);ard=hv2('operation-receipt',arcp);arcp['receipt_hash']=ard;arr=ref('workflow-receipt','wreceipt_',ard)
aw=sum(n(x) for x in (ak,aop,am,arcp));out={'status':'INDEPENDENT_SYNTHETIC_PUB_BUILD_01_EXPECTATIONS_NOT_BUILD_AUTHENTICITY','source_input_sha256':raw(S.read_bytes()),'inventory':vec('inventory',inv),'inventory_digest':snapshot['inventory_digest'],'build_inputs':vec('build-inputs',inputs),'projection':projection,'records':records,'member_count':len(vm['members']),'envelope_count':len(records),'fresh':{'new_documents':len(docs),'document_bytes':db,'wrapper_and_key_bytes':wb,'WB_delta':db+wb,'units':1},'alias':{'new_documents':1,'reused_documents':len(docs)-1,'document_bytes':n(auth),'wrapper_and_key_bytes':aw,'WB_delta':n(auth)+aw,'units':1,'original_build_manifest_ref':br,'records':[vec('authority-snapshot',auth,ar),vec('operation',aop,aor),vec('manifest',am,amr),vec('receipt',arcp,arr)]},'retry':{'units':0,'WB_delta':0}}
T=P/'build-provenance-expected.json';T.write_text(json.dumps(out,ensure_ascii=False,indent=2)+'\n');print(json.dumps({'expected_sha256':raw(T.read_bytes()),'inventory_digest':snapshot['inventory_digest'],'manifest_raw_digest':projection['build_manifest_digest'],'build_manifest_ref':br,'fresh':out['fresh'],'alias_delta':out['alias']['WB_delta'],'members':out['member_count'],'envelopes':out['envelope_count']},indent=2))
