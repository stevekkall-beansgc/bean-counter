import copy
import itertools
import json
import pathlib
import subprocess
import sys
import unittest
from fractions import Fraction
from reference import (Refusal, bounded, rounded, exact, digest, candidate_digest,
                       compare, encoding, strict, evaluate)
from observer import COMMON, PG_ONLY, no_change, assert_result

HERE = pathlib.Path(__file__).resolve().parent
DATA = json.loads((HERE/'stories.json').read_text())


class Oracle(unittest.TestCase):
    def setUp(self):
        self.source = copy.deepcopy(DATA['source'])
        self.candidates = copy.deepcopy(DATA['candidates'])
        self.anchor = digest('source',self.source)

    def results(self, candidates=None, mode='hypothetical'):
        return compare(self.source,self.anchor,candidates or self.candidates,mode)

    def test_literal_complete_chronology(self):
        for candidate, actual in zip(self.candidates,self.results()):
            assert_result(actual,self.source,candidate,DATA['expected'][candidate['label']])

    def test_literal_candidate_differences_at_every_step(self):
        results=self.results(); original=results[0]['steps']
        for candidate,result in zip(self.candidates,results):
            expected=DATA['expected'][candidate['label']]
            for i,(row,base) in enumerate(zip(result['steps'],original)):
                self.assertEqual(str(int(row['delta'])-int(base['delta'])),expected['step_difference_vs_original'][i])
                self.assertEqual(str(int(row['retail'])-int(base['retail'])),expected['retail_difference_vs_original'][i])

    def test_no_change_matrix_is_complete_and_not_product_evidence(self):
        rows=json.loads((HERE/'no-change-matrix.json').read_text())
        self.assertEqual([r['id'] for r in rows],[f'N{i:02}' for i in range(1,17)])
        self.assertTrue(all(r['evidence']=='integration-required' for r in rows))

    def test_candidate_order_and_duplicates(self):
        expected={r['candidate_digest']:r for r in self.results()}
        orders=list(itertools.permutations(self.candidates))+[
            [self.candidates[i] for i in [0,1,0]], [self.candidates[i] for i in [1,0]],
            [self.candidates[1]]*8]
        for candidates in orders:
            for result in self.results(candidates): self.assertEqual(result,expected[result['candidate_digest']])

    def test_fresh_process(self):
        code="import json; from reference import compare,digest; x=json.load(open('stories.json')); print(json.dumps(compare(x['source'],digest('source',x['source']),x['candidates']),sort_keys=True))"
        result=subprocess.check_output([sys.executable,'-B','-c',code],cwd=HERE)
        self.assertEqual(json.loads(result),self.results())

    def test_original_replay_requires_original_terms(self):
        results=self.results(mode='original-replay')
        self.assertEqual([r['status'] for r in results],['complete','refused','refused'])
        self.assertEqual(results[0]['steps'][1]['retail'],'9700')
        self.assertEqual(results[1]['diagnostic'],'ORIGINAL_TERMS_REQUIRED')

    def test_future_description_and_correction_are_distinct(self):
        future=self.results(mode='future-terms-description')
        correction=self.results(mode='authorized-correction-description')
        self.assertIn('future activation',future[1]['prerequisites'])
        self.assertEqual(correction[1]['status'],'refused')
        self.assertIn('exact current revision',correction[0]['prerequisites'])
        self.assertTrue(all(r['committed'] is False for r in future+correction))

    def test_no_mutation(self):
        before=copy.deepcopy((self.source,self.candidates))
        self.results()
        self.assertEqual((self.source,self.candidates),before)

    def test_capacity_no_replenishment(self):
        for result in self.results():
            self.assertEqual([r['source_capacity'] for r in result['steps']],
                [['12000','3000','0']]*2+[['9500','5500','0']]*6+[['0','5500','9500']])
            self.assertEqual(result['steps'][4]['supplier'],'3000')
            self.assertEqual(result['steps'][4]['source_capacity'],['9500','5500','0'])

    def test_zero_inverse_and_reversal_explanations(self):
        rows=self.results()[0]['steps']
        self.assertEqual(rows[3]['nonzero_components'],['-2500'])
        self.assertEqual(rows[5]['nonzero_components'],['1000'])
        self.assertEqual(rows[6]['nonzero_components'],['-1000'])
        self.assertEqual(rows[6]['reason'],'CLAIM_REVERSED')
        self.assertEqual(rows[7]['inverse'],'0')

    def test_signed_rounding_literals(self):
        for n,d,want in [(1,2,1),(-1,2,-1),(49,100,0),(-49,100,0),(51,100,1),(-51,100,-1),(0,1,0),(21,2,11)]:
            with self.subTest(n=n,d=d): self.assertEqual(rounded(Fraction(n,d)),want)

    def test_percentage_original_retail_supplier_basis(self):
        self.assertEqual(exact({'percent':['10','1']},8000),Fraction(800))
        self.assertEqual(exact({'percent':['-75','2']},8000),Fraction(-3000))
        self.assertEqual(exact({'percent':['-40','1']},8000),Fraction(-3200))
        self.assertEqual(exact({'percent':['10','1']},0),Fraction(0))

    def test_language_neutral_exact_vectors(self):
        for vector in json.loads((HERE/'arithmetic.json').read_text()):
            value=exact(vector['rule'],int(vector['basis']))
            self.assertEqual([str(value.numerator),str(value.denominator)],vector['rational'])
            self.assertEqual(str(rounded(value)),vector['rounded'])

    def test_malformed_candidate_isolated(self):
        for bad in [None,[],{}, {'label':'x','terms':self.source['terms'],'rules':None}]:
            actual=self.results([self.candidates[1],bad,self.candidates[1]])
            self.assertEqual(actual[0],actual[2])
            self.assertEqual(actual[1]['status'],'refused')

    def test_gross_limits(self):
        for fee,rebate,category in [('5001','0','PREMIUM_LIMIT'),('5000','-8001','DISCOUNT_CAPACITY')]:
            bad=copy.deepcopy(self.candidates[0])
            bad['rules']['F']['success']={'fixed':fee}; bad['rules']['R']['quality']={'fixed':rebate}
            self.assertEqual(self.results([bad,self.candidates[0]])[0]['diagnostic'],category)
        good=copy.deepcopy(self.candidates[0]);good['rules']['F']['success']={'fixed':'5000'}
        self.assertEqual(self.results([good,good])[0]['status'],'complete')

    def test_invalid_candidate_isolation(self):
        bad=copy.deepcopy(self.candidates[1]);bad['rules']['F']['success']={'network':'file:///secret'}
        result=self.results([self.candidates[1],bad,self.candidates[1]])
        self.assertEqual(result[0],result[2]);self.assertEqual(result[1]['status'],'refused')
        self.assertNotIn('steps',result[1]);self.assertNotIn('file:',encoding(result[1]).decode())

    def test_changed_base_roles_scope_and_supplier_refuse(self):
        for field,value in [('retail_basis','9600'),('currency','EUR'),('scale',3),('unit','token'),('scope',['other','synthetic']),('retail_premium','9999')]:
            bad=copy.deepcopy(self.candidates[0]);bad['terms'][field]=value
            self.assertEqual(self.results([bad,self.candidates[0]])[0]['diagnostic'],'UNSUPPORTED_CHANGE')
        bad=copy.deepcopy(self.candidates[0]);bad['rules']['S']['bonus']={'fixed':'800'}
        self.assertEqual(self.results([bad,bad])[0]['diagnostic'],'SUPPLIER_TERMS_CHANGE')

    def test_membership_change_refuses(self):
        for mutation in ['new-family','missing-code']:
            bad=copy.deepcopy(self.candidates[0])
            if mutation=='new-family': bad['rules']['loyalty']={'extra':{'fixed':'300'}}
            else: del bad['rules']['F']['zero']
            self.assertEqual(self.results([bad,bad])[0]['diagnostic'],'MEMBERSHIP_CHANGE')

    def test_count_bounds(self):
        for n in [0,1,9]:
            with self.assertRaisesRegex(Refusal,'CANDIDATE_LIMIT'):
                compare(self.source,self.anchor,[self.candidates[0]]*n)
        self.assertEqual(len(self.results([self.candidates[0]]*8)),8)

    def test_overflow_precision_and_noncanonical_ratio(self):
        maximum=10**30-1
        self.assertEqual(bounded(maximum),maximum)
        for v in [maximum+1,-maximum-1]:
            with self.assertRaisesRegex(Refusal,'ARITHMETIC_OVERFLOW'): bounded(v)
        with self.assertRaisesRegex(Refusal,'ARITHMETIC_OVERFLOW'): rounded(Fraction(1,2**512))
        for rule in [{'fixed':'0.1'},{'fixed':'-0'},{'percent':['2','2']},{'percent':['1','0']},{'percent':['1','-2']},{'fixed':None}]:
            with self.assertRaises(Refusal): exact(rule,8000)

    def test_strict_parse(self):
        for raw in [b'{"x":1,"x":2}',b'{"x":null}',b'\xef\xbb\xbf{}',b'"\\ud800"',b'{"x":1e1}',b'{"x":9007199254740992}',b'{"x":-0}',b'NaN',b'\xff']:
            with self.subTest(raw=raw),self.assertRaises(Refusal): strict(raw)
        self.assertEqual(strict(b'{"atoms":"-1000"}'),{'atoms':'-1000'})

    def test_original_anchor_substitution_and_missing_family(self):
        for field in ['amount','family','receipt']:
            changed=copy.deepcopy(self.source)
            if field=='amount':changed['base_components'][0]='12000'
            elif field=='family':del changed['terms']['families']['R']
            else:changed['receipt_refs'].pop()
            self.assertNotEqual(digest('source',changed),self.anchor)
            with self.assertRaisesRegex(Refusal,'SOURCE_INTEGRITY'):compare(changed,self.anchor,self.candidates)

    def test_incomplete_and_unavailable(self):
        for key,value,want in [('complete',False,'INCOMPLETE'),('semantics','latest','REPLAY_UNAVAILABLE'),('steps',self.source['steps'][:-1],'INCOMPLETE')]:
            source=copy.deepcopy(self.source);source[key]=value
            with self.assertRaisesRegex(Refusal,want):compare(source,digest('source',source),self.candidates)

    def test_cap_composition_refuses(self):
        self.source['terms']['base_cap']=True
        with self.assertRaisesRegex(Refusal,'OUTCOME_CAP_COMPOSITION'):compare(self.source,digest('source',self.source),self.candidates)

    def test_stale_revision(self):
        self.source['steps'][3]['revision']='3'
        self.assertEqual(compare(self.source,digest('source',self.source),self.candidates)[0]['diagnostic'],'STALE_REVISION')

    def test_structured_identity_and_labels(self):
        self.assertNotEqual(digest('key',['ab','c']),digest('key',['a','bc']))
        self.assertNotEqual(digest('key','é'),digest('key','e\u0301'))
        renamed=copy.deepcopy(self.candidates[0]);renamed['label']='other'
        self.assertEqual(candidate_digest(renamed),candidate_digest(self.candidates[0]))
        self.candidates[1]['label']='P0'
        self.assertNotEqual(candidate_digest(self.candidates[0]),candidate_digest(self.candidates[1]))

    def test_result_assertion_sensitivity_every_leaf(self):
        actual=self.results()[0]
        def leaves(value,path=()):
            if isinstance(value,dict):
                for k,v in value.items(): yield from leaves(v,path+(k,))
            elif isinstance(value,list):
                for i,v in enumerate(value):yield from leaves(v,path+(i,))
            else:yield path
        for path in leaves(actual):
            changed=copy.deepcopy(actual);parent=changed
            for key in path[:-1]:parent=parent[key]
            parent[path[-1]]='MUTATED'
            with self.subTest(path=path),self.assertRaises(AssertionError):
                assert_result(changed,self.source,self.candidates[0],DATA['expected']['P0'])
        for key in actual:
            changed=copy.deepcopy(actual);del changed[key]
            with self.assertRaises(AssertionError):assert_result(changed,self.source,self.candidates[0],DATA['expected']['P0'])

    def test_coverage_ids(self):
        rows=json.loads((HERE/'coverage.json').read_text())
        self.assertEqual(len(rows),66)
        self.assertEqual({r['id'] for r in rows},{f'{p}{i:02}' for p,n in [('F',12),('A',29),('D',25)] for i in range(1,n+1)})
        self.assertTrue(all(r['no_change']=='all-protected-state' for r in rows))
        self.assertEqual(next(r for r in rows if r['id']=='F05')['first_slice'],'unsupported')


