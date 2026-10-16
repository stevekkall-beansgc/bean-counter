#!/usr/bin/env python3
"""Read-only author checks. No product code or fixture rewriting."""
import copy, hashlib, json
from pathlib import Path
from jsonschema import Draft202012Validator
P=Path(__file__).resolve().parent
J=lambda x:json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
def H(d,x):return 'sha256:'+hashlib.sha256(('bean-counter/'+d+'/1\0').encode()+J(x)).hexdigest()
i=json.loads((P/'fixture-input.json').read_text());e=json.loads((P/'fixture-author-expected.json').read_text());s=json.loads((P/'exposure-scope.schema.json').read_text());checks=0
for f in ['exposure-scope.schema.json','registry.schema.json','profile.schema.json']:
 Draft202012Validator.check_schema(json.loads((P/f).read_text()));checks+=1
for data,schema in [('registry.json','registry.schema.json'),('profile.json','profile.schema.json')]:
 Draft202012Validator(json.loads((P/schema).read_text())).validate(json.loads((P/data).read_text()));checks+=1
v=Draft202012Validator(s);v.validate(i['terms3_body']);v.validate(e['accept4_shape']);checks+=2
for mutation in i['negative_mutations']:
 b=copy.deepcopy(i['terms3_body'])
 if mutation['action']=='remove':b.pop(mutation['field'])
 else:b[mutation['field']]=mutation['value']
 assert not v.is_valid(b),mutation['name'];checks+=1
old=json.loads((P.parent/'causal-reversal-02.schema.json').read_text());oldv=Draft202012Validator({'$ref':'#/$defs/SlotBindingTerms2','$defs':old['$defs']});oldv.validate(i['terms2_body']);checks+=1
for key,body in [('terms3',i['terms3_body']),('old_terms2',i['terms2_body'])]:
 x=e[key];digest=H('workflow-document',[body['schema'],body]);assert x['body']==body and x['ref']=={'kind':'workflow-slot-terms','id':'wdoc_'+digest[7:],'digest':digest} and x['canonical_hex']==J(body).hex();checks+=1
for name,termskey in [('new_acceptance_terms','terms3'),('old_terms2_acceptance_terms','old_terms2')]:
 b=copy.deepcopy(i['acceptance_terms_template']);b['binding_inputs']['bindings'][0]['binding_ref']=e[termskey]['ref'];x=e[name];assert b==x['body'];assert H('workflow-document',[b['schema'],b])==x['ref']['digest'];assert J(b).hex()==x['canonical_hex'];Draft202012Validator({'$ref':'#/$defs/AcceptanceTerms','$defs':s['$defs']}).validate(b);checks+=1
assert e['new_acceptance_terms']['ref']['digest']!=e['old_terms2_acceptance_terms']['ref']['digest'];checks+=1
for c,want in zip(i['cases'],e['cases'],strict=True):
 out=[]
 for work in c['works']:
  states=[]
  for st in work['states']:
   b=sum(map(int,st['root_nets_atoms']));p=sum(max([0]+list(map(int,x)))for x in st['family_alternatives_atoms']);d=sum(max([0]+[-int(a)for a in x])for x in st['family_alternatives_atoms']);states.append({'positive_exposure_atoms':str(b+p),'discount_max_atoms':str(d),'discount_valid':d<=b})
  m=max(int(x['positive_exposure_atoms'])for x in states);out.append({'id':work['id'],'states':states,'maximum_exposure_atoms':str(m),'admissible':m<=int(c['ceiling_atoms'])and all(x['discount_valid']for x in states)})
 assert want=={'name':c['name'],'works':out,'all_admissible':all(x['admissible']for x in out)},c['name'];checks+=1
for path,key in [('registry.json','registry_digest'),('profile.json','profile_digest')]:assert 'sha256:'+hashlib.sha256(J(json.loads((P/path).read_text()))).hexdigest()==e[key];checks+=1
for pin in json.loads((P/'source-pins.json').read_text())['sources']:
 b=(P/pin['path']).read_bytes();assert len(b)==pin['bytes']and hashlib.sha256(b).hexdigest()==pin['sha256'];checks+=1
print(json.dumps({'status':'AUTHOR_CHECK_PASS','checks':checks,'scope':'schema/conditional arithmetic/hash/pins only; no product execution or independence'}))
