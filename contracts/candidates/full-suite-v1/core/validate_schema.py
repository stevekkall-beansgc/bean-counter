"""Requires test-only jsonschema; no product runtime dependency."""
import json
from importlib.metadata import version
from pathlib import Path
from jsonschema import Draft202012Validator
p=Path(__file__).parent
s=json.loads((p/'workflow.schema.json').read_text())
Draft202012Validator.check_schema(s)
fixtures=json.loads((p/'shape-fixtures.json').read_text())
for case in fixtures:
 schema={'$schema':s['$schema'],'$defs':s['$defs'],'$ref':'#/$defs/'+case['definition']}
 assert Draft202012Validator(schema).is_valid(case['value']) == case['valid'],case['name']
print(f'jsonschema {version("jsonschema")}: Draft202012 metaschema and {len(fixtures)} shape fixtures passed; semantic freeze/runtime proof not established')
independent=p/'publication-independent-expected.json'
if independent.exists():
    value=json.loads(independent.read_text())
    validator=Draft202012Validator({'$defs':s['$defs'],'$ref':'#/$defs/PublicationBoundaryEnvelope'})
    for envelope in value['envelopes']:
        validator.validate(envelope)
    Draft202012Validator({'$defs':s['$defs'],'$ref':'#/$defs/OperationStatus'}).validate(value['status_result'])
    print(f"independent fixture: {len(value['envelopes'])} complete envelopes and committed status structurally valid")