class NoChange(unittest.TestCase):
    def snapshot(self,backend):
        return {'tables':{name:{'columns':['key','value'],'rows':[[['text','synthetic'],['blob','00']]]} for name in COMMON|(PG_ONLY if backend!='sqlite' else set())},'schema':['exact DDL and index inventory'],'controls':['user_version or migration state'],'destination':['held','leased','retrying','quarantined']}

    def test_all_tables_all_backends_all_fields(self):
        attempts={'writes':0,'network':0,'dispatch':0,'forbidden_reads':0}
        for backend in ['sqlite','postgres17','postgres18']:
            before=self.snapshot(backend)
            no_change(before,copy.deepcopy(before),copy.deepcopy(before),backend,attempts)
            for table in before['tables']:
                for change in ['value','type','column','insert','delete']:
                    after=copy.deepcopy(before)
                    if change=='value':after['tables'][table]['rows'][0][1][1]='01'
                    if change=='type':after['tables'][table]['rows'][0][1][0]='text'
                    if change=='column':after['tables'][table]['columns'].append('ignored')
                    if change=='insert':after['tables'][table]['rows'].append([['text','x'],['integer','0']])
                    if change=='delete':del after['tables'][table]
                    with self.subTest(backend=backend,table=table,change=change),self.assertRaises(AssertionError):no_change(before,after,before,backend,attempts)
            for field in ['schema','controls','destination']:
                changed=copy.deepcopy(before);changed[field].append('mutation')
                with self.assertRaises(AssertionError):no_change(before,before,changed,backend,attempts)
            for kind in attempts:
                trace=dict(attempts);trace[kind]=1
                with self.assertRaises(AssertionError):no_change(before,before,before,backend,trace)

    def test_matrix_matches_migration_table_inventory(self):
        import re
        from foundation import R3_SQLITE, R3_POSTGRES_V6
        root=HERE.parents[3]
        for backend,expected in [('sqlite',COMMON|R3_SQLITE|{'billing_setup','billing_entries','billing_aliases','billing_permissions'}),('postgres',COMMON|PG_ONLY|R3_POSTGRES_V6)]:
            text='\n'.join(p.read_text() for p in (root/'crates/ledgerlab/migrations'/backend).glob('*.sql'))
            actual=set(re.findall(r'CREATE TABLE (?:IF NOT EXISTS )?(?:ledgerlab\.)?(\w+)',text))
            self.assertEqual(actual,expected)

if __name__=='__main__':unittest.main()
