"""Sensitivity tests for the independent oracle; no persistence claims."""
import copy
from fractions import Fraction
import json
from pathlib import Path
import tempfile
import unittest
from oracle import (audit, canonical, strict, normalized_quantity, identity,
                    digest, nearest, allocate, content, row_key, verify_zero)

ROOT = Path(__file__).resolve().parents[3]


class OracleTests(unittest.TestCase):
    def test_strict_negative_profile(self):
        for raw in [b'{"x":1,"x":2}', b'{"x":{"a":0,"a":1}}', b'-0', b'1.0',
                    b'1e0', b'9007199254740992', b'"\\ud800"', b'"\\udc00"',
                    b'NaN', b'Infinity', b'\xef\xbb\xbf{}', b'"\xff"']:
            with self.subTest(raw=raw), self.assertRaises((ValueError, UnicodeError)):
                strict(raw)

    def test_utf16_order_and_no_unicode_folding(self):
        self.assertEqual(canonical({'\ue000': 1, '\U0001f600': 2}), '{"😀":2,"":1}'.encode())
        self.assertNotEqual(identity('event', ['é']), identity('event', ['e\u0301']))
        self.assertEqual(strict(b'"\\ud83d\\ude00"'), '😀')

    def test_domain_and_tuple_framing(self):
        self.assertNotEqual(digest('event', ['ab', 'c']), digest('event', ['a', 'bc']))
        self.assertNotEqual(digest('event', ['a']), digest('receipt', ['a']))
        self.assertNotEqual(digest('event', ['demo', 'sandbox']), digest('event', ['sandbox', 'demo']))

    def test_quantity_bounds_and_idempotence(self):
        for token, expected in [('0001.00', '1'), ('0.000', '0'), ('000.00100', '0.001'),
                                ('9' * 30, '9' * 30), ('0.' + '0' * 17 + '1', '0.' + '0' * 17 + '1')]:
            self.assertEqual(normalized_quantity(token), expected)
            self.assertEqual(normalized_quantity(expected), expected)
        for token in ['-0', '+1', '1e0', ' 1', '1.', '9' * 31, '1.' + '0' * 19, '0' * 65]:
            with self.subTest(token=token), self.assertRaises(ValueError):
                normalized_quantity(token)

    def test_round_and_allocation_discontinuities(self):
        self.assertEqual([nearest(Fraction(n, 2)) for n in [-3, -1, 0, 1, 3]], [-2, -1, 0, 1, 2])
        self.assertEqual(allocate(-2, [1, 1, 1]), [-1, -1, 0])
        self.assertEqual(allocate(2, [0, 1, 1]), [0, 1, 1])
        self.assertNotEqual(3 * nearest(Fraction(4, 10)), nearest(Fraction(12, 10)))

    def test_mutated_frozen_fixture_stops(self):
        # Copy only frozen reference files; never modify the source fixture tree.
        files = json.loads((ROOT / 'contracts/freeze.json').read_text())['files']
        with tempfile.TemporaryDirectory(prefix='ledgerlab-oracle-') as temp:
            target = Path(temp)
            for name in [*files, 'contracts/freeze.json']:
                path = target / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes((ROOT / name).read_bytes())
            broken = target / 'fixtures/journals/first-slice/accepted-records.jsonl'
            broken.write_bytes(broken.read_bytes().replace(b'"atoms":"100"', b'"atoms":"101"', 1))
            with self.assertRaisesRegex(ValueError, 'frozen drift'):
                audit(target)

    def test_zero_completion_projection_and_negative_mutations(self):
        # A transient, synthetic harness sample, NOT a new frozen journal and NOT
        # product output. Derive one valid zero completion to exercise validation.
        seed, paid = audit(ROOT)
        scope = ['demo', 'sandbox']
        event = json.loads((ROOT / 'fixtures/canonical/expected/first-slice.json').read_bytes())
        event.update(id='generation-failed', operation_id='generation-failed', status='failed', quantity='0')
        eid = identity('event', [*scope, 'urn:demo:app', 'generation-failed'])
        cid = identity('claim', [scope, 'urn:demo:app', 'generation-failed', 'completion', 'completion'])
        old = {r['kind']: r for r in paid}
        translation = {old['event']['id']: eid, old['claim']['id']: cid,
                       old['receipt']['id']: identity('receipt', [eid]),
                       old['decision-manifest']['id']: identity('decision', [eid]),
                       'generation-1': 'generation-failed'}
        # Operate on strings only in this ASCII synthetic sample.
        sample = json.dumps(paid)
        for source, target in translation.items():
            sample = sample.replace(source, target)
        retained = {'document', 'snapshot-ref', 'event', 'delivery-key', 'claim', 'control-transition', 'chain-revision'}
        rows = [r for r in json.loads(sample) if r['kind'] in retained]
        for row in rows:
            body = row['body']
            if row['kind'] == 'event':
                row['body'] = event
            if row['kind'] == 'delivery-key':
                body.update(ingress=event, ingress_hash='sha256:' + digest('ingress', event))
            if row['kind'] == 'claim':
                facts = {'schema': 'ledger-claim-facts/1', **{k: event[k] for k in ['type', 'chain', 'customer', 'status', 'quantity', 'unit', 'links', 'evidence']}}
                body['facts_hash'] = 'sha256:' + digest('claim-facts', facts)
            if row['kind'] == 'snapshot-ref':
                body['id'] = row['id'] = identity('snapshot-ref', [eid, body['purpose'], body['document_id']])
            row['content_hash'] = 'sha256:' + content(row)
        xp = identity('explanation', [eid, 0])
        explanation = {'kind': 'explanation', 'scope': scope, 'id': xp, 'body': {
            'schema': 'ledger-explanation/1', 'id': xp, 'scope': scope, 'event_id': eid,
            'ordinal': 0, 'outcome': 'skipped', 'code': 'FAILED_WORK',
            'binding_id': 'demo-retail-v1', 'input_refs': [], 'inputs': [], 'action_ids': []}}
        explanation['content_hash'] = 'sha256:' + content(explanation)
        rows.append(explanation)
        manifest = copy.deepcopy(old['decision-manifest'])
        manifest['id'] = manifest['body']['id'] = identity('decision', [eid])
        manifest['body'].update(event_id=eid, explanation_ids=[xp], members=sorted([
            {k: r[k] for k in ['kind', 'id', 'content_hash']} for r in rows + seed
            if r['kind'] not in ['party', 'source-grant-record', 'binding-record']], key=row_key))
        manifest['content_hash'] = 'sha256:' + content(manifest)
        rows.append(manifest)
        receipt = copy.deepcopy(old['receipt'])
        receipt['id'] = receipt['body']['id'] = identity('receipt', [eid])
        receipt['body'].update(event_id=eid, decision_id=manifest['id'], action_ids=[], intention_ids=[],
                               content_hash='sha256:' + digest('event-content', event), decision_hash=manifest['content_hash'])
        receipt['content_hash'] = 'sha256:' + content(receipt)
        rows.append(receipt)
        encode = lambda entries: b''.join(canonical(r) + b'\n' for r in sorted(entries, key=row_key))
        raw = encode(seed + rows)
        self.assertEqual(verify_zero(ROOT, raw), canonical(receipt['body']))
        with self.assertRaises(ValueError):
            verify_zero(ROOT, encode(seed + rows[:-1]))
        mutated = copy.deepcopy(rows)
        item = next(r for r in mutated if r['kind'] == 'event')
        item['body']['quantity'] = '2'
        item['content_hash'] = 'sha256:' + content(item)
        with self.assertRaises(ValueError):
            verify_zero(ROOT, encode(seed + mutated))

    def test_paid_journal_hash_and_full_audit(self):
        seed, accepted = audit(ROOT)
        self.assertEqual((len(seed), len(accepted)), (10, 25))


if __name__ == '__main__':
    unittest.main()
