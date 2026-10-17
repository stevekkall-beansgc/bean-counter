#!/usr/bin/env python3
"""Author reproduction checks, not independent/runtime qualification."""
import hashlib,json
from pathlib import Path
import derive_author as a
P=Path(__file__).resolve().parent
checks=0
def require(v,label):
 global checks
 if not v:raise AssertionError(label)
 checks+=1
expected=a.load('fixture-expected.json');require(expected==a.OUT,'author reproduction')
for row in a.load('source-pins.json')['sources']:
 b=Path(row['path']).read_bytes();require(len(b)==row['bytes'] and hashlib.sha256(b).hexdigest()==row['sha256'],row['path'])
old=json.loads((P/'../app-cost-causal-01/app-cost-causal.schema.json').read_text())['$defs']
for k,v in old.items():require(a.SC['$defs'][k]==v,k)
reg=a.load('registry.json');d=a.load('dispatch.json');rows=[]
for e in reg['entries']:
 for payload in e['payload_schemas']:rows.append((e['kind'],payload,e['recipe_id'],e['saved_recipe']))
require(rows==[(r['operation_kind'],r['payload_schema'],r['recipe_id'],r['saved_recipe']) for r in d['rows']],'dispatch equality')
for n in ['profile','registry']:require(a.load(n+'.schema.json')['const']==a.load(n+'.json'),n+' exact metadata')
for n,r in a.OUT['records'].items():require(r['ref']['kind'] in reg['record_kinds'],n+' kind')
for case in a.OUT['cases'].values():require(case['accepted'] or all(not case[k] for k in ['derived_refs','audited_data_refs','commitment_refs']),'no refusal outputs')
terms=a.BASE['acceptance_terms']['ref'];auth=a.BASE['platform_authorization']['ref']
require(terms not in a.OUT['retention_closure']['reachable_refs'],'no prior terms dependency')
require(terms in a.OUT['retention_closure']['commitment_refs'],'exact commitment preserved')
require(auth in a.OUT['retention_closure']['reachable_refs'],'real authorization not cut')
require(terms in a.OUT['closure']['reachable_refs'],'consuming acceptance resolves basis')
for n in ['retain_commitment_without_terms','retain_unvalidated_economic_digest','accept_first_terms_atomically']:require(a.OUT['cases'][n]['accepted'],n)
for n in ['rehashed_evidence_wrong_digest','rehashed_wrong_terms_body','closure_missing_authorization','closure_missing_terms']:require(not a.OUT['cases'][n]['accepted'],n)
print(json.dumps({'status':'AUTHOR_CHECK_PASS_NOT_INDEPENDENT_OR_RUNTIME','checks':checks,'record_count':len(a.OUT['records']),'cases':len(a.OUT['cases']),'accepted_count':sum(c['accepted'] for c in a.OUT['cases'].values()),'schema_negatives':len(a.OUT['schema_negatives'])},indent=2))
