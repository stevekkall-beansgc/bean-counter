#!/usr/bin/env python3
"""Read-only schema/history consistency checks; neither independent oracle nor runtime proof."""
import copy,json,pathlib,base64,hashlib
from jsonschema import Draft202012Validator
P=pathlib.Path(__file__).resolve().parent
S=json.loads((P/'publication-transport-02.schema.json').read_text());Draft202012Validator.check_schema(S);V=Draft202012Validator(S)
A=json.loads((P/'publication-transport-02-author.json').read_text());n=0
for rec in A['records']:V.validate(rec);n+=1
for op in A['operations']:
 for x in [op['request'],op['descriptor_resolution'],{'schema':'ledger-read/1','kind':'operation.descriptors','payload':{'operation_kind':op['request']['kind'],'candidate':op['request']['payload']}}]:V.validate(x);n+=1
for pair in A['canonical_reads']:
 for x in [pair['request'],pair['response']]:V.validate(x);n+=1
V.validate(A['original_retry']['status']);n+=1
V.validate({'schema':'ledger-read-error/1','code':'RESPONSE_BUDGET_EXCEEDED'});n+=1
first=A['operations'][0]['request'];prepare={'schema':'ledger-read/1','kind':'operation.descriptors','payload':{'operation_kind':'evidence.retain','candidate':first['payload']}}
for mutation in [{'locators':[]},{'documents':[]},{'descriptor_refs':[]}]:
 wrong=copy.deepcopy(prepare);wrong['payload'].update(mutation);assert list(V.iter_errors(wrong));n+=1
ops=A['operations'];assert ops[4]['profile_b']['units']==1 and len(ops[4]['profile_b']['new_document_refs'])==1;assert ops[5]['profile_b']['units']==1 and len(ops[5]['profile_b']['new_document_refs'])==1
assert A['original_retry']['units']==0 and A['original_retry']['payload_bytes']==0
assert A['plan_measurement']['body_bytes']==1089536
print(json.dumps({'status':'AUTHOR_SHAPE_HISTORY_CHECKS_ONLY','checks':n,'envelopes':len(A['records']),'operations':len(ops),'profile_b_bytes':A['totals']['payload_bytes']}))
