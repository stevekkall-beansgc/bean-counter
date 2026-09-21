"""Owner-approved numeric reservation oracle. TEST OBSERVATIONS ONLY.

No production imports, record serialization, IDs, hashing or schema. Integer
atoms are synthetic. This models one invocation after its original base consumed
3000 of 15000 authorized atoms. Held capacity is 12000. Positive ordinary
premiums consume held; a zero/negative outcome never returns capacity.
Post-hoc economics may change within pinned ceilings, but reservation cannot.
"""
from dataclasses import dataclass, field
from copy import deepcopy


@dataclass
class State:
    held: int = 12000
    consumed: int = 3000
    released: int = 0
    closed: bool = False
    reservation_revision: int = 0
    claims: dict = field(default_factory=dict)
    identities: dict = field(default_factory=dict)
    postings: list = field(default_factory=list)

    def observe(self):
        return (self.held, self.consumed, self.released, self.closed,
                self.reservation_revision,
                tuple(sorted((k, *v) for k, v in self.claims.items())),
                tuple(self.postings))

    def check(self):
        assert min(self.held, self.consumed, self.released) >= 0
        assert self.held + self.consumed + self.released == 15000
        assert not self.closed or self.held == 0


def apply(before, command):
    """Returns (new state, test category); invalid/retry never changes state.

    Tuple commands deliberately omit every candidate encoding choice. Revision
    numbers here count numeric test transitions, not frozen record identities.
    Ordinary uses expected reservation revision; adjustment uses claim revision.
    Claim amounts across this one binding obey gross premium <=10000 and gross
    discount <=3000. New authority cannot silently raise these original limits.
    """
    s = deepcopy(before)
    kind, identity, *args = command
    if identity in s.identities:
        return s, 'duplicate' if s.identities[identity] == command else 'identity_conflict'
    if kind == 'ordinary':
        family, amount, expected, authorized = args
        if not authorized: return s, 'unauthorized'
        if family in s.claims: return s, 'claim_conflict'
        if s.closed: return s, 'closed'
        if expected != s.reservation_revision: return s, 'stale_reservation'
        claim_revision = 1
    elif kind == 'adjust':
        family, amount, expected, authorized = args
        if not authorized: return s, 'unauthorized'
        if family not in s.claims: return s, 'missing_claim'
        if expected != s.claims[family][0]: return s, 'stale_claim'
        claim_revision = expected + 1
    elif kind == 'close':
        expected, authorized = args
        if not authorized: return s, 'unauthorized'
        if s.closed: return s, 'closed'
        if expected != s.reservation_revision: return s, 'stale_reservation'
        s.released += s.held
        s.held = 0
        s.closed = True
        s.reservation_revision += 1
        s.identities[identity] = command
        s.check()
        return s, 'accepted'
    else:
        raise ValueError('unknown test operation')
    proposed = {k: v[1] for k, v in s.claims.items()}
    proposed[family] = amount
    if sum(max(v, 0) for v in proposed.values()) > 10000 or sum(max(-v, 0) for v in proposed.values()) > 3000:
        return s, 'ceiling'
    if kind == 'ordinary':
        consume = max(amount, 0)
        if consume > s.held: return s, 'capacity'
        s.held -= consume
        s.consumed += consume
        # An accepted ordinary, even zero, advances its guarded observation.
        # This numeric guard convention is test scaffolding, NOT an encoding mandate.
        s.reservation_revision += 1
    else:
        previous = s.claims[family][1]
        if previous: s.postings.append(-previous)
    if amount: s.postings.append(amount)
    s.claims[family] = (claim_revision, amount)
    s.identities[identity] = command
    s.check()
    return s, 'accepted'


def reservation(s):
    return (s.held, s.consumed, s.released, s.closed)


