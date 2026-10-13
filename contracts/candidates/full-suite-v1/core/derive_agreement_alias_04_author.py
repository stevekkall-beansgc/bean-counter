#!/usr/bin/env python3
"""Source-only author alias assembly. No reviewer or production output imports."""
import base64,copy,json,sys,hashlib
from pathlib import Path
P=Path(__file__).resolve().parent
ns={'__file__':str(P/'derive_agreement_03_author.py')}
src=(P/'derive_agreement_03_author.py').read_text();exec(compile(src[:src.index("for scenario in ['primary','expiry']:")],str(P/'derive_agreement_03_author.py'),'exec'),ns)
jb=ns['jb'];h=ns['h'];raw=ns['raw'];unique=ns['unique'];ordered=ns['ordered'];refs_in=ns['direct_refs'];docref=ns['doc_ref'];sem=ns['semantic_ref']
f=json.loads((P/'agreement-alias-04-fixture-input.json').read_text());reg=json.loads((P/'agreement-alias-04-registry.json').read_text());rd='sha256:'+raw(jb(reg));pub=ns['pub'];results=[]

def intent(payload):return {k:v for k,v in payload.items() if k!='documents'}
for scenario in f['scenarios']:
 base=ns['derive'](scenario['base_scenario']);named={r['name']:r['ref'] for r in base['records']};bodies={r['ref']['id']:(r['ref'],r['body']) for r in base['records']}
 for n,b in pub['source_documents'].items():bodies[pub['source_refs'][n]['id']]=(pub['source_refs'][n],b)
 for rk,bk in [('compiled_plan_ref','compiled_plan'),('publication_ref','publication'),('authority_ref','authority_snapshot')]:bodies[pub[rk]['id']]=(pub[rk],pub[bk])
 named['P1.authority']=pub['authority_ref'];named['P1.publication']=pub['publication_ref']
 for old in base['operations']:
  for k in ['operation','manifest','receipt']:bodies[old[k]['ref']['id']]=(old[k]['ref'],old[k]['body'])
 committed=set(bodies);out=[];documents=[]
 for row in scenario['operations']:
  original=next(o for o in base['operations'] if o['name']==row['original_name']);kind=original['operation']['body']['kind'];payload=copy.deepcopy(original['normalized_request']['payload'])
  if kind=='agreement.accept':payload['documents']=[]
  assert intent(payload)==intent(original['normalized_request']['payload'])
  key={**original['operation']['body']['key'],'operation_id':row['operation_id']};at=row['accepted_at']
  current=named[scenario['current_source_state_name']];st=bodies[current['id']][1]
  ob={'enrollment_ref':st['enrollment_ref'],'state_ref':current,'revision':st['revision'],'status':st['status']}
  if st['status']=='active':
   gr=st['current_grant_ref'];gb=bodies[gr['id']][1];time_status='not_started' if at<gb['starts_at'] else 'expired' if at>=gb['ends_at'] else 'valid';ob.update(grant_ref=gr,grant_time_status=time_status)
  assert ob['status']==scenario['expected_source_status'] and ob.get('grant_time_status')==scenario['expected_grant_time_status']
  old_results=copy.deepcopy(original['receipt']['body']['result_refs']);ir=next(r for r in old_results if r['kind']=='workflow-agreement-intent')
  authority={'schema':'ledger-workflow-agreement-authority/3','purpose':'semantic_alias','operation':kind,'actor':f['trusted_actor']['actor'],'source':f['trusted_actor']['source'],'scope':key['scope'],'customer':'customer-A','agreement_id':'agreement-A','local_admin_ref':named['P1.authority'],'original_operation_ref':original['operation']['ref'],'original_intent_ref':ir,'accepted_agreement_ref':named['A.accepted'],'binding_ref':named['A.binding'],'binding_source_namespace':ns['source_id'],'source_observation':ob,'observed_at':at}
  ar=docref('workflow-authority',authority);bodies[ar['id']]=(ar,authority);documents.append({'name':row['name']+'.authority','ref':ar,'body':authority,'canonical_hex':jb(authority).hex()})
  dr=copy.deepcopy(original['descriptor_resolution']);dr['registry_digest']=rd;dr.pop('closure_digest');dr['closure_digest']=h('descriptor-closure',dr)
  request={'schema':'ledger-operation/1','key':key,'kind':kind,'contract':'workflow/1','payload':payload};rh=h('operation-request',[dr['closure_digest'],request],2)
  ev={'schema':'ledger-operation-request-evidence/2','normalization_profile':'ledger-operation-normalization/2','original_request_bytes_base64':base64.b64encode(jb(request)).decode(),'original_request_bytes_sha256':raw(jb(request)),'normalized_request':request,'request_hash':rh,'schema_bundle_digest':h('schema-bundle',dr['descriptors']),'descriptor_resolution':dr}
  op={'schema':'ledger-workflow-agreement-alias-operation/1','key':key,'kind':kind,'contract':'workflow/1','request_evidence':ev,'authority_snapshot':authority,'accepted_at':at,'commit_sequence':row['commit_sequence'],'result_refs':old_results};opr=sem('workflow-operation','wop_','workflow-operation',key,op);bodies[opr['id']]=(opr,op)
  seen={};todo=[opr,ar]+old_results+[named['A.accepted']];allowed_ops={opr['id'],original['operation']['ref']['id']}
  while todo:
   r=todo.pop()
   if r['id'] in seen:continue
   assert r['kind']not in ['workflow-manifest','workflow-receipt']
   if r['kind']=='workflow-operation':assert r['id']in allowed_ops
   rr,b=bodies[r['id']];assert rr==r;seen[r['id']]=r;todo.extend(refs_in(b))
   if r==named['P1.publication']:todo.append(named['P1.authority'])
  members=unique(seen.values());man={'schema':'ledger-workflow-agreement-alias-manifest/1','key':key,'members':members};mh=h('workflow-manifest',man);mr={'kind':'workflow-manifest','id':'wmanifest_'+mh[7:],'digest':mh}
  receipt={'schema':'ledger-operation-receipt/2','key':key,'request_hash':rh,'request_hash_profile':'bean-counter/operation-request/2','normalization_profile':'ledger-operation-normalization/2','schema_bundle_digest':ev['schema_bundle_digest'],'descriptor_closure_digest':dr['closure_digest'],'original_request_bytes_sha256':ev['original_request_bytes_sha256'],'result_kind':'semantic_alias','result_refs':old_results,'accepted_at':at,'commit_sequence':row['commit_sequence'],'manifest_hash':mh};receipt['receipt_hash']=h('operation-receipt',receipt,2);rr={'kind':'workflow-receipt','id':'wreceipt_'+receipt['receipt_hash'][7:],'digest':receipt['receipt_hash']}
  fresh=[r for r in members if r['id']not in committed and r['kind']!='workflow-operation'];assert fresh==[ar]
  newbytes=sum(len(jb(bodies[r['id']][1])) for r in fresh);wb=sum(len(jb(v)) for v in [key,op,man,receipt]);committed.update(r['id'] for r in members)
  out.append({'name':row['name'],'original_name':row['original_name'],'original_operation_ref':original['operation']['ref'],'original_receipt_ref':original['receipt']['ref'],'locators':original['locators'],'descriptor_resolution':dr,'normalized_request':request,'request_evidence':ev,'authority_ref':ar,'operation':{'ref':opr,'body':op,'canonical_hex':jb(op).hex()},'manifest':{'ref':mr,'body':man,'canonical_hex':jb(man).hex()},'receipt':{'ref':rr,'body':receipt,'canonical_hex':jb(receipt).hex()},'resolved_member_body_bytes':sum(len(jb(bodies[r['id']][1])) for r in members),'meter':{'units':1,'new_document_count':len(fresh),'new_document_bytes':newbytes,'wrapper_bytes':wb,'wb_delta':newbytes+wb},'original_key_retry':{'receipt_ref':rr,'units':0,'wb_delta':0},'original_administrative_key_retry':{'receipt_ref':original['receipt']['ref'],'units':0,'wb_delta':0},'heads_before_and_after':scenario['heads_before_and_after']})
 results.append({'scenario':scenario['name'],'base_scenario':scenario['base_scenario'],'new_documents':documents,'operations':out,'all_member_envelopes':[{'ref':r,'body':bodies[r['id']][1],'canonical_hex':jb(bodies[r['id']][1]).hex()} for r in unique(r for o in out for r in o['manifest']['body']['members'])],'total_new_units':len(out),'total_wb_delta':sum(o['meter']['wb_delta'] for o in out)})
output={'schema':'agreement-alias-04-author-output/1','status':'AUTHOR_SOURCE_ONLY_ENCODING_PENDING_INDEPENDENT_REVIEW','input_sha256':raw((P/'agreement-alias-04-fixture-input.json').read_bytes()),'registry_digest':rd,'scenarios':results,'fresh_admission_after_source_loss':'AUTHORITY_REQUIRED; no alias branch without existing equal semantic intent','financial_admission_enabled':False}
if __name__=='__main__':
 b=(json.dumps(output,indent=2)+'\n').encode();p=P/'agreement-alias-04-author.json'
 if sys.argv[1:]==['--write']:p.write_bytes(b)
 else:assert not sys.argv[1:] and p.read_bytes()==b
 print(json.dumps({'sha256':raw(b),'scenarios':[(s['scenario'],s['total_new_units'],s['total_wb_delta']) for s in results]}))
