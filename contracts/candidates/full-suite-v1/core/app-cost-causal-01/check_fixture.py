#!/usr/bin/env python3
"""Recheck author conditional output and immutable input/source bindings only."""
import hashlib,json
from pathlib import Path
import derive_author as a
P=Path(__file__).resolve().parent
R=Path('/Users/stephenkall/beans/reviews/bean-counter-2026-10-12/waves12/committed-bill-sprint-20261016')
checks=[]
def check(name,v):
 if not v:raise AssertionError(name)
 checks.append(name)
def pin(p):
 b=Path(p['path']).read_bytes();return len(b)==p['bytes'] and hashlib.sha256(b).hexdigest()==p['sha256']
expected=json.loads((P/'fixture-expected.json').read_text())
check('recomputed_author_output',a.output==expected)
for p in json.loads((R/'APP-COST-CAUSAL-INPUT-FIRST-HANDOFF-2.json').read_text())['files']:check('input:'+Path(p['path']).name,pin(p))
for p in a.load('source-pins.json')['sources']:check('source:'+p['path'],pin(p))
old=a.load('../app-owned-cost-01/app-owned-cost.schema.json')['$defs']
for n,v in old.items():check('unchanged_definition:'+n,a.SC['$defs'][n]==v)
reg=a.load('registry.json');kinds=reg['record_kinds']
for rows in [a.F['records'],a.F['pending_records']]:
 for r in rows:check('registered_literal:'+r['name'],r['kind'] in kinds)
for name in ['workflow-economic-authority','workflow-capacity-retirement','workflow-verification']:check('registered_derived:'+name,name in kinds)
for e in reg['entries']:
 if e['saved_recipe']['closure_profile']!='absent':check('tuple:'+e['kind'],e['saved_recipe']=={'authority_schema':'ledger-workflow-economic-authority/7','operation_schema':'ledger-workflow-economic-operation/8','manifest_schema':'ledger-workflow-economic-manifest/8','closure_profile':'ledger-economic-resolved-inputs/8','authority_audit_profile':'ledger-authority-origin-audit/1','financial_audit_profile':'ledger-financial-origin-audit/1'})
check('fresh_count',sum(v['disposition']=='fresh_causal_inverse' for v in expected['cases'].values())==7)
check('replay_count',sum(v['disposition']=='saved_replay' for v in expected['cases'].values())==1)
check('pending_count',sum(v['accepted'] for v in expected['pending_cases'].values())==1)
check('schema_negatives',all(v['refused'] for v in expected['schema_negatives']))
check('original_money',expected['money']['original']['totals_atoms']==['3250000000000000000','2800000000000000000','2400000000000000000'])
check('policy_data_variant',expected['money']['policy_variant']['totals_atoms']==['3500000000000000000','2960000000000000000','2560000000000000000'])
check('zero_keeps_anchor',expected['timing_cases']['zero_initial_same_anchor']['correction_until']==expected['timing_cases']['correction_equal']['correction_until'])
check('non_sliding',expected['timing_cases']['corrected_revision_does_not_slide']['disposition']=='refused')
report={'status':'PASS','checks':len(checks),'expected_sha256':hashlib.sha256((P/'fixture-expected.json').read_bytes()).hexdigest(),'scope':'Author conditional derivation and immutable input/source agreement only. Root independent freeze still required; no product/runtime qualification.'}
(P/'author-check.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
