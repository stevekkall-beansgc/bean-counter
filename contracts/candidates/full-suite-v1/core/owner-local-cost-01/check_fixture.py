#!/usr/bin/env python3
"""Author contract checks; does NOT substitute for root's independent freeze."""
from pathlib import Path
import copy,json,hashlib
import jsonschema
from derive_author import derive,jcs,H,at
P=Path(__file__).resolve().parent

def run():
 s=json.loads((P/'owner-local-cost.schema.json').read_text());i=json.loads((P/'fixture-input.json').read_text());e=json.loads((P/'fixture-author-expected.json').read_text());checks=[]
 def check(name,ok):
  if not ok:raise AssertionError(name)
  checks.append(name)
 jsonschema.Draft202012Validator.check_schema(s);check('new_schema_valid',True)
 check('author_derivation_exact',derive()==e)
 def valid(definition,body):return jsonschema.Draft202012Validator({'$schema':s['$schema'],'$ref':'#/$defs/'+definition,'$defs':s['$defs']}).is_valid(body)
 defs={z['name']:z['definition'] for z in i['records']}
 for r in i['records']:check('body_'+r['name'],valid(r['definition'],e['records'][r['name']]['body']))
 check('initial_consumption_body',valid('CostConsumption1',e['initial_consumption']['body']));check('finalize_payload',valid('CostFinalize1',e['finalize']))
 for ix,n in enumerate(i['schema_negative_mutations']):
  b=copy.deepcopy(e['records'][n['record']]['body']);path=n.get('delete',n.get('path'));x=at(b,path[:-1])
  if 'delete'in n:del x[path[-1]]
  else:x[path[-1]]=n['value']
  check('negative_'+str(ix),not valid(defs[n['record']],b))
 prior=json.loads((P.parent/'family-close-01/family-close.schema.json').read_text())
 for k,v in prior['$defs'].items():check('preserved_definition_'+k,s['$defs'][k]==v)
 prov=json.loads((P/'definition-provenance.json').read_text());check('provenance_old_names',set(prov['copied_definitions'])==set(prior['$defs']));check('provenance_new_names',set(prov['new_definitions'])==set(s['$defs'])-set(prior['$defs']))
 for name in ['registry','profile']:
  body=json.loads((P/(name+'.json')).read_text());sc=json.loads((P/(name+'.schema.json')).read_text());check(name+'_exact_schema',jsonschema.Draft202012Validator(sc).is_valid(body));check(name+'_rejects_unknown',not jsonschema.Draft202012Validator(sc).is_valid({**body,'extra':True}))
 registry=json.loads((P/'registry.json').read_text());tuples=[]
 for en in registry['entries']:
  check('saved_tuple_fields_'+en['kind']+'_'+en['payload_schemas'][0],set(en['saved_recipe'])=={'authority_schema','operation_schema','manifest_schema','closure_profile','authority_audit_profile','financial_audit_profile'})
  for sc in en['payload_schemas']:tuples.append((en['kind'],sc))
 check('unambiguous_kind_payload_dispatch',len(tuples)==len(set(tuples)))
 old=json.loads((P.parent/'family-close-01/registry.json').read_text())
 connected={'evidence.retain','work.accept','link.assert','outcome.submit','outcome.correct','outcome.close','quantity.correct','work.reverse'}
 for en in old['entries']:
  if en['kind'] in ['agreement.accept','policy.publish']:continue
  matching=[z for z in registry['entries'] if z['kind']==en['kind'] and z['payload_schemas']==en['payload_schemas']]
  check('new_connected_tuple_'+en['kind'],len(matching)==1 and matching[0]['saved_recipe']['authority_schema']=='ledger-workflow-economic-authority/5' and matching[0]['saved_recipe']['operation_schema']=='ledger-workflow-economic-operation/6' and matching[0]['saved_recipe']['manifest_schema']=='ledger-workflow-economic-manifest/6' and matching[0]['saved_recipe']['closure_profile']=='ledger-economic-resolved-inputs/6')
  check('old_tuple_not_widened_'+en['kind'],matching[0]['saved_recipe']!=en['saved_recipe'])
 for field,value in s['$defs']['EconomicAuthority3']['properties'].items():
  if field in ['schema','operation']:continue
  check('connected_preserves_causal_authority_'+field,s['$defs']['ConnectedEconomicAuthority5']['properties'][field]==value)
 for definition in ['ConnectedOperation6','ConnectedManifest6','ConnectedEconomicAuthority5','CostDependencyClosureCheck','SupplierConnectedEnvelope','ConnectedRequestEvidence']:
  check('new_closed_definition_'+definition,definition in s['$defs'])
 edge={'owner_ref':e['records']['final_line']['ref'],'field':'prior_final_evidence_ref','target_ref':e['records']['final_line']['ref']}
 check('new_edge_field_grammar_only',valid('CostFinancialHistoryEdge',edge))
 edge['field']='supporting_evidence_refs';check('no_generic_provenance_cut_grammar',not valid('CostFinancialHistoryEdge',edge))
 closure={'cost_revision_ref':e['records']['cost_claim']['ref'],'expected_revision':'1','original_supplier_terms_ref':e['records']['supplier_terms']['ref'],'dependent_authority_ref':e['records']['cost_authority']['ref'],'authority_evidence_refs':[e['records']['final_line']['ref'],e['records']['verification']['ref']],'replacement_cost_proof':{'attestation_ref':e['records']['final_line']['ref'],'verification_ref':e['records']['verification']['ref']}}
 check('cost_closure_pair_shape_only',valid('CostDependencyClosureCheck',closure))
 del closure['replacement_cost_proof'];check('cost_closure_missing_pair_refuses',not valid('CostDependencyClosureCheck',closure))
 check('supplier_authority_retains_cost_closure','cost_closure_checks' in s['$defs']['CostEconomicAuthority4']['required'])
 check('only_one_conditional_accept',[x['name'] for x in e['conditional_cases'] if x['accepted']]==['valid_owner_local'])
 base=e['records']['final_line']['ref']
 for c in e['conditional_cases']:
  if c['name'] in ['wrong_subject_store','wrong_subject_work','wrong_account','new_physical_line','wrong_kind','nonfinal','over_ceiling','wrong_quantity','wrong_unit']:check('rehash_'+c['name'],c['assertion_ref']!=base and not c['accepted'])
 for t,inp in zip(e['high_water_traces'],i['high_water_traces']):
  prior_high={};prior_c=int(inp['other_consumed_atoms']);prior_h=int(inp['authorized_atoms'])-prior_c-int(inp['released_atoms'])
  for ix,row in enumerate(t['steps']):
   c=int(row['consumed_atoms']);h=int(row['held_atoms']);rel=int(row['released_atoms']);check(t['name']+'_conservation_'+str(ix),c+h+rel==int(inp['authorized_atoms']) and min(c,h,rel)>=0)
   check(t['name']+'_monotone_'+str(ix),c>=prior_c and all(int(row['high_waters'][k])>=v for k,v in prior_high.items()))
   if not row['accepted']:check(t['name']+'_refusal_unchanged_'+str(ix),{k:int(v) for k,v in row['high_waters'].items()}==prior_high and c==prior_c and h==prior_h)
   prior_high={k:int(v) for k,v in row['high_waters'].items()};prior_c=c;prior_h=h
 for h in e['money_histories']:
  old=int(h['initial']['retail_atoms']);mid=int(h['outcome_corrected']['retail_atoms']);new=int(h['cost_corrected']['retail_atoms'])
  check(h['name']+'_outcome_exact_actions',old+sum(map(int,h['outcome_correction_actions']))==mid)
  check(h['name']+'_cost_exact_actions',mid+sum(map(int,h['retail_cost_correction_actions']))==new)
  check(h['name']+'_supplier_exact_actions',int(h['initial']['supplier_atoms'])+sum(map(int,h['supplier_cost_correction_actions']))==int(h['cost_corrected']['supplier_atoms']))
  check(h['name']+'_independent_outcome',new-int(h['service_atoms'])-int(h['cost_corrected']['supplier_atoms'])==int(h['discount_atoms']))
 for z in e['identity_variants']:check('identity_'+z['name'],z['same_as_original']==(z['name'] in ['other_scope','other_invocation','credential_rotation']))
 for c in e['consent_diagnostics']:check('consent_digest_'+c['name'],c['terms_digest']!=e['acceptance_terms_digest'] and c['acceptance_terms']['ref']!=e['records']['acceptance_terms']['ref'])
 check('invalid_basis_refuses',not valid('SupplierTerms4',e['consent_diagnostics'][0]['supplier_terms']['body']))
 for f in json.loads((P/'source-pins.json').read_text())['files']:
  q=Path(f['path']);q=q if q.is_absolute() else P.parent/q;check('source_pin_'+f['path'],q.stat().st_size==f['bytes'] and hashlib.sha256(q.read_bytes()).hexdigest()==f['sha256'])
 return {'status':'AUTHOR_CONTRACT_CHECKS_PASS_NOT_INDEPENDENT_FREEZE','checks':len(checks),'check_names':checks,'conditional_cases':len(e['conditional_cases']),'schema_negatives':len(i['schema_negative_mutations']),'product_execution':False}
if __name__=='__main__':
 report=run();(P/'author-check.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps({k:v for k,v in report.items() if k!='check_names'}))
