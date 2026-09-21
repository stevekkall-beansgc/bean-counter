"""Independent outcome expectations and black-box adapter runner (test-only API).

No production imports. Frozen reconstruction never receives adapter output.
Adapters must read real storage; the sensitivity double is not store evidence.
"""
import copy
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
sys.path.insert(0, str(ROOT / 'scripts/contract_checks/v2_candidate'))
from profile import canonical, row_order, strict, reference
from reconstruct import files
from functools import lru_cache

reconstructed = lru_cache(maxsize=1)(files)
from audit import verify

# Variants of the same base -> outcome -> correction path. No new wire format.
HISTORIES = ('correction-replacement', 'correction-reinstatement',
             'zero-adjustment', 'full-reversal-reinstatement', 'supplier-separation')


def require(ok, message):
    if not ok:
        raise AssertionError(message)


class Oracle:
    def __init__(self, name):
        require(name in HISTORIES, 'outside bounded outcome lane')
        generated = reconstructed()[name + '.json']
        require(generated == (ROOT / 'contracts/candidates/v2/goldens' / (name + '.json')).read_bytes(),
                'frozen reconstruction differs')
        self.history = strict(generated)
        self.anchor = reference(next(r for r in self.history['seed'] if r['kind'] == 'base-acceptance'))
        verify(self.history, self.anchor)
        self.decisions = self.history['decisions']

    def rows(self, count):
        return sorted(self.history['seed'] + [r for d in self.decisions[:count] for r in d['records']],
                      key=row_order)

    def journal(self, count):
        return [canonical(r).decode() for r in self.rows(count)]

    def command(self, index):
        return copy.deepcopy(next(r['body'] for r in self.decisions[index]['records'] if r['kind'] == 'event'))

    def receipt(self, index):
        return self.decisions[index]['receipt_utf8']

    def heads(self, count):
        heads = {}
        for d in self.decisions[:count]:
            r = next(r for r in d['records'] if r['kind'] == 'claim-revision')
            heads[r['body']['claim_id']] = r['id']
        return heads

    def totals(self, count):
        totals = {r['body']['binding_id']: {'premium': '0', 'discount': '0'}
                  for r in self.history['seed'] if r['kind'] == 'binding-snapshot'}
        current = {}
        for d in self.decisions[:count]:
            r = next(r for r in d['records'] if r['kind'] == 'claim-revision')
            current[r['body']['claim_id']] = r['body']
        for binding in totals:
            amounts = [int(r['amount']['atoms']) for r in current.values() if r['binding_id'] == binding]
            totals[binding] = {'premium': str(sum(max(n, 0) for n in amounts)),
                               'discount': str(sum(max(-n, 0) for n in amounts))}
        return totals

    def check(self, snapshot, count):
        require(snapshot['journal_utf8'] == self.journal(count), 'original stored journal bytes differ')
        require(snapshot['base_anchor'] == self.anchor, 'committed base anchor changed')
        require(snapshot['receipts_utf8'] == [self.receipt(i) for i in range(count)], 'original receipts differ')
        require(snapshot['claim_heads'] == self.heads(count), 'current claim head differs')
        require(snapshot['binding_totals'] == self.totals(count), 'binding capacity aggregate differs')
        require(snapshot['rows'], 'missing full physical inventory')
        require(all(isinstance(v, dict) for v in snapshot['rows'].values()), 'invalid physical inventory')


def preserved(before, after):
    """Exact immutable-row preservation; mutable state checked by its projections."""
    old = {canonical([r['kind'], r['id']]): r for r in map(strict, (s.encode() for s in before['journal_utf8']))}
    new = {canonical([r['kind'], r['id']]): r for r in map(strict, (s.encode() for s in after['journal_utf8']))}
    require(all(new.get(k) == v for k, v in old.items()), 'permanent record deleted or rewritten')


def reopened(adapter, oracle, count):
    before = adapter.observe()
    adapter.reopen()  # Close every connection/owner, reopen the SAME durable database.
    after = adapter.observe()
    require(before == after, 'reopen changed complete state')
    oracle.check(after, count)
    return after


def expect_reply(reply, status, receipt=None):
    require(reply == {'status': status, 'receipt_utf8': receipt}, 'wrong status/original receipt')


