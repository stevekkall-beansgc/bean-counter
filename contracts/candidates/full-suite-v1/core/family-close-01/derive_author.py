#!/usr/bin/env python3
"""Author fixture derivation only. Not runtime authority or independent verification."""
import copy,hashlib,json
from pathlib import Path
P=Path(__file__).resolve().parent
J=lambda x:json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
D=lambda x:hashlib.sha256(J(x)).hexdigest()
H=lambda d,x:hashlib.sha256(('bean-counter/'+d+'/1\0').encode()+J(x)).hexdigest()
def doc(kind,body):
 h=H('workflow-document',[body['schema'],body]);return {'body':body,'ref':{'kind':kind,'id':'wdoc_'+h,'digest':'sha256:'+h},'canonical_hex':J(body).hex()}
def derive(i):
 out={'schema':'family-close-author-expected/1','qualification':'AUTHOR_ONLY_SYNTHETIC_CONDITIONAL_NO_RUNTIME_AUTHORITY','documents':{},'cases':[]}
 terms=doc('workflow-slot-terms',i['terms3_body']);out['documents']['terms3']=terms
 for variant in ['opted_in','absent','different_close_slot','different_nominated_source']:
  p=copy.deepcopy(i['policy2_body'])
  if variant=='absent':del p['families'][0]['no_success_close']
  if variant=='different_close_slot':p['families'][0]['no_success_close']['close_source_slot']='fixture.measure:closer'
  pd=doc('workflow-policy',p);pub=copy.deepcopy(i['publication_template']);pub['root_policy_ref']=pd['ref'];pub['source_refs']=[pd['ref']];scope=i['acceptance_terms_template']['scope'];pubref={'kind':'workflow-publication','id':'wpub_'+H('workflow-publication',['fixture-store',scope,pd['ref']]),'digest':'sha256:'+H('workflow-record',['workflow-publication',scope,pub])}
  at=copy.deepcopy(i['acceptance_terms_template']);at['publication_ref']=pubref;at['binding_inputs']['bindings'][0]['binding_ref']=terms['ref']
  if variant=='different_nominated_source':
   slot=p['families'][0]['no_success_close']['close_source_slot'];next(z for z in at['binding_inputs']['source_slots'] if z['name']==slot)['sources']=['urn:fixture:other']
  out['documents'][variant]={'policy':pd,'publication':{'body':pub,'ref':pubref,'canonical_hex':J(pub).hex()},'acceptance_terms':doc('workflow-acceptance-terms',at)}
 def evidence(a,v):
  a=doc('workflow-fact-attestation',a);v=copy.deepcopy(v);v['attestation_ref']=a['ref'];v['attestation_digest']=a['ref']['digest'];v['attestation_raw_sha256']=hashlib.sha256(J(a['body'])).hexdigest();v['subject_binding_refs'].sort(key=J);v=doc('workflow-evidence-verification',v);cl=copy.deepcopy(i['close_template']);cl['close_proof']={'attestation_ref':a['ref'],'verification_ref':v['ref']};return a,v,cl
 a,v,cl=evidence(i['assertion'],i['verification_template']);out['documents']['assertion']=a;out['documents']['verification']=v;out['close']={'body':cl,'canonical_hex':J(cl).hex()}
 for name,termsref in [('dependency_binding',i['dependency']['original_terms_ref']),('INVALID_dependency_slot_terms',terms['ref'])]:
  b=copy.deepcopy(i['dependency']);b['original_terms_ref']=termsref;id_='wdep_'+H('workflow-dependency',[b['ancestor_decision_id'],b['dependent_decision_id'],b['original_terms_ref']]);out['documents'][name]={'body':b,'ref':{'kind':'workflow-dependency','id':id_,'digest':'sha256:'+H('workflow-record',['workflow-dependency',b['operation_key']['scope'],b])},'canonical_hex':J(b).hex()}
 for case in i['cases']:
  data={'assertion':copy.deepcopy(i['assertion']),'verification':copy.deepcopy(i['verification_template']),'context':copy.deepcopy(i['context'])}
  for patch in case['patches']:
   obj=data
   for key in patch['path'][:-1]:obj=obj[key]
   obj[patch['path'][-1]]=patch['value']
  a,v,cl=evidence(data['assertion'],data['verification']);a0=a['body'];v0=v['body'];c=data['context'];n=c['original_nomination'];source=a0['source'];slot=n['close_source_slot']if n else None
  grants=all(g['source']==source and slot in g['source_slots'] and 'outcome.close'in g['operations'] and 'submit'in g['permissions']and 'outcome'in g['evidence_kinds'] for g in [c['original_grant'],c['current_grant']])
  subject={'logical_store_id':'fixture-store','scope':i['acceptance_terms_template']['scope'],'original_binding_ref':c['current_binding_ref'],'target_ref':cl['target_ref'],'family_slot':cl['family_slot'],'expected_family_revision':cl['expected_revision']}
  passed=bool(n and source in n['sources']and'outcome'in n['evidence_kinds']and grants and c['current_grant']['status']=='active'and c['accepted_at']<c['current_grant']['ends_at']and c['authenticated_source']==source==v0['verified_source']and c['authenticated_principal_ref']==v0['authenticated_principal_ref']and c['verified_retention_origin']and a0['schema']=='ledger-workflow-family-close-attestation/1'and a0['evidence_kind']=='outcome'and a0['purpose']=='family_no_success'and a0['assertion']=='no_success'and a0['subject']==subject and c['family_revision']==cl['expected_revision']and v0['subject_binding_refs']==sorted(c['subject_bindings'],key=J)and c['family_status']in ['open','awaiting_anchor']and c['previous_trusted_at']<=c['accepted_at']and a0['issued_at']<=v0['trusted_received_at']<=c['accepted_at'])
  out['cases'].append({'name':case['name'],'admissible_given_context':passed,'assertion_ref':a['ref'],'verification_ref':v['ref'],'normalized_close_hex':J(cl).hex()})
 for name in ['registry','profile','family-close.schema']:
  out[name+'_digest']='sha256:'+D(json.loads((P/(name+'.json')).read_text()))
 return out
if __name__=='__main__':
 i=json.loads((P/'fixture-input.json').read_text());(P/'fixture-author-expected.json').write_text(json.dumps(derive(i),indent=2)+'\n')
