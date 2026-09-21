"""Assertion sensitivity only: these snapshots are NOT real-store evidence."""
import copy
import unittest
from conformance import HISTORIES, Oracle, preserved, expect_reply, require


class OutcomeAssertions(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.oracles = {name: Oracle(name) for name in HISTORIES}

    def snapshot(self, name='correction-replacement', count=2):
        o = self.oracles[name]
        return {'journal_utf8': o.journal(count), 'base_anchor': o.anchor,
                'receipts_utf8': [o.receipt(i) for i in range(count)],
                'claim_heads': o.heads(count), 'binding_totals': o.totals(count),
                'aliases': [], 'rows': {'assertion_example_only': {'key': 'value'}}}

    def test_five_frozen_histories_every_prefix(self):
        for name, o in self.oracles.items():
            for i in range(len(o.decisions) + 1):
                o.check(self.snapshot(name, i), i)

    def test_exact_inverse_replacement_math(self):
        o = self.oracles['correction-replacement']
        deltas = [[int(r['body']['amount']['atoms']) for r in d['records'] if r['kind'] == 'action'] for d in o.decisions]
        self.assertEqual([sorted(d) for d in deltas], [[2500], [-2500, 1000], [-1000, 1000], [-1000, 2500]])
        self.assertEqual(sum(sum(d) for d in deltas), 2500)

    def test_zero_net_correction_retains_actions_without_intention(self):
        rows = self.oracles['correction-replacement'].decisions[2]['records']
        self.assertEqual(sum(r['kind'] == 'action' for r in rows), 2)
        self.assertFalse(any(r['kind'] == 'intention' for r in rows))

    def test_zero_acceptance_has_permanent_claim_and_receipt(self):
        o = self.oracles['zero-adjustment']
        rows = o.decisions[0]['records']
        self.assertEqual(sum(r['kind'] == 'claim' for r in rows), 1)
        self.assertFalse(any(r['kind'] in ('action', 'effect', 'intention') for r in rows))
        self.assertTrue(o.receipt(0))

    def test_supplier_uses_retail_percentage_not_supplier_capacity(self):
        o = self.oracles['supplier-separation']
        # Hand math: 10000 retail * -10/100 = -1000, not 3000 * -10/100.
        self.assertEqual(o.totals(2)['binding-supplier']['discount'], '1000')
        self.assertEqual(o.totals(3)['binding-supplier']['discount'], '1000')
        self.assertEqual(o.totals(4)['binding-supplier']['discount'], '0')

    def test_reversal_does_not_release_claim(self):
        o = self.oracles['full-reversal-reinstatement']
        self.assertEqual(set(o.heads(1)), set(o.heads(3)))
        self.assertEqual(sum(r['kind'] == 'claim' for r in o.rows(3)), 1)
        self.assertEqual(o.totals(2)['binding-retail'], {'premium': '0', 'discount': '0'})

    def test_partial_rows_rejected_at_every_prefix(self):
        for name, o in self.oracles.items():
            for i in range(1, len(o.decisions) + 1):
                complete = self.snapshot(name, i)
                for row in range(len(complete['journal_utf8'])):
                    partial = copy.deepcopy(complete)
                    partial['journal_utf8'].pop(row)
                    with self.assertRaisesRegex(AssertionError, 'journal'):
                        o.check(partial, i)

    def test_duplicate_economics_rejected(self):
        s = self.snapshot(); s['journal_utf8'].append(s['journal_utf8'][0])
        with self.assertRaises(AssertionError): self.oracles['correction-replacement'].check(s, 2)

    def test_changed_receipt_byte_rejected(self):
        s = self.snapshot(); s['receipts_utf8'][0] += '\n'
        with self.assertRaisesRegex(AssertionError, 'receipt'): self.oracles['correction-replacement'].check(s, 2)

    def test_newest_receipt_cannot_replace_original(self):
        s = self.snapshot(); s['receipts_utf8'][0] = s['receipts_utf8'][1]
        with self.assertRaisesRegex(AssertionError, 'receipt'): self.oracles['correction-replacement'].check(s, 2)

    def test_anchor_substitution_rejected(self):
        s = copy.deepcopy(self.snapshot()); s['base_anchor']['content_hash'] = 'sha256:' + '0' * 64
        with self.assertRaisesRegex(AssertionError, 'anchor'): self.oracles['correction-replacement'].check(s, 2)

    def test_stale_head_rejected(self):
        s = self.snapshot(); s['claim_heads'] = self.oracles['correction-replacement'].heads(1)
        with self.assertRaisesRegex(AssertionError, 'head'): self.oracles['correction-replacement'].check(s, 2)

    def test_capacity_replenishment_rejected(self):
        s = self.snapshot(); s['binding_totals']['binding-retail']['premium'] = '0'
        with self.assertRaisesRegex(AssertionError, 'capacity'): self.oracles['correction-replacement'].check(s, 2)

    def test_missing_inventory_rejected(self):
        s = self.snapshot(); s['rows'] = {}
        with self.assertRaisesRegex(AssertionError, 'inventory'): self.oracles['correction-replacement'].check(s, 2)

    def test_permanent_record_removal_rejected(self):
        before = self.snapshot(count=1); after = self.snapshot(count=2)
        preserved(before, after)
        after['journal_utf8'].remove(before['journal_utf8'][0])
        with self.assertRaisesRegex(AssertionError, 'permanent'): preserved(before, after)

    def test_duplicate_receipt_sensitive(self):
        with self.assertRaises(AssertionError):
            expect_reply({'status': 'duplicate_identity', 'receipt_utf8': 'new'}, 'duplicate_identity', 'original')

    def test_checks_survive_python_optimization(self):
        with self.assertRaises(AssertionError): require(False, 'required')


if __name__ == '__main__':
    unittest.main()
