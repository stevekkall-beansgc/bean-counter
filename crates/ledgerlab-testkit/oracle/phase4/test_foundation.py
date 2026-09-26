"""Checker sensitivity only; actual store evidence is supplied by integration."""
import copy
import json
import unittest
from foundation import no_change, R3_SQLITE, R3_POSTGRES, R3_POSTGRES_V6
from observer import COMMON, PG_ONLY


def seed(backend):
    if backend == 'sqlite':
        snapshot = [[name, ['key', 'value'], ["'synthetic'|X'00'"]] for name in sorted(COMMON)]
        snapshot.append(['sqlite_schema', [], ["'table'|'example'|'CREATE TABLE example(...)'"]])
        snapshot.append(['user_version', ['value'], ['4']])
        metadata = {'user_version': ['4'], 'schema': copy.deepcopy(snapshot[-2][2])}
    else:
        snapshot = {name: {'columns': [['key', 'text', 'NO', None], ['value', 'bytea', 'YES', None]],
                           'rows': ['{"key":"synthetic","value":"\\\\x00"}']}
                    for name in sorted(COMMON | PG_ONLY)}
        snapshot['migration_history'] = {'columns': [['version','bigint','NO',None],['checksum','text','NO',None]], 'rows': [json.dumps({'version': version, 'checksum': 'synthetic'}) for version in range(1,5)]}
        metadata = {'schema': {'indexes':[['index','definition']], 'constraints':[['table','constraint','definition']], 'triggers':[]}}
    return {'backend': backend, 'B0': snapshot, 'B1': copy.deepcopy(snapshot), 'B2': copy.deepcopy(snapshot),
            'attempted_writes': 0, 'external_calls': 0,
            'metadata': {k: copy.deepcopy(metadata) for k in ('B0', 'B1', 'B2')}}


class FoundationInventory(unittest.TestCase):
    def test_unchanged_backend_shapes(self):
        for backend in ('sqlite', 'postgres17', 'postgres18'):
            no_change(seed(backend))

    def test_current_additive_schema_inventory_is_complete(self):
        for backend in ('sqlite', 'postgres17', 'postgres18'):
            for version in ((5,6,7,8,9) if backend == 'sqlite' else (5,6)):
                evidence = seed(backend)
                added = set(R3_SQLITE) if backend == 'sqlite' else R3_POSTGRES_V6 if version == 6 else R3_POSTGRES
                if backend == 'sqlite' and version >= 7: added |= {'billing_setup','billing_entries'}
                if backend == 'sqlite' and version >= 8: added |= {'billing_aliases','billing_permissions'}
                if backend == 'sqlite' and version >= 9: added |= {'billing_customers','billing_agreements','billing_m2_changes','billing_m2_permissions','billing_m2_entries','billing_m2_aliases'}
                for stage in ('B0','B1','B2'):
                    if backend == 'sqlite':
                        evidence[stage].extend([[name,['value'],["X'00'"]] for name in sorted(added)])
                        next(row for row in evidence[stage] if row[0]=='user_version')[2] = [str(version)]
                        evidence['metadata'][stage]['user_version'] = [str(version)]
                    else:
                        evidence[stage].update({name:{'columns':[['value','bytea','NO',None]],'rows':['{"value":"synthetic"}']} for name in added})
                        evidence[stage]['migration_history']['rows'].extend(json.dumps({'version':v,'checksum':'synthetic'}) for v in range(5,version+1))
                no_change(evidence)
                for name in added:
                    # Even simultaneous omission from every snapshot must fail;
                    # equality by itself is insufficient inventory evidence.
                    omitted = copy.deepcopy(evidence)
                    for stage in ('B0','B1','B2'):
                        if backend == 'sqlite': omitted[stage] = [r for r in omitted[stage] if r[0] != name]
                        else: del omitted[stage][name]
                    with self.assertRaises(AssertionError): no_change(omitted)
                    changed = copy.deepcopy(evidence)
                    if backend == 'sqlite': next(r for r in changed['B1'] if r[0] == name)[2].append("X'01'")
                    else: changed['B1'][name]['rows'].append('{"value":"changed"}')
                    with self.assertRaises(AssertionError): no_change(changed)
                unknown = copy.deepcopy(evidence)
                for stage in ('B0','B1','B2'):
                    if backend == 'sqlite':
                        next(row for row in unknown[stage] if row[0]=='user_version')[2] = ['10']
                        unknown['metadata'][stage]['user_version'] = ['10']
                    else: unknown[stage]['migration_history']['rows'].append(json.dumps({'version':7,'checksum':'synthetic'}))
                with self.assertRaises(AssertionError): no_change(unknown)

    def test_every_table_and_column_mutation(self):
        for backend in ('sqlite', 'postgres17', 'postgres18'):
            evidence = seed(backend)
            names = sorted(COMMON | (set() if backend == 'sqlite' else PG_ONLY))
            for stage in ('B1', 'B2'):
                for name in names:
                    for mutation in ('row', 'column', 'remove'):
                        changed = copy.deepcopy(evidence)
                        if backend == 'sqlite':
                            row = next(r for r in changed[stage] if r[0] == name)
                            if mutation == 'row': row[2].append("'extra'|NULL")
                            elif mutation == 'column': row[1].append('ignored-column')
                            else: changed[stage].remove(row)
                        else:
                            if mutation == 'remove': del changed[stage][name]
                            elif mutation == 'row': changed[stage][name]['rows'].append('{"key":"extra","value":null}')
                            else: changed[stage][name]['columns'].append(['ignored-column', 'text', 'YES', None])
                        with self.subTest(backend=backend, stage=stage, name=name, mutation=mutation), self.assertRaises(AssertionError):
                            no_change(changed)

    def test_equal_but_incomplete_inventory_refused(self):
        for backend in ('sqlite', 'postgres17', 'postgres18'):
            evidence = seed(backend)
            for stage in ('B0', 'B1', 'B2'):
                if backend == 'sqlite': evidence[stage] = [r for r in evidence[stage] if r[0] != 'actions']
                else: del evidence[stage]['actions']
            with self.assertRaises(AssertionError): no_change(evidence)

    def test_metadata_attempts_and_bool_zero_refused(self):
        for backend in ('sqlite', 'postgres17', 'postgres18'):
            evidence = seed(backend)
            for field in ('attempted_writes', 'external_calls'):
                for value in (1, False):
                    changed = copy.deepcopy(evidence); changed[field] = value
                    with self.assertRaises(AssertionError): no_change(changed)
            changed = copy.deepcopy(evidence)
            if backend == 'sqlite': changed['metadata']['B2']['schema'].append('new index')
            else: changed['metadata']['B2']['schema']['indexes'].append(['new','definition'])
            with self.assertRaises(AssertionError): no_change(changed)
            if backend == 'sqlite':
                changed = copy.deepcopy(evidence); changed['metadata']['B2']['user_version'] = 5
                with self.assertRaises(AssertionError): no_change(changed)


