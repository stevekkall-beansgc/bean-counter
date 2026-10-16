#!/usr/bin/env python3
from pathlib import Path
import copy,json,hashlib
import jsonschema
from derive_author import derive,jcs
P=Path(__file__).resolve().parent

def run():
 s=json.loads((P/'deadline-close.schema.json').read_text());i=json.loads((P/'fixture-input.json').read_text());e=json.loads((P/'fixture-author-expected.json').read_text());checks=[]
 def ck(n,v):
  if not v:raise AssertionError(n)
  checks.append(n)
 def valid(k,v):return jsonschema.Draft202012Validator({'$schema':s['$schema'],'$ref':'#/$defs/'+k,'$defs':s['$defs']}).is_valid(v)
 jsonschema.Draft202012Validator.check_schema(s);ck('schema_valid',True);ck('author_derivation_matches',derive()==e)
 ck('base_deadline_payload',valid('OutcomeClose2',i['payload']))
 for n in i['schema_negative_mutations']:
  p=copy.deepcopy(i['payload']);p[n['field']]=n['value'];ck(n['name'],not valid('OutcomeClose2',p))
 old=json.loads((P.parent/'owner-local-cost-01/owner-local-cost.schema.json').read_text());ck('same_definition_names',set(old['$defs'])==set(s['$defs']))
 for k,v in old['$defs'].items():ck('unchanged_'+k,v==s['$defs'][k])
 oldr=json.loads((P.parent/'owner-local-cost-01/registry.json').read_text());r=json.loads((P/'registry.json').read_text());q=copy.deepcopy(r);next(z for z in q['entries']if z['kind']=='outcome.close')['recipe_id']=next(z for z in oldr['entries']if z['kind']=='outcome.close')['recipe_id'];ck('only_registry_recipe_changed',q==oldr)
 for n in ['registry','profile']:
  x=json.loads((P/(n+'.json')).read_text());sc=json.loads((P/(n+'.schema.json')).read_text());ck(n+'_const',jsonschema.Draft202012Validator(sc).is_valid(x));ck(n+'_unknown_refuses',not jsonschema.Draft202012Validator(sc).is_valid({**x,'extra':1}))
 for c in e['cases']:
  ck(c['name']+'_slots_order',c['eligible_slots']==sorted(set(c['eligible_slots']),key=jcs))
  if c['accepted']:
   ck(c['name']+'_authority_schema',valid('ConnectedEconomicAuthority5',c['authority']['body']))
   ck(c['name']+'_selected_first',c['selected_slot']==c['eligible_slots'][0])
   ck(c['name']+'_rights',c['authority']['body']['rights']==['close','read'])
   ck(c['name']+'_release',int(c['proposed_state']['released_delta_atoms'])+int(c['proposed_state']['remaining_held_atoms'])==int(i['context']['held_atoms']))
  else:ck(c['name']+'_no_authority_or_state',c['selected_slot'] is None and 'authority'not in c and 'proposed_state'not in c)
 by={c['name']:c for c in e['cases']};ck('reorder_same_authority',by['ordinary_sufficient_no_submit']['authority']==by['original_slots_reordered']['authority']);ck('scheduler_no_privilege',by['ordinary_sufficient_no_submit']['authority']==by['same_authority_scheduler_label']['authority']);ck('different_sufficient_slot_changes_ref',by['ordinary_sufficient_no_submit']['authority']['ref']!=by['only_zeta_sufficient']['authority']['ref'])
 ck('positive_cases',set(c['name']for c in e['cases']if c['accepted'])=={'ordinary_sufficient_no_submit','same_authority_scheduler_label','awaiting_anchor_expired','original_slots_reordered','only_zeta_sufficient'})
 for f in json.loads((P/'source-pins.json').read_text())['files']:
  q=Path(f['path']);ck('pin_'+q.name,q.stat().st_size==f['bytes']and hashlib.sha256(q.read_bytes()).hexdigest()==f['sha256'])
 return {'status':'AUTHOR_CHECKS_PASS_PENDING_INDEPENDENT_FREEZE','checks':len(checks),'check_names':checks,'cases':len(e['cases']),'schema_negatives':len(i['schema_negative_mutations']),'product_execution':False}
if __name__=='__main__':
 a=run();(P/'author-check.json').write_text(json.dumps(a,indent=2)+'\n');print(json.dumps({k:v for k,v in a.items()if k!='check_names'}))
