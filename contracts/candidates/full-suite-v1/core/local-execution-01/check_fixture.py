#!/usr/bin/env python3
"""Focused dictionary and conditional fixture check, not a runtime test."""
import copy,json
from pathlib import Path
from derive_author import derive
P=Path(__file__).resolve().parent
schema=json.loads((P/'local-execution.schema.json').read_text())
base=json.loads((P.parent/'party-commitment-first-01/party-commitment-first.schema.json').read_text())
assert all(schema['$defs'][k]==v for k,v in base['$defs'].items())
for d in json.loads((P/'schema-delta.json').read_text())['deltas']:
 value=copy.deepcopy(base['$defs'][d['base_definition']])
 for patch in d['exact_patch']:
  path=patch['path'].split('/')[1:];target=value
  for field in path[:-1]:target=target[field]
  if patch['op']=='remove':del target[path[-1]]
  else:target[path[-1]]=patch['value']
 assert value==schema['$defs'][d['new_definition']]
assert derive()==json.loads((P/'fixture-expected.json').read_text())
print('PASS:333 unchanged definitions;11 exact deltas;conditional fixture reproduction. No runtime admission.')
