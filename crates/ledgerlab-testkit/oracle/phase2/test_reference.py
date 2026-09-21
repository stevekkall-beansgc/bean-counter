"""Read-only fixture contract and sensitivity tests; not engine/store conformance."""
from copy import deepcopy
from fractions import Fraction
from itertools import permutations
import json
from pathlib import Path
import sys
import unittest

from jsonschema import Draft202012Validator, ValidationError
from reference import Reference, Refusal, calculate, decimal, round_atoms, LIMIT

ROOT = Path(__file__).resolve().parents[4]
FIX = ROOT / 'crates/ledgerlab-testkit/fixtures/phase2-proposed-v1'
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from oracle import audit, strict  # Independent legacy wire audit, never production.

SCHEMA = strict((FIX / 'history.schema.json').read_bytes())
VALIDATOR = Draft202012Validator(SCHEMA)


def load(name):
    fixture = strict((FIX / (name + '.json')).read_bytes())
    VALIDATOR.validate(fixture)
    return fixture


def verify(fixture):
    VALIDATOR.validate(fixture)
    assert all(k == e['id'] for k, e in fixture['events'].items()), 'event alias mismatch'
    assert all(a['event'] in fixture['events'] for a in fixture['attempts']), 'unknown attempt'
    cfg = fixture['config']
    assert len({g['source'] for g in cfg['grants']}) == len(cfg['grants']), 'ambiguous grant'
    assert len({s['id'] for s in cfg['suppliers']}) == len(cfg['suppliers']), 'duplicate supplier'
    assert len({s['invocation']['id'] for s in cfg['suppliers']}) == len(cfg['suppliers']), 'duplicate invocation'
    actual = calculate(fixture)
    assert actual == fixture['expected'], 'reference and explicit expectation disagree'
    seen = {}
    prior_decisions = set()
    for index, decision in enumerate(actual['journal'], 1):
        assert decision['revision'] == str(index)
        assert decision['event'] == fixture['events'][decision['id']]
        assert set(decision['depends_on']) <= prior_decisions, 'forward economic dependency'
        prior_decisions.add(decision['id'])
        for posting in decision['postings']:
            assert posting['id'] not in seen, 'duplicate posting'
            assert int(posting['atoms']) != 0
            assert set(posting['inputs']) <= seen.keys(), 'forward/missing posting basis'
            if posting['kind'] == 'reversal':
                old = seen[posting['reverses']]
                assert int(posting['atoms']) == -int(old['atoms'])
                assert all(posting[k] == old[k] for k in ('component','book','binding','roles'))
            seen[posting['id']] = posting
        covered = set()
        for obligation in decision['obligations']:
            rows = [seen[p] for p in obligation['posting_ids']]
            assert obligation['book'] != 'cost_observation'
            assert int(obligation['atoms']) == sum(int(p['atoms']) for p in rows)
            assert int(obligation['atoms']) != 0
            assert all(p['roles'] == obligation['roles'] and p['binding'] == obligation['binding'] and
                       p['book'] == obligation['book'] for p in rows)
            assert not covered.intersection(obligation['posting_ids'])
            covered.update(obligation['posting_ids'])
        for xp in decision['explanations']:
            if 'unrounded' in xp:
                n, d = xp['unrounded']['numerator'], xp['unrounded']['denominator']
                ratio = Fraction(int(n), int(d))
                assert str(ratio.numerator) == n and str(ratio.denominator) == d, 'unreduced ratio'
                if xp['code'] != 'SHARE_CEILING':
                    assert round_atoms(ratio) == int(xp['rounded_atoms'])
                else:
                    assert int(xp['rounded_atoms']) < round_atoms(ratio)
    return actual


