#!/usr/bin/env python3
"""Astra author assembly from literal source inputs only. No reviewer/production imports.
--write saves author outputs; default compares to saved author outputs, read-only.
"""
import base64,copy,hashlib,json,sys
from pathlib import Path
P=Path(__file__).resolve().parent
# Reuse the author's original source-only P1 recipe without executing its output writes.
namespace={'__file__':str(P/'derive_publication_fixture.py')}
source=(P/'derive_publication_fixture.py').read_text()
exec(compile(source[:source.index('checks=')],str(P/'derive_publication_fixture.py'),'exec'),namespace)
pub=namespace['outputs']
def cj(v):
 if isinstance(v,dict):return '{'+','.join(cj(k)+':'+cj(v[k]) for k in sorted(v,key=lambda z:z.encode('utf-16-be')))+'}'
 if isinstance(v,list):return '['+','.join(cj(z) for z in v)+']'
 return json.dumps(v,ensure_ascii=False,separators=(',',':'),allow_nan=False)
def jb(v):return cj(v).encode()
def raw(v):return hashlib.sha256(v).hexdigest()
def h(domain,v,version=1):return 'sha256:'+raw(f'bean-counter/{domain}/{version}'.encode()+b'\0'+jb(v))
def ordered(xs):return sorted(xs,key=jb)
def unique(xs):return ordered(list({cj(v):v for v in xs}.values()))
base=json.loads((P/'agreement-boundary-fixture-input.json').read_text())
ready=json.loads((P/'agreement-ready-fixture-input.json').read_text())
stop_input=json.loads((P/'agreement-stop-fixture-input.json').read_text())
registry=json.loads((P/'agreement-ready-registry.json').read_text())
registry_digest='sha256:'+raw(jb(registry))
S=base['scope'];store=base['store'];actor=ready['root']['actor'];source_id=base['source']
def doc_ref(kind,body):
 digest=h('workflow-document',[body['schema'],body]);return {'kind':kind,'id':'wdoc_'+digest[7:],'digest':digest}
def semantic_ref(kind,prefix,domain,key,body):
 return {'kind':kind,'id':prefix+h(domain,key)[7:],'digest':h('workflow-record',[kind,S,body])}
def direct_refs(v):
 if isinstance(v,dict):
  if set(v)=={'kind','id','digest'}:return [v]
  return [r for z in v.values() for r in direct_refs(z)]
 if isinstance(v,list):return [r for z in v for r in direct_refs(z)]
 return []

