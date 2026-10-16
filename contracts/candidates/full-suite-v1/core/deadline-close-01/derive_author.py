#!/usr/bin/env python3
"""Pure synthetic contract derivation, not product authority or execution."""
from pathlib import Path
import copy,hashlib,json
from datetime import datetime
import jsonschema
P=Path(__file__).resolve().parent

def jcs(x):return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def sha(x):return hashlib.sha256(x).hexdigest()
def H(d,x):return sha(('bean-counter/'+d+'/1\0').encode()+jcs(x))
def instant(x):return datetime.strptime(x,'%Y-%m-%dT%H:%M:%S.%fZ')
def derive():
 i=json.loads((P/'fixture-input.json').read_text());s=json.loads((P/'deadline-close.schema.json').read_text());validator=jsonschema.Draft202012Validator({'$schema':s['$schema'],'$ref':'#/$defs/OutcomeClose2','$defs':s['$defs']});rows=[]
 for case in i['cases']:
  d=copy.deepcopy({'context':i['context'],'payload':i['payload']})
  for z in case['patches']:
   v=d
   for key in z['path'][:-1]:v=v[key]
   v[z['path'][-1]]=copy.deepcopy(z['value'])
  c,p=d['context'],d['payload'];at=instant(c['accepted_at']);og,cg=c['original_grant'],c['current_grant'];grants=[og,cg]
  slots=[]
  for slot in c['original_slots']:
   ok=all(slot[k] is True for k in ['nomination_original','role_terms_original','original_ceiling','original_scope_selectors','current_scope_selectors','same_source_role_association']) and c['authenticated_source'] in slot['sources']
   ok=ok and all(slot['name'] in g['source_slots'] and 'outcome.close' in g['operations'] and {'close','read'}<=set(g['permissions']) and instant(g['starts_at'])<=at<instant(g['ends_at']) for g in grants)
   if ok:slots.append(slot['name'])
  slots=sorted(set(slots),key=jcs)
  ok=validator.is_valid(p) and p['reason']=='deadline' and p['target_ref']==c['target_ref'] and p['family_slot']==c['family_slot'] and p['expected_revision']==c['family_revision'] and c['original_binding_ref']==i['fixed_original_binding_ref']
  ok=ok and all(c[k] is True for k in ['authenticated','source_association_verified','original_history_verified','snapshot_current','remaining_obligations_protected']) and c['current_state']=='active'
  ok=ok and all(g['principal']==c['principal'] and g['source']==c['authenticated_source'] for g in grants)
  ok=ok and instant(c['received_at'])<=at and instant(c['previous_trusted_at'])<=at and c['family_state'] in ['awaiting_anchor','open']
  if c['family_state'] in ['awaiting_anchor','open']:ok=ok and at>instant(c['activation_deadline'] if c['family_state']=='awaiting_anchor' else c['accepted_by'])
  ok=bool(ok and 0<=int(c['eligible_release_atoms'])<=int(c['held_atoms']) and slots)
  r={'name':case['name'],'accepted':ok,'eligible_slots':slots,'selected_slot':slots[0] if ok else None}
  if ok:
   a=copy.deepcopy(i['authority_template']);selected=next(z for z in c['original_slots'] if z['name']==slots[0]);a.update(source_slot=slots[0],actor=c['principal'],source=c['authenticated_source'],canonical_economic_namespace=c['source_namespace'],original_binding_ref=c['original_binding_ref'],original_grant_refs=sorted(selected['original_grant_refs'],key=jcs),observed_at=c['accepted_at'])
   digest=H('workflow-document',[a['schema'],a]);r['authority']={'body':a,'canonical_hex':jcs(a).hex(),'raw_sha':sha(jcs(a)),'ref':{'kind':'workflow-economic-authority','id':'wdoc_'+digest,'digest':'sha256:'+digest}}
   r['proposed_state']={'family_state':'expired','revision':str(int(c['family_revision'])+1),'released_delta_atoms':c['eligible_release_atoms'],'remaining_held_atoms':str(int(c['held_atoms'])-int(c['eligible_release_atoms']))}
  rows.append(r)
 return {'cases':rows,'normalized_payload':{'canonical_hex':jcs(i['payload']).hex(),'raw_sha':sha(jcs(i['payload']))},'metadata':{n:sha(jcs(json.loads((P/n).read_text())))for n in ['profile.json','registry.json','deadline-close.schema.json']},'registry_entries':json.loads((P/'registry.json').read_text())['entries']}
if __name__=='__main__':
 (P/'fixture-author-expected.json').write_text(json.dumps(derive(),indent=2)+'\n');print('Author expected written; independent freeze required.')
