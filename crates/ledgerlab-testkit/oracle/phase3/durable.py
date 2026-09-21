"""Audit actual reopened records; expectations never import production output.

The fixture is one synthetic held-base/ordinary/correction/closure lifecycle.
The existing independent economic audit checks original inputs and all economic
equations. The pre-encoding numeric oracle supplies reservation expectations.
This is not real-world authority verification or a general workload benchmark.
"""
import copy
import json
import sys
from pathlib import Path
import jsonschema

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'scripts/contract_checks/v2_candidate'))
from profile import canonical, reference, row_order, strict
from audit import verify as economic_verify
sys.path.insert(0, str(ROOT / 'scripts/reservation_settlement'))
from codec import row as settlement_row, digest as settlement_digest
from lifecycle import State, apply

KINDS = {'reservation-observation', 'reservation-transition', 'reservation-receipt'}
SETTLEMENT = jsonschema.Draft202012Validator(json.loads(
    (ROOT / 'contracts/candidates/reservation-settlement-v1/records.schema.json').read_text()))


def identity(r):
    return canonical([r['scope'], r['kind'], r['id']])


def check(evidence):
    assert evidence['backend'] in ('sqlite', 'postgres17', 'postgres18')
    prefixes = evidence['prefixes']
    assert len(prefixes) == 4
    numeric = State()
    operations = [None, ('ordinary', 'o1', 'supplier', 2500, 0, True),
                  ('adjust', 'fix', 'supplier', 0, 1, True), ('close', 'c1', 1, True)]
    history = {'status': 'candidate-not-frozen', 'target_admission': 'accepted',
               'name': strict(prefixes[1]['deliveries'][1]['economic_utf8'].encode())['body']['chain_id'],
               'seed': [], 'decisions': [], 'probes': []}
    retained = {}
    previous_result = None
    previous_deliveries = []
    anchor = None
    count = 0
    for index, prefix in enumerate(prefixes):
        raw = prefix['records_utf8']
        assert raw == sorted(set(raw)), 'duplicate or unordered observer bytes'
        records = [strict(s.encode()) for s in raw]
        assert all(canonical(r).decode() == s for r, s in zip(records, raw)), 'noncanonical storage'
        current = {identity(r): r for r in records}
        assert len(current) == len(records), 'identity conflict'
        assert all(current.get(k) == r for k, r in retained.items()), 'rewritten/deleted record'
        new = [r for r in records if identity(r) not in retained]
        economics = sorted((r for r in new if r['kind'] not in KINDS), key=row_order)
        settlements = [r for r in new if r['kind'] in KINDS]
        delivery = prefix['deliveries'][-1]
        assert len(prefix['deliveries']) == index + 1
        assert prefix['deliveries'][:-1] == previous_deliveries, 'original receipt pair changed'
        for d in prefix['deliveries']:
            assert (d['source'], d['external_id']) == (d['canonical_source'], d['canonical_external_id'])
            for field in ('economic_utf8', 'settlement_utf8'):
                if d[field] is not None:
                    r = strict(d[field].encode())
                    assert current[identity(r)] == r
        if index == 0:
            history['seed'] = economics
            base = next(r for r in economics if r['kind'] == 'base-acceptance')
            anchor = reference(base)
        elif index < 3:
            # The older economic audit accepts the receipt body, whereas the
            # composite store retains the entire envelope (checked above).
            receipt_body = strict(delivery['economic_utf8'].encode())['body']
            history['decisions'].append({'records': economics, 'receipt_utf8': canonical(receipt_body).decode()})
        else:
            assert economics == [] and delivery['economic_utf8'] is None, 'closure fabricated economics'
        economic_verify(history, anchor)
        assert len(settlements) == (3 if index in (1, 3) else 2)
        for r in settlements:
            SETTLEMENT.validate(r)
            assert r == settlement_row(r['kind'], r['scope'], r['body']), 'settlement ID/hash'
        observation = next(r for r in settlements if r['kind'] == 'reservation-observation')
        receipt = next(r for r in settlements if r['kind'] == 'reservation-receipt')
        o, rb = observation['body'], receipt['body']
        assert canonical(receipt).decode() == delivery['settlement_utf8']
        command = strict(delivery['command_utf8'].encode())
        assert command == o['command']
        assert command['kind'] == ['register', 'ordinary', 'post_hoc', 'close'][index]
        assert o['request_hash'] == settlement_digest('request', command) == rb['request_hash']
        assert rb['observation'] == reference(observation)
        assert all(rb[k] == command[k] for k in ('source', 'external_id', 'invocation_id'))
        assert o['anchor']['base_acceptance'] == anchor
        assert o['received_at'] <= o['accepted_at']
        permission = ['submit', 'submit', 'correct', 'close'][index]
        assert o['authority']['active'] and int(o['authority']['grant_revision']) > 0
        assert {'read', permission} <= set(o['authority']['permissions'])
        if delivery['economic_utf8'] is not None:
            economic_ref = reference(strict(delivery['economic_utf8'].encode()))
            assert o['economic_receipt'] == rb['economic_receipt'] == economic_ref
        else:
            assert 'economic_receipt' not in o and 'economic_receipt' not in rb
        if index:
            assert o['before'] == previous_result, 'reservation predecessor changed'
            assert o['previous'] == reference(strict(previous_deliveries[-1]['settlement_utf8'].encode()))
            assert o['registration'] == reference(strict(previous_deliveries[0]['settlement_utf8'].encode()))
            numeric, status = apply(numeric, operations[index])
            assert status == 'accepted'
        else:
            assert o['before'] == rb['result']
        result = rb['result']
        assert [int(result[k]) for k in ('held', 'consumed', 'released', 'maximum', 'revision')] == [
            numeric.held, numeric.consumed, numeric.released, 15000, numeric.reservation_revision]
        assert result['families'] == sorted(result['families'], key=canonical)
        assert len(result['families']) == 1
        family = result['families'][0]
        supplier = next(r['body'] for r in history['seed']
                        if r['kind'] == 'policy-snapshot' and r['body']['binding_id'] == 'binding-supplier')
        assert family['key'] == {'agreement_id': supplier['agreement_id'],
                                 'family_id': supplier['family_id'], 'target': base['body']['target']}
        assert family['accepted_by'] == supplier['ordinary']['accepted_by']
        assert family['status'] == ('open' if index == 0 else 'claimed')
        if index:
            assert family['ordinary_receipt'] == reference(strict(prefix['deliveries'][1]['economic_utf8'].encode()))
        if index == 2:
            assert result == previous_result, 'post-hoc changed reservation'
        changed = result != o['before']
        transitions = [r for r in settlements if r['kind'] == 'reservation-transition']
        assert len(transitions) == int(changed)
        if changed:
            t = transitions[0]
            assert rb['transition'] == reference(t)
            assert t['body']['observation'] == reference(observation)
            assert t['body']['before'] == o['before'] and t['body']['after'] == result
            assert int(t['body']['consume']) == (2500 if index == 1 else 0)
            assert int(t['body']['release']) == (9500 if index == 3 else 0)
        else:
            assert 'transition' not in rb
        # The full persisted economic/settlement prefix is the replay closure.
        expected_refs = sorted((reference(r) for r in records if r != receipt), key=canonical)
        assert rb['replay'] == expected_refs, 'incomplete replay prefix'
        heads = {h['class']: h for h in prefix['heads'] if h['class'] in ('Reservation', 'Target')}
        assert strict(heads['Reservation']['value_utf8'].encode()) == result
        assert heads['Reservation']['revision'] == result['revision']
        assert heads['Target']['revision'] == str(index)
        registration = reference(strict(prefix['deliveries'][0]['settlement_utf8'].encode()))
        target = strict(heads['Target']['value_utf8'].encode())
        assert target == {'base': anchor, 'registration': registration,
                          'records': sorted(map(reference, records), key=canonical)}
        expected_anchors = [{'scope': base['scope'], 'kind': r['kind'],
                             'id_utf8': canonical(r['id']).decode(), 'content_hash': r['content_hash']}
                            for r in (anchor, registration)]
        assert sorted(prefix['anchors'], key=canonical) == sorted(expected_anchors, key=canonical)
        latest = (next(r for r in history['decisions'][-1]['records'] if r['kind'] == 'claim-revision')
                  if history['decisions'] else None)
        for head in prefix['heads']:
            key = strict(head['key_utf8'].encode())
            value = None if head['value_utf8'] is None else strict(head['value_utf8'].encode())
            if head['class'] == 'Claim':
                if latest and key[1] == supplier['agreement_id']:
                    assert head['revision'] == latest['body']['number']
                    assert value == {'revision': reference(latest),
                                     'original_receipt': family['ordinary_receipt']}
                else:
                    assert value is None and head['revision'] is None
            elif head['class'] == 'BindingAggregate':
                expected = [reference(latest)] if latest and key[-1] == supplier['binding_id'] else []
                assert value == {'revisions': expected}
                assert head['revision'] == (str(len(history['decisions'])) if expected else '0')
            elif head['class'] == 'InvocationConsumption':
                assert value == {'target': base['body']['target'], 'registration': registration}
                assert head['revision'] == '0'
            elif head['class'] == 'BaseReversal':
                assert value == {'reversed': False} and head['revision'] == '0'
        assert prefix['anchors'] == prefixes[0]['anchors'] and prefix['anchors']
        assert prefix['physical'] and len({t for t, _ in prefix['physical']}) == len(prefix['physical'])
        retained, previous_result = current, result
        previous_deliveries = prefix['deliveries']
        count += len(new)
    return count