def derive(scenario):
 refs={};bodies={};names={};new=[];chains=[];registered={};accepted_by_binding={};committed=set()
 def keep(name,ref,body,is_new=True):
  refs[name]=ref;names[name]=body;bodies[ref['id']]=(ref,body)
  if is_new:new.append({'name':name,'ref':ref,'body':body,'canonical_hex':jb(body).hex()})
  return ref
 def document(name,kind,body):return keep(name,doc_ref(kind,body),body)
 for n,b in pub['source_documents'].items():keep('P1.'+n,pub['source_refs'][n],b,False)
 for name,rkey,bkey in [('plan','compiled_plan_ref','compiled_plan'),('publication','publication_ref','publication'),('authority','authority_ref','authority_snapshot')]:keep('P1.'+name,pub[rkey],pub[bkey],False)
 committed.update(bodies)
 def resolve(v):
  if isinstance(v,dict):
   if set(v)=={'fixture_ref'}:return refs[v['fixture_ref']]
   if set(v)=={'fixture_document'}:return names[v['fixture_document']]
   if set(v)=={'fixture_digest'}:return h('workflow-acceptance-terms',names[v['fixture_digest']])
   return {k:resolve(z) for k,z in v.items()}
  if isinstance(v,list):return [resolve(z) for z in v]
  return v
 def private(tag):
  if tag+'.terms' in refs:return
  entry=next(x for x in base['customers'] if x['tag']==tag)
  for n,k in [('role','workflow-roles'),('catalog','workflow-slot-catalog'),('instantiation','workflow-slot-instantiation'),('slot_terms','workflow-slot-terms')]:document(tag+'.'+n,k,resolve(entry[n]))
  inp={k:v for k,v in entry['binding_inputs'].items() if k not in ['assent_refs','authority_refs']}
  terms={'schema':'ledger-workflow-acceptance-terms/1','scope':S,'publication_ref':refs['P1.publication'],'agreement_id':'agreement-'+tag,'customer':'customer-'+tag,'version':'1','binding_inputs':resolve(inp)}
  document(tag+'.terms','workflow-acceptance-terms',terms)
 def assent(tag):
  if tag+'.assent' in refs:return
  document(tag+'.assent','workflow-assent',{'schema':'ledger-workflow-assent/1','scope':S,'agreement_id':'agreement-'+tag,'customer':'customer-'+tag,'terms_digest':h('workflow-acceptance-terms',names[tag+'.terms']),'mode':'synthetic','acceptor':'customer-'+tag,'accepted_at':'2026-10-13T00:00:00.000000Z','evidence_ref':refs[tag+'.assent_evidence'],'operator':actor,'operator_attestation_ref':refs[tag+'.operator_attestation']})
 def descriptors(kind,payload):
  loc=[];anchors=[];desc=[]
  def add(role,ref,relation,ds=[]):
   loc.append({'role':role,'ref':ref});anchors.append({'role':role,'record_ref':ref,'relation':relation,'descriptor_refs':ordered(ds)})
  alldesc=ordered([refs['P1.facts'],refs['P1.context'],refs['P1.compiler']])
  if kind=='agreement.accept':
   add('publication',payload['publication_ref'],'publication_source',alldesc)
   clean={k:v for k,v in payload.items() if k not in ['publication_ref','documents']}
   for ref in unique(direct_refs(clean)):
    context=ref==refs['P1.context'];add('private_document',ref,'context_schema' if context else 'agreement_input',[ref] if context else [])
   desc=alldesc
  elif kind in ['agreement.activate','agreement.revoke']:
   add('accepted_binding',payload['accepted_binding_ref'],'accepted_binding',alldesc)
   for k in ['transition_ref','replaces_transition_ref']:
    if k in payload:add('activation_target',payload[k],'activation_target')
   for ref in payload['evidence_refs']:add('private_document',ref,'agreement_input')
   desc=alldesc
  elif kind=='evidence.retain':
   if payload['purpose']=='operator_attestation':add('evidence',payload['metadata']['assent_evidence_ref'],'evidence_input')
   if payload['purpose']=='admission_stop':
    add('accepted_binding',payload['metadata']['binding_ref'],'accepted_binding');add('activation_target',payload['metadata']['target_transition_ref'],'activation_target')
  elif kind=='authority.change':
   action=payload['action']
   if action=='customer.register':add('evidence',payload['registration_evidence_ref'],'evidence_input')
   elif action=='source.enroll':
    for ref in payload['customer_refs']:add('customer',ref,'customer_registration')
    add('evidence',payload['evidence_ref'],'evidence_input')
   else:add('source_enrollment',payload['enrollment_ref'],'source_enrollment')
  loc=unique(loc);anchors=unique(anchors)
  dr={'schema':'ledger-operation-descriptors/1','registry_digest':registry_digest,'operation_kind':kind,'payload_schema':payload['schema'],'locator_digest':h('descriptor-locators',[kind,payload['schema'],loc]),'anchors':anchors,'descriptors':desc,'normalization_profile':'ledger-operation-normalization/2'}
  dr['closure_digest']=h('descriptor-closure',dr)
  return loc,dr
 def closure(start):
  found={};stack=list(start)
  while stack:
   ref=stack.pop()
   if ref['id'] in found:continue
   if ref['kind'] in ['workflow-manifest','workflow-receipt']:continue
   found[ref['id']]=ref
   stored,body=bodies[ref['id']];assert stored==ref
   stack.extend(direct_refs(body))
   if ref==refs['P1.publication']:stack.append(refs['P1.authority'])
  return unique(found.values())
 sequence=ready['operations']+(stop_input['operations'] if scenario=='primary' else stop_input['expiry_alternative_operations'])
 for operation in sequence:
  tag=operation['name'][0];name=operation['name'];kind=operation['kind'];at=operation['accepted_at'];seq=operation['commit_sequence']
  if name.endswith('.assent_evidence'):private(tag)
  if kind=='agreement.accept':assent(tag)
  payload=resolve(operation['payload'])
  if 'documents' in payload:payload['documents']=ordered(payload['documents'])
  key={'logical_store_id':store,'scope':S,'source':source_id,'operation_id':operation['operation_id']}
  authority={'schema':'ledger-workflow-publication-authority/1','principal_id':actor,'source':source_id,'authority_refs':[refs['P1.authority']],'rights':['admin'],'validated_at':at}
  results=[];extra=[]
  if kind=='evidence.retain':
   body={**payload,'schema':'ledger-workflow-retained-evidence/1','retained_by':actor,'retained_at':at,'installation_mode':'isolated_synthetic','authority_ref':refs['P1.authority']};results=[document(name,'workflow-evidence',body)]
  elif kind=='authority.change':
   action=payload['action']
   if action=='customer.register':
    body={'schema':'ledger-workflow-customer-registration/1','scope':S,'customer':payload['customer'],'acceptor':payload['acceptor'],'evidence_ref':payload['registration_evidence_ref'],'registered_by':actor,'registered_at':at,'authority_ref':refs['P1.authority'],'installation_mode':'isolated_synthetic'};results=[document(name,'workflow-customer',body)]
   elif action=='source.enroll':
    body={'schema':'ledger-workflow-source-enrollment/1','scope':S,'enrollment_id':payload['enrollment_id'],'customer_refs':payload['customer_refs'],'ceiling':payload['ceiling'],'evidence_ref':payload['evidence_ref'],'enrolled_by':actor,'enrolled_at':at,'authority_ref':refs['P1.authority'],'installation_mode':'isolated_synthetic'}
    enr=document(name,'workflow-source-enrollment',body)
    grant={'schema':'ledger-workflow-source-grant/1','grant_id':payload['enrollment_id'],'revision':'1',**payload['ceiling'],'original_ceiling_ref':enr}
    gr=document(tag+'.verified_grant','workflow-source-grant',grant)
    state={'schema':'ledger-workflow-source-state/1','enrollment_ref':enr,'revision':'1','status':'active','current_grant_ref':gr,'operation_key':key,'actor':actor,'accepted_at':at}
    sr=semantic_ref('workflow-source-state','wsource_','workflow-source-state',[store,S,payload['enrollment_id'],'1'],state);keep(tag+'.initial_source_state',sr,state);results=[enr,gr,sr]
   elif action=='source.revoke':
    state={'schema':'ledger-workflow-source-state/1','enrollment_ref':payload['enrollment_ref'],'revision':'2','status':'revoked','prior_state_ref':refs[tag+'.initial_source_state'],'operation_key':key,'actor':actor,'accepted_at':at}
    sr=semantic_ref('workflow-source-state','wsource_','workflow-source-state',[store,S,'enrollment-'+tag,'2'],state);keep(tag+'.revoked_source_state',sr,state);results=[sr]
   else:raise AssertionError(action)
  else:
   customer='customer-'+tag;agreement='agreement-'+tag
   if kind=='agreement.revoke':authority=resolve(operation['authority_snapshot'])
   else:authority={'schema':'ledger-workflow-agreement-authority/2','purpose':'admit','actor':actor,'source':source_id,'canonical_economic_namespace':source_id,'operation':kind,'scope':S,'customer':customer,'agreement_id':agreement,'local_admin_ref':refs['P1.authority'],'source_grant_refs':[refs[tag+'.verified_grant']],'assent_refs':[refs[tag+'.assent']],'observed_at':at,'source_revision':'1','source_state_refs':[refs[tag+'.initial_source_state']]}
   authref=document(name+'.authority','workflow-authority',authority);extra.append(authref)
   intent=document(name+'.intent','workflow-agreement-intent',{'schema':'ledger-workflow-agreement-intent/1','kind':kind,'payload':payload})
   if kind=='agreement.accept':
    binding={'schema':'ledger-workflow-binding/1',**payload['binding_inputs'],'policy_ref':refs['P1.policy'],'product_ref':refs['P1.product'],'fact_schema_refs':[refs['P1.facts']],'compiled_plan_ref':refs['P1.plan'],'evaluator':'ledger-workflow-evaluator/1','accepted_by':actor,'accepted_at':at}
    br=document(tag+'.binding','workflow-binding',binding)
    ac={'schema':'ledger-workflow-accepted-agreement/1','operation_key':key,'scope':S,'customer':customer,'agreement_id':agreement,'version':'1','publication_ref':refs['P1.publication'],'binding_ref':br,'terms_ref':refs[tag+'.terms'],'terms_digest':h('workflow-acceptance-terms',names[tag+'.terms']),'authority_observation_ref':authref,'accepted_at':at,'commit_sequence':seq}
    ar=semantic_ref('workflow-accepted-agreement','waccepted_','workflow-agreement-acceptance',[store,S,customer,agreement,'1'],ac);keep(tag+'.accepted',ar,ac);accepted_by_binding[br['id']]=ar
    headkind='agreement_acceptance';newrev='1';oldrev='0'
   else:br=refs[tag+'.binding'];headkind='agreement_activation';newrev='2' if kind=='agreement.revoke' else '1';oldrev='1' if kind=='agreement.revoke' else '0'
   head={'schema':'ledger-workflow-head-transition/1','head_kind':headkind,'scope':S,'customer':customer,'agreement_id':agreement,'operation_key':key,'actor':actor,'old_revision':oldrev,'new_revision':newrev,'accepted_binding_ref':br,'accepted_at':at,'commit_sequence':seq}
   if headkind=='agreement_activation':head['resolved_effective_time']=at
   if kind=='agreement.revoke':head['prior_head_ref']=refs[tag+'.activation_head']
   hr=semantic_ref('workflow-head-transition','whead_','workflow-head-transition',[store,S,customer,agreement,headkind,newrev],head);keep(name+'.head',hr,head)
   if kind=='agreement.accept':results=[intent,ar,hr];refs[tag+'.acceptance_head']=hr
   else:
    action='revoke' if kind=='agreement.revoke' else 'activate'
    transition={'schema':'ledger-workflow-activation-transition/1','action':action,'head_transition_ref':hr,'binding_ref':br,'effective_at':at,'commit_sequence':seq,'accepted_at':at}
    if action=='revoke':transition['target_transition_ref']=payload['transition_ref']
    tr=semantic_ref('workflow-activation','wactivation_','workflow-activation-transition',[store,S,customer,agreement,newrev],transition);keep(name+'.transition',tr,transition);results=[intent,hr,tr]
    if action=='activate':refs[tag+'.activation']=tr;refs[tag+'.activation_head']=hr
  results=ordered(results)
  loc,dr=descriptors(kind,payload)
  request={'schema':'ledger-operation/1','key':key,'kind':kind,'contract':'workflow/1','payload':payload}
  reqhash=h('operation-request',[dr['closure_digest'],request],2)
  evidence={'schema':'ledger-operation-request-evidence/2','normalization_profile':'ledger-operation-normalization/2','original_request_bytes_base64':base64.b64encode(jb(request)).decode(),'original_request_bytes_sha256':raw(jb(request)),'normalized_request':request,'request_hash':reqhash,'schema_bundle_digest':h('schema-bundle',dr['descriptors']),'descriptor_resolution':dr}
  op={'schema':'ledger-workflow-operation-record/1','key':key,'kind':kind,'contract':'workflow/1','request_evidence':evidence,'authority_snapshot':authority,'accepted_at':at,'commit_sequence':seq,'result_refs':results}
  opr=semantic_ref('workflow-operation','wop_','workflow-operation',key,op);keep(name+'.operation',opr,op)
  accepted_roots=[refs[tag+'.accepted']] if kind in ['agreement.activate','agreement.revoke'] else []
  members=closure([opr]+results+extra+accepted_roots)
  manifest={'schema':'ledger-workflow-agreement-manifest/1','key':key,'members':members};mh=h('workflow-manifest',manifest);mr={'kind':'workflow-manifest','id':'wmanifest_'+mh[7:],'digest':mh}
  receipt={'schema':'ledger-operation-receipt/2','key':key,'request_hash':reqhash,'request_hash_profile':'bean-counter/operation-request/2','normalization_profile':'ledger-operation-normalization/2','schema_bundle_digest':evidence['schema_bundle_digest'],'descriptor_closure_digest':dr['closure_digest'],'original_request_bytes_sha256':evidence['original_request_bytes_sha256'],'result_kind':'administrative','result_refs':results,'accepted_at':at,'commit_sequence':seq,'manifest_hash':mh};receipt['receipt_hash']=h('operation-receipt',receipt,2);rr={'kind':'workflow-receipt','id':'wreceipt_'+receipt['receipt_hash'][7:],'digest':receipt['receipt_hash']}
  fresh=[r for r in members if r['id'] not in committed and r['kind']!='workflow-operation']
  docbytes=sum(len(jb(bodies[r['id']][1])) for r in fresh)
  wrapperbytes=sum(len(jb(z)) for z in [key,op,manifest,receipt]);committed.update(r['id'] for r in members)
  chains.append({'name':name,'locators':loc,'descriptor_resolution':dr,'normalized_request':request,'request_evidence':evidence,'operation':{'ref':opr,'body':op},'manifest':{'ref':mr,'body':manifest},'receipt':{'ref':rr,'body':receipt},'meter':{'units':1,'new_document_count':len(fresh),'new_document_bytes':docbytes,'wrapper_bytes':wrapperbytes,'wb_delta':docbytes+wrapperbytes}})
 return {'authorship':'Astra author comparator derived only from literal source inputs and own P1 source recipe; no independent-oracle claim','scenario':scenario,'registry_digest':registry_digest,'records':new,'operations':chains,'record_count':len(new),'operation_count':len(chains)}
for scenario in ['primary','expiry']:
 output=derive(scenario);encoded=(json.dumps(output,indent=2,ensure_ascii=False)+'\n').encode();path=P/('agreement-03-'+scenario+'-author.json')
 if '--write' in sys.argv:path.write_bytes(encoded)
 else:assert path.read_bytes()==encoded,'Author reproduction differs '+scenario
 print(scenario,output['record_count'],'bodies',output['operation_count'],'operations',raw(encoded))