from foundation import (EXPECTED, ASSUMPTIONS, candidate_fingerprint, private_digest,
                        wire, report, check, load)


def report_seed(prefix=3):
    """Synthetic checker input only, never advertised as real adapter output."""
    p = {'semantics':'ledgerlab-private-comparison/1','snapshot':'a'*64,'activity':'b'*64,
         'source_build':'ledgerlab/0.0.0/supplier-foundation-1','assumptions':ASSUMPTIONS,'committed':False}
    result = {'status':'complete','steps':copy.deepcopy(EXPECTED['steps'][:prefix]),
              'latest':copy.deepcopy(EXPECTED['latest_by_prefix'][prefix])}
    original = {'key':'original','amounts':copy.deepcopy(EXPECTED['original_amounts']),'result':result}
    candidates=[]
    for key,amount in [('retail-a','-20'),('retail-b','-30')]:
        candidate=copy.deepcopy(original);candidate['key']=key
        next(row for row in candidate['amounts'] if row['agreement']=='agreement-retail' and row['code']=='rebate')['amount']['percent']['numerator']=amount
        candidates.append(candidate)
    value={'milestone':'PHASE-4 FOUNDATION ONLY','committed':False,'provenance':p,
           'basis':copy.deepcopy(EXPECTED['basis']),'bindings':copy.deepcopy(EXPECTED['bindings']),
           'historical_receipt_refs':[['synthetic:settlement-register','synthetic:base-acceptance'],['synthetic:settlement-ordinary','synthetic:economic-ordinary'],['synthetic:settlement-correction','synthetic:economic-correction'],['synthetic:settlement-close',None]][:prefix+1],
           'original':original,'candidates':candidates}
    rehash(value)
    return value


def rehash(value):
    p=value['provenance']
    for candidate in [value['original'],*value['candidates']]:
        candidate['fingerprint']=candidate_fingerprint(candidate,p)
    p['draft_candidates']=[c['fingerprint'] for c in value['candidates']]
    p['report_provenance']=private_digest('report',wire({k:v for k,v in p.items() if k!='report_provenance'}))


def leaves(value,path=()):
    if isinstance(value,dict):
        for key,child in value.items():yield from leaves(child,path+(key,))
    elif isinstance(value,list):
        for key,child in enumerate(value):yield from leaves(child,path+(key,))
    else:yield path


