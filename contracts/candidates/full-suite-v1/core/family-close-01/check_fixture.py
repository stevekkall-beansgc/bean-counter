#!/usr/bin/env python3
"""Read-only author contract checks; no product execution or independent claim."""
import copy,hashlib,json
from pathlib import Path
from jsonschema import Draft202012Validator
from derive_author import derive
P=Path(__file__).resolve().parent;B=P.parent
read=lambda p:json.loads(p.read_text())
s=read(P/'family-close.schema.json');i=read(P/'fixture-input.json');e=read(P/'fixture-author-expected.json');checks=0
for n in ['family-close.schema.json','profile.schema.json','registry.schema.json']:
 Draft202012Validator.check_schema(read(P/n));checks+=1
for n in ['profile','registry']:
 Draft202012Validator(read(P/(n+'.schema.json'))).validate(read(P/(n+'.json')));checks+=1
v=lambda name:Draft202012Validator({'$ref':'#/$defs/'+name,'$defs':s['$defs']})
v('Policy2').validate(i['policy2_body']);v('OutcomeFamily2').validate(i['policy2_body']['families'][0]);v('SlotBindingTerms3').validate(i['terms3_body']);checks+=3
for name,b in [('FamilyCloseAttestation',e['documents']['assertion']['body']),('EvidenceVerification',e['documents']['verification']['body']),('OutcomeClose2',e['close']['body']),('OutcomeFamilyDependency',i['dependency']),('FamilyCloseAttest',{'schema':'ledger-workflow-family-close-attest/1','attestation':i['assertion']})]:v(name).validate(b);checks+=1
for name in ['opted_in','absent','different_close_slot','different_nominated_source']:
 for shape,key in [('Policy2','policy'),('Publication','publication'),('AcceptanceTerms','acceptance_terms')]:v(shape).validate(e['documents'][name][key]['body']);checks+=1
bases={'OutcomeFamily2':i['policy2_body']['families'][0],'FamilyCloseAttestation':i['assertion'],'OutcomeClose2':e['close']['body'],'OutcomeFamilyDependency':i['dependency']}
for mutation in i['schema_negative_mutations']:
 b=copy.deepcopy(bases[mutation['definition']]);path=mutation.get('path',mutation.get('delete'));o=b
 for key in path[:-1]:o=o[key]
 if 'delete'in mutation:del o[path[-1]]
 else:o[path[-1]]=mutation['value']
 assert not v(mutation['definition']).is_valid(b),mutation;checks+=1
assert e==derive(i);checks+=1
assert [x['name']for x in e['cases']if x['admissible_given_context']]==['open_valid','awaiting_valid'];checks+=1
for variant in ['absent','different_close_slot','different_nominated_source']:
 assert e['documents'][variant]['acceptance_terms']['ref']!=e['documents']['opted_in']['acceptance_terms']['ref'];checks+=1
assert e['documents']['dependency_binding']['ref']['id']!=e['documents']['INVALID_dependency_slot_terms']['ref']['id'];assert not v('OutcomeFamilyDependency').is_valid(e['documents']['INVALID_dependency_slot_terms']['body']);checks+=2
for pin in read(P/'source-pins.json')['files']:
 raw=(B/pin['path']).read_bytes();assert len(raw)==pin['bytes']and hashlib.sha256(raw).hexdigest()==pin['sha256'];checks+=1
for item in read(P/'definition-provenance.json')['copied_definitions']:
 assert s['$defs'][item['name']]==read(B/item['source'])['$defs'][item['name']];checks+=1
old=read(B/'exposure-scope-01/registry.json');new=read(P/'registry.json');ne={x['kind']:x for x in new['entries']}
for ent in old['entries']:
 assert ne[ent['kind']]['saved_recipe']==ent['saved_recipe'];checks+=1
for key in old:
 if key!='entries':assert old[key]==new[key];checks+=1
assert ne['outcome.close']['payload_schemas']==['ledger-workflow-outcome-close/2'];checks+=1
print(json.dumps({'status':'AUTHOR_CONTRACT_CHECK_PASS','checks':checks,'conditional_cases':len(e['cases']),'schema_negatives':len(i['schema_negative_mutations']),'qualification':'Author schema/source-copy/conditional relation/hash checks only. Not independent review, authentication, resource proof or product execution.'}))
