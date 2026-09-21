"""Numeric analysis and assertion sensitivity, not real-store race evidence."""
import copy
import itertools
import unittest
from lifecycle import (State, HISTORIES, apply, reservation, check_step,
                       assert_complete_or_absent)


class LifecycleOracle(unittest.TestCase):
    def test_hand_authored_histories(self):
        for name, steps in HISTORIES.items():
            s = State()
            for expected in steps:
                with self.subTest(history=name, command=expected[0]):
                    before = copy.deepcopy(s)
                    s, status = apply(s, expected[0])
                    self.assertEqual(status, expected[1])
                    check_step(s, before, expected)
                    if status != 'accepted': self.assertEqual(s, before)
                    if expected[0][0] == 'adjust':
                        self.assertEqual(reservation(s), reservation(before))
                        self.assertEqual(s.reservation_revision, before.reservation_revision)

    def test_shared_invocation_two_serializations(self):
        base, _ = apply(State(), ('ordinary','seed','a',8000,0,True))
        base, _ = apply(base, ('adjust','reverse','a',0,1,True))
        # Only 4000 remains, despite a zero current claim and restored economic ceiling.
        for winner, loser in [('b','c'), ('c','b')]:
            s, status = apply(base, ('ordinary',winner,winner,3000,1,True))
            self.assertEqual(status, 'accepted')
            self.assertEqual(reservation(s), (1000,14000,0,False))
            rejected, status = apply(s, ('ordinary',loser,loser,3000,1,True))
            self.assertEqual(status, 'stale_reservation')
            self.assertEqual(rejected, s)
            rejected, status = apply(s, ('ordinary',loser,loser,3000,2,True))
            self.assertEqual(status, 'capacity')
            self.assertEqual(rejected, s)

    def test_outcome_versus_closure_two_serializations(self):
        s, status = apply(State(), ('ordinary','o','a',2500,0,True))
        self.assertEqual(status, 'accepted')
        rejected, status = apply(s, ('close','c',0,True))
        self.assertEqual(status, 'stale_reservation')
        self.assertEqual(rejected, s)
        s, status = apply(s, ('close','c',1,True))
        self.assertEqual(status, 'accepted')
        self.assertEqual(reservation(s), (0,5500,9500,True))
        s, status = apply(State(), ('close','c',0,True))
        self.assertEqual(status, 'accepted')
        self.assertEqual(reservation(s), (0,3000,12000,True))
        rejected, status = apply(s, ('ordinary','o','a',2500,0,True))
        self.assertEqual(status, 'closed')
        self.assertEqual(rejected, s)

    def test_two_corrections_one_expected_claim_revision(self):
        base, _ = apply(State(), ('ordinary','o','a',2500,0,True))
        base, _ = apply(base, ('close','c',1,True))
        for winner, loser in [(1000,0), (0,1000)]:
            s, status = apply(base, ('adjust','winner','a',winner,1,True))
            self.assertEqual(status, 'accepted')
            rejected, status = apply(s, ('adjust','loser','a',loser,1,True))
            self.assertEqual(status, 'stale_claim')
            self.assertEqual(rejected, s)
            self.assertEqual(reservation(s), (0,5500,9500,True))

    def test_crash_partial_commit_every_changed_field_combination(self):
        before = State()
        after, _ = apply(before, ('ordinary','o','a',2500,0,True))
        # Six changed state groups: held, consumed, revision, claims, identities, postings.
        fields = [k for k in vars(before) if getattr(before,k) != getattr(after,k)]
        self.assertEqual(len(fields), 6)
        probes = 0
        for mask in itertools.product((False,True), repeat=len(fields)):
            s = copy.deepcopy(before)
            for key, take_after in zip(fields,mask):
                if take_after: setattr(s,key,copy.deepcopy(getattr(after,key)))
            if all(mask) or not any(mask):
                assert_complete_or_absent(s,before,after)
            else:
                with self.assertRaisesRegex(AssertionError,'partial commit'):
                    assert_complete_or_absent(s,before,after)
                probes += 1
        self.assertEqual(probes,62)

    def test_unknown_commit_retry_both_durable_outcomes(self):
        command = ('ordinary','o','a',2500,0,True)
        before = State()
        committed, _ = apply(before, command)
        for durable, expected in [(before,'accepted'),(committed,'duplicate')]:
            resolved, status = apply(durable,command)
            self.assertEqual(status,expected)
            self.assertEqual(resolved,committed)
            self.assertEqual(reservation(resolved),(9500,5500,0,False))
        # Absence while a transaction may still be active is intentionally NOT
        # classified here. This model only resolves after known durable outcome.

    def test_closure_retry_never_releases_twice(self):
        command = ('close','c',0,True)
        s, _ = apply(State(),command)
        retried, status = apply(s,command)
        self.assertEqual(status,'duplicate')
        self.assertEqual(retried,s)
        self.assertEqual(reservation(retried),(0,3000,12000,True))

    def test_unauthorized_closure_preserves_hold(self):
        before = State()
        s, status = apply(before,('close','c',0,False))
        self.assertEqual(status,'unauthorized')
        self.assertEqual(s,before)

    def test_new_authority_does_not_expand_existing_agreement(self):
        s, _ = apply(State(),('ordinary','o','a',2500,0,True))
        # An authenticated/authorized correction still obeys the original ceiling.
        after, status = apply(s,('adjust','new-credential','a',10001,1,True))
        self.assertEqual(status,'ceiling')
        self.assertEqual(after,s)
        # A NEW agreement is a distinct scope requiring its own explicit fixture;
        # it cannot be manufactured by raising this invocation's budget.

    def test_bad_replenishment_detected_even_when_total_conserved(self):
        before, _ = apply(State(),('ordinary','o','a',2500,0,True))
        after, _ = apply(before,('adjust','reverse','a',0,1,True))
        bad = copy.deepcopy(after)
        bad.held += 2500
        bad.consumed -= 2500
        bad.check()  # Conservation alone misses forbidden replenishment.
        self.assertNotEqual(reservation(bad),reservation(before))

    def test_bad_release_on_zero_detected_even_when_total_conserved(self):
        after, _ = apply(State(),('ordinary','z','a',0,0,True))
        bad = copy.deepcopy(after)
        bad.released = bad.held
        bad.held = 0
        bad.check()
        self.assertNotEqual(reservation(bad),(12000,3000,0,False))


if __name__ == '__main__':
    unittest.main()
