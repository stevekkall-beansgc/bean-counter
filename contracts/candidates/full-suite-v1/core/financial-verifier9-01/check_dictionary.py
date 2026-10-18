"""Focused author dictionary consistency check; no production support body."""
import copy, hashlib, json
from pathlib import Path
from jsonschema import Draft202012Validator
p=Path(__file__).resolve().parent
load=lambda f:json.loads(f.read_text())
new=load(p/'financial-verifier9.schema.json'); old=load(p/'../economic-constructor-04.schema.json')
Draft202012Validator.check_schema(new)
names={'AuditVerifier','OriginRecipe','Artifact','Digest'}
assert set(new['$defs'])==names
reachable=set()
def walk(v):
    if isinstance(v,dict):
        if '$ref' in v:
            assert v['$ref'].startswith('#/$defs/')
            name=v['$ref'].removeprefix('#/$defs/')
            if name not in reachable:
                reachable.add(name);walk(new['$defs'][name])
        for k,x in v.items():
            if k!='$ref':walk(x)
    elif isinstance(v,list):
        for x in v:walk(x)
walk(new['oneOf']);assert reachable==names
assert new['oneOf']==old['oneOf'] and new['$schema']==old['$schema']
expected={n:copy.deepcopy(old['$defs'][n]) for n in names}
expected['OriginRecipe']['properties']['closure_profile']['enum'].append('ledger-economic-resolved-inputs/9')
assert new['$defs']==expected
removed=copy.deepcopy(new['$defs']);assert removed['OriginRecipe']['properties']['closure_profile']['enum'].pop()=='ledger-economic-resolved-inputs/9'
assert removed=={n:old['$defs'][n] for n in names}
delta=load(p/'schema-delta.json');assert len(delta['definition_changes'])==1
change=delta['definition_changes'][0]
assert change['old']==old['$defs']['OriginRecipe']['properties']['closure_profile']['enum']
assert change['new']==new['$defs']['OriginRecipe']['properties']['closure_profile']['enum']
for row in load(p/'source-pins.json')['sources']:
    raw=(p/row['path']).read_bytes();assert len(raw)==row['bytes'] and hashlib.sha256(raw).hexdigest()==row['sha256']
r=load(p/'registered-financial-tuples.json');reg=load(p/'../party-commitment-first-01/registry.json')
digest='sha256:'+hashlib.sha256(json.dumps(reg,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()
assert r['registry_digest']==digest
rows=[x for x in load(p/'../party-commitment-first-01/dispatch.json')['rows'] if x['saved_recipe']['closure_profile']=='ledger-economic-resolved-inputs/9']
assert r['tuple_count']==len(rows)==len(r['rows'])==13
nv=Draft202012Validator({'$ref':'#/$defs/OriginRecipe','$defs':new['$defs']});ov=Draft202012Validator({'$ref':'#/$defs/OriginRecipe','$defs':old['$defs']})
for actual,saved in zip(r['rows'],rows):
    recipe={'registry_digest':digest,'operation_kind':saved['operation_kind'],'payload_schema':saved['payload_schema'],'normalization_profile':'ledger-operation-normalization/2','closure_profile':'ledger-economic-resolved-inputs/9','variant':'all'}
    assert actual=={'origin_recipe':recipe,'recipe_id':saved['recipe_id'],'descriptor_schema':saved['descriptor_schema'],'saved_recipe':saved['saved_recipe']}
    nv.validate(recipe);assert not ov.is_valid(recipe)
    for v in range(4,9):
        wrong=dict(recipe,closure_profile=f'ledger-economic-resolved-inputs/{v}');assert not nv.is_valid(wrong)
    for v in (2,3):
        inherited=dict(recipe,closure_profile=f'ledger-economic-resolved-inputs/{v}');assert ov.is_valid(inherited) and nv.is_valid(inherited)
result={'status':'PASS','meaning':'Author dictionary consistency only; no production descriptor or executable qualification','meta_schema_valid':True,'reachable_definitions':sorted(names),'sole_definition_delta':change,'exact_registered_tuples':13,'old_schema_rejects_all_13_closure9_tuples':True,'new_schema_accepts_all_13_closure9_tuples':True,'closures4_through8_remain_rejected':True,'inherited_closure2_and3_shape_unchanged':True,'source_pins_verified':len(load(p/'source-pins.json')['sources']),'runtime_or_build_executed':False}
(p/'check-result.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result))