def sensitivity(evidence):
    probes = []
    for i in range(4):
        changed = copy.deepcopy(evidence)
        changed['prefixes'][i]['records_utf8'].pop()
        probes.append(changed)
    changed = copy.deepcopy(evidence)
    changed['prefixes'][2]['deliveries'][0]['settlement_utf8'] = changed['prefixes'][1]['deliveries'][1]['settlement_utf8']
    probes.append(changed)
    changed = copy.deepcopy(evidence)
    changed['prefixes'][1]['anchors'].pop()
    probes.append(changed)
    changed = copy.deepcopy(evidence)
    next(h for h in changed['prefixes'][2]['heads']
         if h['class'] == 'Claim' and h['revision'] is not None)['revision'] = '1'
    probes.append(changed)
    changed = copy.deepcopy(evidence)
    next(h for h in changed['prefixes'][2]['heads'] if h['class'] == 'Reservation')['revision'] = '2'
    probes.append(changed)
    for changed in probes:
        try:
            check(changed)
        except (AssertionError, KeyError, StopIteration):
            continue
        raise AssertionError('negative sensitivity probe accepted')
    return len(probes)


def main():
    paths = sys.argv[1:]
    compare = paths[0] == '--compare'
    if compare:
        paths = paths[1:]
    values = [json.loads(Path(p).read_text()) for p in paths]
    for value in values:
        print(json.dumps({'backend': value['backend'], 'prefixes': 4,
                          'records': check(value), 'negative_probes': sensitivity(value)}))
    if compare:
        assert {v['backend'] for v in values} == {'sqlite', 'postgres17', 'postgres18'}
        common = lambda v: [{k: x for k, x in p.items() if k != 'physical'} for p in v['prefixes']]
        assert all(common(v) == common(values[0]) for v in values[1:]), 'durable cross-store mismatch'
        print('PASS: exact reopened records, original receipts, anchors and heads agree across SQLite/PG17/PG18')


if __name__ == '__main__':
    main()