class Phase2Tests(unittest.TestCase):
    def test_schema_and_all_explicit_histories(self):
        Draft202012Validator.check_schema(SCHEMA)
        paths = sorted(FIX.glob('*.json'))
        self.assertGreaterEqual(len(paths) - 1, 25)
        for path in paths:
            if path.name != 'history.schema.json':
                with self.subTest(fixture=path.name):
                    verify(load(path.stem))

    def test_every_step_preserves_original_journal(self):
        for path in FIX.glob('*.json'):
            if path.name == 'history.schema.json':
                continue
            f = load(path.stem)
            model = Reference(f['config'])
            for step, expected in zip(f['attempts'], f['expected']['results'], strict=True):
                before = deepcopy(model.journal)
                result = model.submit(f['events'][step['event']], step['received'])
                self.assertEqual(result, expected)
                self.assertEqual(model.journal[:len(before)], before)
                self.assertEqual(len(model.journal), len(before) + (result['status'] == 'accepted'))
            self.assertEqual(model.journal, f['expected']['journal'])

    def test_expected_is_not_an_input_and_mutations_fail(self):
        f = load('multi-capped')
        expected = calculate(f)
        f['expected'] = {}
        self.assertEqual(calculate(f), expected)
        for field in ('atoms','roles','inputs'):
            f = load('multi-capped')
            p = f['expected']['journal'][0]['postings'][0]
            p[field] = {'atoms':'81','roles':dict(p['roles'],payer='intruder'),'inputs':['unknown']}[field]
            with self.subTest(field=field), self.assertRaises(AssertionError):
                verify(f)
        f = load('out-of-order')
        f['expected']['results'][0]['status'] = 'accepted'
        with self.assertRaises((AssertionError, ValidationError)):
            verify(f)

    def test_schema_rejects_unknown_null_and_illegal_receipts(self):
        mutations = [lambda f: f['events']['g'].update(tier='standard'),
                     lambda f: f['config'].update(funding=None),
                     lambda f: f['config']['retail']['roles'].update(payer_delegation='unverified'),
                     lambda f: f.update(status='frozen'),
                     lambda f: f['expected']['results'][0].update(status='waiting'),
                     lambda f: f['expected']['journal'][0]['postings'][0].update(atoms='0')]
        for mutate in mutations:
            f = load('generation')
            mutate(f)
            with self.assertRaises(ValidationError):
                VALIDATOR.validate(f)
        for raw in [b'{"x":1,"x":2}',b'{"x":null,"x":2}',b'{"x":1e0}',b'{"x":-0}']:
            with self.assertRaises(ValueError):
                strict(raw)

    def test_permutations_settle_only_explicit_dependencies(self):
        f = load('out-of-order')
        expected = {d['id']: d['postings'] for d in f['expected']['journal']}
        for order in permutations(['g','p','a']):
            model = Reference(f['config'])
            waiting = []
            for event_id in order:
                result = model.submit(f['events'][event_id], '50')
                if result['status'] == 'waiting':
                    waiting.append(event_id)
                else:
                    self.assertEqual(result['status'], 'accepted')
            for _ in range(3):
                for event_id in waiting[:]:
                    if model.submit(f['events'][event_id], '50')['status'] == 'accepted':
                        waiting.remove(event_id)
            self.assertEqual(waiting, [])
            self.assertEqual({d['id']:d['postings'] for d in model.journal}, expected)
            self.assertEqual(model.state()['totals']['retail'], '650')

    def test_waiting_does_not_reserve_identity_or_claim(self):
        f = load('out-of-order'); model = Reference(f['config'])
        candidate = deepcopy(f['events']['p'])
        self.assertEqual(model.submit(candidate,'50')['status'], 'waiting')
        self.assertEqual((model.claims,model.deliveries,model.journal), ({},{},[]))
        model.submit(f['events']['g'],'50')
        candidate['quantity']='2'
        result = model.submit(candidate,'50')
        self.assertEqual(result['status'],'accepted')
        self.assertEqual(model.journal[-1]['postings'][0]['atoms'],'100')

    def test_retries_ignore_changed_price_versions_but_keep_source_rights(self):
        f = load('generation'); model = Reference(f['config']); event = f['events']['g']
        model.submit(event,'50'); original = deepcopy(model.journal)
        model.config['policy_version']='unusable/99'
        self.assertEqual(model.submit(event,'90000')['code'],'IDENTITY_DUPLICATE')
        alias = dict(event,id='alias')
        self.assertEqual(model.submit(alias,'90000')['code'],'SEMANTIC_DUPLICATE')
        self.assertEqual(model.journal,original)
        self.assertEqual(model.submit(dict(event,quantity='2'),'50')['code'],'IDENTITY_CONFLICT')
        self.assertEqual(model.submit(dict(alias,id='conflict',quantity='2'),'50')['code'],'SEMANTIC_CONFLICT')
        model.config['grants'][0]['active']=False
        self.assertEqual(model.submit(event,'50')['code'],'SOURCE_UNAUTHORIZED')

    def test_exact_rounding_and_boundaries(self):
        for n in range(-101,102):
            for d in [1,2,3,10,100]:
                x=Fraction(n,d)
                result=round_atoms(x)
                self.assertEqual(result,-round_atoms(-x))
                self.assertLessEqual(abs(x-result),Fraction(1,2))
                if abs(x-result)==Fraction(1,2):
                    self.assertGreater(abs(result),abs(x))
        self.assertEqual(round_atoms(Fraction(LIMIT)),LIMIT)
        for value in [Fraction(LIMIT+1),Fraction(-(LIMIT+1)),Fraction(2**513,3)]:
            with self.assertRaises(Refusal): round_atoms(value)
        for token in ['-0','1e0','1.0000000000000000000','9'*31]:
            with self.assertRaises(Refusal): decimal(token)
        self.assertEqual(decimal('0001.50'),Fraction(3,2))

    def test_book_total_overflow_discards_entire_second_decision(self):
        f=load('tier-standard'); model=Reference(f['config'])
        model.config['scale']=0
        for rate in model.config['rates']:
            if rate['kind']=='content.generated': rate['unit_price']=str(LIMIT)
        self.assertEqual(model.submit(f['events']['g'],'50')['status'],'accepted')
        before=deepcopy(model.__dict__)
        later=dict(f['events']['g'],id='g2',operation='g2')
        self.assertEqual(model.submit(later,'50')['code'],'ARITHMETIC_OVERFLOW')
        self.assertEqual(model.__dict__,before)

    def test_quality_reads_booked_net_and_is_one_adjustment(self):
        f=load('generation'); c=f['config']; c.update(quality_enabled=True,quality_percent='50')
        model=Reference(c); model.submit(f['events']['g'],'50')
        q=load('later-quality')['events']['q']
        self.assertEqual(model.submit(q,'50')['status'],'accepted')
        self.assertEqual(model.journal[-1]['postings'][0]['atoms'],'-40')  # net 80, not base 100
        new=dict(q,id='q2',operation='q2',claim='second-quality')
        self.assertEqual(model.submit(new,'50')['code'],'QUALITY_ALREADY_ADJUSTED')

    def test_reversal_uses_stored_atoms_and_each_supplier_authority(self):
        f=load('multi-capped'); model=Reference(f['config'])
        for id in ['g','o','p','a']: self.assertEqual(model.submit(f['events'][id],'50')['status'],'accepted')
        old=deepcopy(model.journal); consumed=deepcopy(model.consumed)
        model.config['premium_atoms']='9999'
        model.config['suppliers'][0]['correction_sources']=[]
        self.assertEqual(model.submit(f['events']['r'],'90')['code'],'CORRECTION_UNAUTHORIZED')
        self.assertEqual(model.journal,old)
        model.config['suppliers'][0]['correction_sources']=['urn:host:app']
        self.assertEqual(model.submit(f['events']['r'],'90')['status'],'accepted')
        self.assertEqual(model.consumed,consumed)
        self.assertTrue(model.closed)
        self.assertEqual(set(model.state()['totals'].values()),{'0'})
        second=dict(f['events']['r'],id='r2',operation='r2')
        self.assertEqual(model.submit(second,'90')['code'],'ALREADY_REVERSED')

    def test_share_rounds_before_ceiling_and_binding_order_is_stable(self):
        f=load('multi-uncapped')
        for ceiling,expected,reason in [('50','1','SHARE_APPLIED'),('0','0','SHARE_CEILING')]:
            c=deepcopy(f['config']); c['premium_atoms']='2'
            c['suppliers'][1]['share_ceiling_atoms']=ceiling
            c['suppliers'].reverse()
            model=Reference(c)
            for id in ['g','o','p','a']:
                self.assertEqual(model.submit(f['events'][id],'50')['status'],'accepted')
            xp=next(x for x in model.journal[-1]['explanations'] if x['component']=='publisher.share')
            self.assertEqual(xp['unrounded'],{'numerator':'1','denominator':'2'})
            self.assertEqual((xp['rounded_atoms'],xp['code']),(expected,reason))
            shares=[p for p in model.journal[-1]['postings'] if p['kind']=='share']
            self.assertEqual(len(shares),int(expected != '0'))

    def test_frozen_legacy_compatibility(self):
        seed,accepted=audit(ROOT)  # all 99 frozen bytes, original IDs/receipt, 60 vectors
        self.assertEqual((len(seed),len(accepted)),(10,25))
        first=load('generation')['expected']['journal'][0]
        frozen=[r['body'] for r in accepted if r['kind']=='action']
        self.assertEqual(sorted((p['component'],p['atoms']) for p in first['postings']),
                         sorted((p['component'],p['amount']['atoms']) for p in frozen))
        self.assertEqual(first['obligations'][0]['atoms'],'80')
        for old,new in [('capped','multi-capped'),('third-party','multi-uncapped')]:
            frozen=strict((ROOT / 'fixtures/journals' / old / 'economics.json').read_bytes())
            history=load(new)['expected']['journal'][:4]
            self.assertEqual(sorted((p['book'],p['component'],p['atoms']) for d in history for p in d['postings']),
                             sorted((p['book'],p['component'],p['atoms']) for p in frozen['postings']))
        math={v['name']:v['expected'] for v in strict((ROOT / 'fixtures/math/vectors.json').read_bytes())['vectors']}
        self.assertEqual([round_atoms(Fraction(x)*100) for x in ['1.005','-1.005']],math['signed_ties'])
        self.assertEqual([round_atoms(Fraction(x)*100) for x in ['1.0049','-1.0049']],math['signed_non_ties'])
        self.assertEqual(round_atoms(decimal('0.07')*decimal('1.5')*100),math['unit_fraction'])
        self.assertEqual([3*round_atoms(Fraction(2,5)),round_atoms(Fraction(6,5))],math['rounding_stage'])


if __name__ == '__main__':
    unittest.main(verbosity=2)
