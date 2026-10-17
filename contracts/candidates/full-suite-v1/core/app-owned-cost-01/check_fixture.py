#!/usr/bin/env python3
"""Author fixture integrity check; independent root review remains required."""
import hashlib,json
from pathlib import Path
from derive_author import output,P,validation
saved=json.loads((P/'fixture-expected.json').read_text())
assert output==saved,'author expected fixture drift'
for f in ['profile','registry']:
 import jsonschema
 jsonschema.Draft202012Validator(json.loads((P/(f+'.schema.json')).read_text())).validate(json.loads((P/(f+'.json')).read_text()))
pins=json.loads((P/'source-pins.json').read_text())
for x in pins['sources']:
 b=Path(x['path']).read_bytes();assert len(b)==x['bytes'] and hashlib.sha256(b).hexdigest()==x['sha256'],x['path']
print(json.dumps({'status':'PASS','scope':'author fixture/source consistency only; independent freeze and runtime qualification separate','records':len(output['records']),'conditional_cases':len(output['cases']),'conditional_accepts':sum(x['accepted'] for x in output['cases'].values()),'schema_negatives':len(output['schema_negatives']),'unchanged_old_definitions':output['copied_definition_count'],'source_pins':len(pins['sources'])},indent=2))