def run_history(factory, name):
    """Factory creates fresh real store, accepts base via coordinator, returns adapter.

    No expected journals/actions are supplied to factory or acceptance methods.
    Fixture name selects synthetic original inputs; it is not permission to seed
    accepted rows. Returned count is executed assertions/scenarios, not SQL writes.
    """
    require(name != 'supplier-separation', 'BLOCKED: no frozen outcome reservation transition')
    oracle = Oracle(name)
    a = factory(name)
    count = 0
    try:
        evidence = a.evidence()
        require(evidence['backend'] in ('sqlite-file', 'postgres17', 'postgres18'), 'real backend required')
        require(evidence['location'] and evidence['version'], 'missing engine evidence')
        before = reopened(a, oracle, 0)
        for i in range(len(oracle.decisions)):
            expect_reply(a.accept(oracle.command(i)), 'accepted', oracle.receipt(i))
            after = reopened(a, oracle, i + 1)
            preserved(before, after)
            # Retry every historical event after EVERY later correction, including zero/reversal.
            for old in range(i + 1):
                expect_reply(a.accept(oracle.command(old)), 'duplicate_identity', oracle.receipt(old))
                require(a.observe() == after, 'identity retry mutated physical state')
                count += 1
            before = after
            count += 1
        # Ordinary alias must return first receipt, even after later correction.
        command = oracle.command(0)
        command['data']['external_id'] += '-alias'
        expect_reply(a.accept(command), 'duplicate_claim', oracle.receipt(0))
        aliased = reopened(a, oracle, len(oracle.decisions))
        require(aliased['aliases'] == before['aliases'] + [
            {'command_utf8': canonical(command).decode(), 'receipt_utf8': oracle.receipt(0)}], 'alias mapping differs')
        require(aliased['claim_heads'] == before['claim_heads'], 'alias advanced claim')
        # Same alias is an original identity lookup now and must not create another alias.
        expect_reply(a.accept(command), 'duplicate_identity', oracle.receipt(0))
        require(a.observe() == aliased, 'alias retry mutated physical state')
        count += 2
        # Both guards: new delivery label means identity retry cannot mask stale revision.
        correction = next(i for i in range(len(oracle.decisions)) if oracle.command(i)['data']['type'] == 'correction')
        for guard in ('expected_revision', 'expected_revision_number'):
            stale = oracle.command(correction)
            stale['data']['external_id'] += '-stale-' + guard
            # One stale component, one current component; both must be checked.
            current = next(r for r in oracle.rows(len(oracle.decisions))
                           if r['kind'] == 'claim-revision' and r['id'] ==
                           oracle.heads(len(oracle.decisions))[stale['data']['claim_id']])
            other = 'expected_revision_number' if guard == 'expected_revision' else 'expected_revision'
            stale['data'][other] = current['body']['number'] if other.endswith('number') else current['id']
            expect_reply(a.accept(stale), 'guard_rejected')
            require(a.observe() == aliased, 'stale correction changed physical state')
            count += 1
        return count
    finally:
        a.close()


def run_boundaries(factory, name, index):
    """Adapter inventories actual per-item writes/awaits; runner injects every edge.

    Factory.with_prefix accepts base and preceding commands normally. Schedule
    entries are (site, item, edge, phase); phase is precommit or commit. Adapter
    must prove catalogue completeness against coordinator instrumentation.
    """
    require(name != 'supplier-separation', 'BLOCKED: no frozen outcome reservation transition')
    oracle = Oracle(name)
    probe = factory.with_prefix(name, index)
    try:
        schedule = probe.boundaries()
    finally:
        probe.close()
    require(schedule and len({tuple(p) for p in schedule}) == len(schedule), 'empty/duplicate boundary catalogue')
    require(all(len(p) == 4 and p[2] in ('before', 'after') and p[3] in ('precommit', 'commit') for p in schedule), 'invalid boundary')
    for point in schedule:
        for mode in ('error', 'cancel'):
            a = factory.with_prefix(name, index)
            try:
                before = reopened(a, oracle, index)
                reply, hit = a.inject(oracle.command(index), point, mode)
                require(hit == point, 'requested boundary was not reached')
                require(reply['status'] in ('rolled_back', 'cancelled', 'unknown', 'accepted'), 'failure classified as rejection')
                a.drain()
                a.reopen()
                after = a.observe()
                if point[3] == 'precommit' or reply['status'] == 'rolled_back':
                    require(after == before, 'partial precommit write residue')
                elif after != before:
                    oracle.check(after, index + 1)
                if reply['status'] == 'accepted':
                    oracle.check(after, index + 1)
                    require(reply['receipt_utf8'] == oracle.receipt(index), 'accepted without original receipt')
                require(a.pool_clean(), 'transaction leaked to next borrower')
                resolved = a.resolve_and_retry(oracle.command(index))
                require(resolved['status'] in ('accepted', 'duplicate_identity') and resolved['receipt_utf8'] == oracle.receipt(index), 'unknown not resolved by original identity')
                reopened(a, oracle, index + 1)
            finally:
                a.close()
    return 2 * len(schedule)