# Hand-authored expected totals. Never compute these through apply().
# command, status, (held, consumed, released, closed), current family amounts,
# exact NEW postings. Receipt semantics are tested by the external adapter seam.
HISTORIES = {
    'ordinary-consume': [
        (('ordinary','o1','a',2500,0,True),'accepted',(9500,5500,0,False),{'a':2500},[2500]),
    ],
    'accepted-zero': [
        (('ordinary','z1','a',0,0,True),'accepted',(12000,3000,0,False),{'a':0},[]),
        (('ordinary','z2','a',2500,1,True),'claim_conflict',(12000,3000,0,False),{'a':0},[]),
    ],
    'ordinary-discount': [
        (('ordinary','d1','a',-1000,0,True),'accepted',(12000,3000,0,False),{'a':-1000},[-1000]),
    ],
    'explicit-closure': [
        (('ordinary','o1','a',2500,0,True),'accepted',(9500,5500,0,False),{'a':2500},[2500]),
        (('close','c1',1,True),'accepted',(0,5500,9500,True),{'a':2500},[]),
        (('ordinary','o2','b',1000,2,True),'closed',(0,5500,9500,True),{'a':2500},[]),
    ],
    'deadline-closure-zero': [
        (('ordinary','z1','a',0,0,True),'accepted',(12000,3000,0,False),{'a':0},[]),
        (('close','deadline',1,True),'accepted',(0,3000,12000,True),{'a':0},[]),
    ],
    'shared-invocation': [
        (('ordinary','o1','a',2500,0,True),'accepted',(9500,5500,0,False),{'a':2500},[2500]),
        (('ordinary','o2','b',4000,1,True),'accepted',(5500,9500,0,False),{'a':2500,'b':4000},[4000]),
        (('close','c1',2,True),'accepted',(0,9500,5500,True),{'a':2500,'b':4000},[]),
    ],
    'duplicate-and-stale': [
        (('ordinary','o1','a',2500,0,True),'accepted',(9500,5500,0,False),{'a':2500},[2500]),
        (('ordinary','o1','a',2500,0,True),'duplicate',(9500,5500,0,False),{'a':2500},[]),
        (('ordinary','o1','a',3000,0,True),'identity_conflict',(9500,5500,0,False),{'a':2500},[]),
        (('ordinary','o2','b',1000,0,True),'stale_reservation',(9500,5500,0,False),{'a':2500},[]),
        (('close','c1',0,True),'stale_reservation',(9500,5500,0,False),{'a':2500},[]),
    ],
    'post-closure-adjustments': [
        (('ordinary','o1','a',2500,0,True),'accepted',(9500,5500,0,False),{'a':2500},[2500]),
        (('close','c1',1,True),'accepted',(0,5500,9500,True),{'a':2500},[]),
        (('adjust','fix','a',1000,1,True),'accepted',(0,5500,9500,True),{'a':1000},[-2500,1000]),
        (('adjust','reverse','a',0,2,True),'accepted',(0,5500,9500,True),{'a':0},[-1000]),
        (('adjust','reinstate','a',2500,3,True),'accepted',(0,5500,9500,True),{'a':2500},[2500]),
        (('adjust','chargeback','a',-1000,4,True),'accepted',(0,5500,9500,True),{'a':-1000},[-2500,-1000]),
        (('adjust','attribution-fix','a',-1000,5,True),'accepted',(0,5500,9500,True),{'a':-1000},[1000,-1000]),
        (('adjust','dispute','a',0,6,True),'accepted',(0,5500,9500,True),{'a':0},[1000]),
        (('ordinary','o1','a',2500,0,True),'duplicate',(0,5500,9500,True),{'a':0},[]),
        (('adjust','fix','a',1000,1,True),'duplicate',(0,5500,9500,True),{'a':0},[]),
        (('close','c1',1,True),'duplicate',(0,5500,9500,True),{'a':0},[]),
    ],
    'authority-and-ceilings': [
        (('ordinary','bad','a',2500,0,False),'unauthorized',(12000,3000,0,False),{},[]),
        (('ordinary','o1','a',2500,0,True),'accepted',(9500,5500,0,False),{'a':2500},[2500]),
        (('adjust','bad-fix','a',1000,1,False),'unauthorized',(9500,5500,0,False),{'a':2500},[]),
        (('adjust','too-high','a',10001,1,True),'ceiling',(9500,5500,0,False),{'a':2500},[]),
        (('adjust','too-low','a',-3001,1,True),'ceiling',(9500,5500,0,False),{'a':2500},[]),
        (('adjust','limit','a',10000,1,True),'accepted',(9500,5500,0,False),{'a':10000},[-2500,10000]),
        (('adjust','stale','a',1000,1,True),'stale_claim',(9500,5500,0,False),{'a':10000},[]),
    ],
    'no-replenishment-before-close': [
        (('ordinary','o1','a',8000,0,True),'accepted',(4000,11000,0,False),{'a':8000},[8000]),
        (('adjust','reverse','a',0,1,True),'accepted',(4000,11000,0,False),{'a':0},[-8000]),
        (('ordinary','o2','b',5000,1,True),'capacity',(4000,11000,0,False),{'a':0},[]),
        (('ordinary','o3','b',4000,1,True),'accepted',(0,15000,0,False),{'a':0,'b':4000},[4000]),
        (('close','c1',2,True),'accepted',(0,15000,0,True),{'a':0,'b':4000},[]),
    ],
}


def check_step(state, previous, expected):
    _, _, totals, amounts, delta = expected
    assert reservation(state) == totals
    assert {k:v[1] for k,v in state.claims.items()} == amounts
    assert state.postings == previous.postings + delta
    state.check()


def assert_complete_or_absent(observed, before, after):
    """Unknown result is not resolved by absence alone; caller must drain/retry."""
    if observed != before and observed != after:
        raise AssertionError('partial commit: economics/reservation/claim/receipt must commit together')


def run_adapter(adapter, name):
    """Unconnected reusable numeric adapter hook, no expected rows passed to SUT.

    Adapter translates test commands into approved commands/authority fixtures.
    observe_numeric returns reservation tuple, family amounts, posting sequence.
    Replies are (category, original receipt bytes or None). Close/reopen reads
    same durable store. Concrete identity and revision guards belong to adapter.
    """
    receipts = {}
    expected_postings = []
    for command, status, totals, amounts, delta in HISTORIES[name]:
        actual_status, receipt = adapter.submit(command)
        assert actual_status == status
        if status == 'accepted':
            assert isinstance(receipt, bytes) and receipt
            receipts[command[1]] = receipt
        elif status == 'duplicate':
            assert receipt == receipts[command[1]]
        else:
            assert receipt is None
        expected_postings.extend(delta)
        expected = (totals, amounts, expected_postings)
        assert adapter.observe_numeric() == expected
        before = adapter.observe_full()
        adapter.reopen()
        assert adapter.observe_full() == before
        assert adapter.observe_numeric() == expected
    return len(HISTORIES[name])