class FoundationReport(unittest.TestCase):
    def test_all_four_literal_prefixes(self):
        for prefix in range(4):
            self.assertEqual(report(report_seed(prefix)),prefix)

    def test_every_result_leaf_mutation_refused(self):
        for prefix in range(4):
            baseline=report_seed(prefix)
            for section in ('original','candidates'):
                indexes=[None] if section=='original' else [0,1]
                for index in indexes:
                    candidate=baseline[section] if index is None else baseline[section][index]
                    for path in leaves(candidate['result']):
                        changed=copy.deepcopy(baseline)
                        parent=changed[section] if index is None else changed[section][index]
                        parent=parent['result']
                        for key in path[:-1]:parent=parent[key]
                        parent[path[-1]]='MUTATED'
                        with self.subTest(prefix=prefix,section=section,index=index,path=path),self.assertRaises((AssertionError,TypeError,KeyError)):
                            report(changed)

    def test_basis_roles_and_noncommitted_sensitivity(self):
        baseline=report_seed()
        for path in [('basis','atoms'),('basis','currency'),('basis','scale'),('bindings',0,'roles','payer'),('bindings',1,'book'),('bindings',1,'premium_limit','atoms'),('committed',),('milestone',)]:
            changed=copy.deepcopy(baseline);parent=changed
            for key in path[:-1]:parent=parent[key]
            parent[path[-1]]='MUTATED'
            with self.assertRaises(AssertionError):report(changed)

    def test_candidate_and_report_digest_sensitivity(self):
        for section in ('original','candidate','report','activity'):
            changed=report_seed()
            if section=='original':changed['original']['fingerprint']='0'*64
            elif section=='candidate':changed['candidates'][0]['fingerprint']='0'*64
            elif section=='report':changed['provenance']['report_provenance']='0'*64
            else:changed['provenance']['activity']='0'*64
            with self.assertRaises(AssertionError):report(changed)

    def test_rehashed_supplier_change_and_membership_attack(self):
        for mutation in ('supplier','missing','extra','duplicate'):
            changed=report_seed();rows=changed['candidates'][0]['amounts']
            if mutation=='supplier':next(r for r in rows if r['code']=='fee')['amount']['fixed']['atoms']='1600'
            elif mutation=='missing':rows.pop()
            elif mutation=='extra':rows.append({'agreement':'agreement-retail','family':'new-family','code':'extra','amount':{'fixed':{'currency':'USD','scale':2,'atoms':'300'}}})
            else:rows[0]=copy.deepcopy(rows[1])
            rehash(changed)
            with self.assertRaises(AssertionError):report(changed)

    def test_order_and_label_invariance(self):
        value=report_seed();value['candidates'].reverse()
        for candidate in value['candidates']:
            candidate['key']='same-display-label';candidate['amounts'].reverse()
            for key in ('bindings','families','projected_reservations'):candidate['result']['latest'][key].reverse()
        value['bindings'].reverse();rehash(value)
        self.assertEqual(report(value),3)

    def test_missing_or_promoted_fields_refused(self):
        for field in report_seed():
            changed=report_seed();del changed[field]
            with self.assertRaises(AssertionError):report(changed)
        changed=report_seed();changed['receipt']={'committed':True}
        with self.assertRaises(AssertionError):report(changed)
        changed=report_seed();changed['candidates'][0]['result']={'status':'infeasible','reason':'FORGED','ordinal':None}
        with self.assertRaises(AssertionError):report(changed)

    def test_exact_inventory_envelope(self):
        for backend in ('sqlite','postgres17','postgres18'):
            evidence=seed(backend);evidence['report']=report_seed()
            self.assertEqual(check(evidence,True),3)

    def test_duplicate_json_refused_and_null_preserved(self):
        with self.assertRaises(AssertionError):load('{"committed":true,"committed":false}')
        self.assertEqual(load('{"inverse":null}'),{'inverse':None})

    def test_existing_source_inputs_match_literal_basis(self):
        # Read unchanged source inputs, never production output as expectations.
        from pathlib import Path
        import json
        root=Path(__file__).resolve().parents[4]
        golden=json.loads((root/'contracts/candidates/v2/goldens/supplier-separation.json').read_text())
        base=next(r['body'] for r in golden['seed'] if r['kind']=='base-evaluation')
        original=json.loads(base['original_evaluation_utf8'])
        policies=original['bundle']['policies']
        self.assertEqual(policies[0]['rules'][0]['operation'],{'Base':{'Fixed':'100'}})
        self.assertEqual(len(policies[0]['rules']),1)
        self.assertEqual(policies[1]['rules'][0]['operation'],{'Base':{'Fixed':'30'}})
        self.assertEqual(EXPECTED['basis']['atoms'],'10000')

if __name__ == "__main__": unittest.main()
