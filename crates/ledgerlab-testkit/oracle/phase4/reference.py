"""Independent, bounded test arithmetic. Never imports production economics.

Test-story format only: not a public DTO, capability, frozen contract or ledger.
Amounts/ratios are strings so fixtures are language-neutral and exact.
"""
import hashlib
import json
import re
from fractions import Fraction

MAX = 10**30 - 1


class Refusal(ValueError):
    pass


def require(condition, category):
    if not condition:
        raise Refusal(category)


def integer(value):
    require(isinstance(value, str) and re.fullmatch(r'0|-?[1-9][0-9]*', value), 'MALFORMED')
    require(len(value) <= 156, 'ARITHMETIC_OVERFLOW')
    return int(value)


def bounded(value):
    require(abs(value) <= MAX, 'ARITHMETIC_OVERFLOW')
    return value


def rounded(value):
    require(max(abs(value.numerator).bit_length(), value.denominator.bit_length()) <= 512,
            'ARITHMETIC_OVERFLOW')
    q, r = divmod(abs(value.numerator), value.denominator)
    return bounded((-1 if value < 0 else 1) * (q + (2*r >= value.denominator)))


def exact(rule, basis):
    require(type(rule) is dict, 'MALFORMED')
    if set(rule) == {'fixed'}:
        return Fraction(bounded(integer(rule['fixed'])))
    require(set(rule) == {'percent'}, 'UNSUPPORTED_OPERATOR')
    ratio = rule['percent']
    require(type(ratio) is list and len(ratio) == 2, 'MALFORMED')
    n, d = map(integer, ratio)
    require(d > 0 and max(abs(n).bit_length(), d.bit_length()) <= 512, 'MALFORMED')
    value = Fraction(n, d)
    require((value.numerator, value.denominator) == (n, d), 'NONCANONICAL_RATIO')
    return basis * value / 100


def encoding(value):
    # Local ASCII-key story encoding, deliberately NOT ledger canonical bytes.
    return json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(',', ':')).encode()


def digest(kind, value):
    return hashlib.sha256(b'phase4-test-story/1\0' + kind.encode() + b'\0' + encoding(value)).hexdigest()


def candidate_digest(candidate):
    return digest('candidate', ({k:v for k,v in candidate.items() if k != 'label'} if isinstance(candidate, dict) else candidate))


def strict(raw):
    def pairs(items):
        result = {}
        for k, v in items:
            require(k not in result, 'MALFORMED')
            result[k] = v
        return result
    def number(token):
        require(token != '-0' and abs(int(token)) <= 9007199254740991, 'MALFORMED')
        return int(token)
    def reject(_):
        raise Refusal('MALFORMED')
    require(len(raw) <= 262144, 'LIMIT')
    try:
        value = json.loads(raw.decode('utf-8'), object_pairs_hook=pairs,
                           parse_int=number, parse_float=reject, parse_constant=reject)
        def check(v):
            require(v is not None, 'MALFORMED')
            if isinstance(v, str):
                v.encode('utf-8')
            if isinstance(v, dict):
                for k, x in v.items(): check(k); check(x)
            if isinstance(v, list):
                for x in v: check(x)
        check(value)
        return value
    except (UnicodeError, json.JSONDecodeError, RecursionError):
        raise Refusal('MALFORMED') from None


def validate_candidate(source, candidate):
    require(type(candidate) is dict, 'MALFORMED')
    require(set(candidate) == {'label', 'terms', 'rules'}, 'UNSUPPORTED_CHANGE')
    require(type(candidate['rules']) is dict, 'MALFORMED')
    require(candidate['terms'] == source['terms'], 'UNSUPPORTED_CHANGE')
    require(set(candidate['rules']) == set(source['original']['rules']), 'MEMBERSHIP_CHANGE')
    for family, codes in source['original']['rules'].items():
        require(type(candidate['rules'][family]) is dict, 'MALFORMED')
        require(set(candidate['rules'][family]) == set(codes), 'MEMBERSHIP_CHANGE')
        # Supplier terms never vary in this approved first slice.
        if source['terms']['families'][family]['book'] == 'supplier':
            require(candidate['rules'][family] == codes, 'SUPPLIER_TERMS_CHANGE')
        for rule in candidate['rules'][family].values():
            exact(rule, integer(source['terms']['retail_basis']))


def validate_source(source, anchor):
    require(digest('source', source) == anchor, 'SOURCE_INTEGRITY')
    require(source['complete'] is True, 'INCOMPLETE')
    require(source['semantics'] == 'synthetic-outcome-1', 'REPLAY_UNAVAILABLE')
    require(source['predecessors'] == [], 'UNSUPPORTED_PREDECESSOR')
    require(source['terms']['base_cap'] is False, 'OUTCOME_CAP_COMPOSITION')
    require(source['steps'] and source['steps'][-1]['kind'] == 'close', 'INCOMPLETE')
    require(len(source['steps']) <= 64, 'LIMIT')
    require(len(source['receipt_refs']) == len(source['steps']) + 1, 'INCOMPLETE')


def evaluate(source, candidate):
    validate_candidate(source, candidate)
    terms = source['terms']
    basis = integer(terms['retail_basis'])
    live, revisions, rows = {}, {}, []
    for step in source['steps']:
        kind = step['kind']
        if kind == 'close':
            rows.append({'step': step['id'], 'kind': kind, 'delta': '0',
                         'retail': str(bounded(basis + sum(v for f,v in live.items()
                              if terms['families'][f]['book'] == 'retail'))),
                         'supplier': str(bounded(integer(terms['supplier_base']) + sum(v for f,v in live.items()
                              if terms['families'][f]['book'] == 'supplier'))),
                         'live': {f:str(v) for f,v in live.items()},
                         'source_capacity': step['capacity']})
            continue
        require(kind in ('ordinary', 'correct', 'reverse'), 'UNSUPPORTED_OPERATOR')
        family = step['family']
        require(family in terms['families'], 'MEMBERSHIP_CHANGE')
        info = terms['families'][family]
        prior = live.get(family, 0)
        if kind == 'ordinary':
            require(family not in live, 'CLAIM_CONFLICT')
            require(step['revision'] == '1', 'STALE_REVISION')
        else:
            require(family in live and integer(step['revision']) == revisions[family] + 1, 'STALE_REVISION')
        value = Fraction(0) if kind == 'reverse' else exact(candidate['rules'][family][step['code']], basis)
        amount = rounded(value)
        live[family], revisions[family] = amount, integer(step['revision'])
        for book, base, premium in [('retail', basis, integer(terms['retail_premium'])),
                                    ('supplier', integer(terms['supplier_base']), integer(terms['supplier_premium']))]:
            values = [v for f,v in live.items() if terms['families'][f]['book'] == book]
            require(sum(max(v, 0) for v in values) <= premium, 'PREMIUM_LIMIT')
            require(sum(max(-v, 0) for v in values) <= base, 'DISCOUNT_CAPACITY')
        inverse = -prior if kind != 'ordinary' else 0
        rows.append({'step': step['id'], 'kind': kind, 'family': family, 'code': step.get('code', 'reverse'),
                     'book': info['book'], 'roles': info['roles'], 'binding': info['binding'],
                     'basis_name': 'original_final_booked_retail_net', 'basis': str(basis),
                     'rational': [str(value.numerator), str(value.denominator)],
                     'inverse': str(inverse), 'replacement': str(amount),
                     'delta': str(bounded(inverse + amount)),
                     'reason': 'CLAIM_REVERSED' if kind == 'reverse' else ('ZERO_ROUNDED' if amount == 0 else 'OUTCOME_APPLIED'),
                     'inverse_reason': 'EXACT_REVERSAL' if kind != 'ordinary' else 'NOT_APPLICABLE',
                     'nonzero_components': [str(v) for v in ([inverse] if kind != 'ordinary' else []) + [amount] if v],
                     'retail': str(bounded(basis + sum(v for f,v in live.items() if terms['families'][f]['book'] == 'retail'))),
                     'supplier': str(bounded(integer(terms['supplier_base']) + sum(v for f,v in live.items() if terms['families'][f]['book'] == 'supplier'))),
                     'live': {f:str(v) for f,v in live.items()}, 'source_capacity': step['capacity']})
    return rows


def compare(source, anchor, candidates, mode='hypothetical'):
    require(mode in ('hypothetical', 'original-replay', 'future-terms-description', 'authorized-correction-description'), 'UNSUPPORTED_MODE')
    require(2 <= len(candidates) <= 8, 'CANDIDATE_LIMIT')
    validate_source(source, anchor)
    result = []
    for candidate in candidates:
        item = {'committed': False, 'mode': mode, 'source_digest': anchor,
                'candidate_digest': candidate_digest(candidate), 'source_revision': source['revision'],
                'observation': source['observation'], 'historical_receipt_refs': source['receipt_refs']}
        try:
            if mode in ('original-replay', 'authorized-correction-description'):
                require(candidate_digest(candidate) == candidate_digest(source['original']), 'ORIGINAL_TERMS_REQUIRED')
            item.update(status='complete', steps=evaluate(source, candidate))
            item['latest'] = item['steps'][-1]['live']
            if mode == 'future-terms-description':
                item['prerequisites'] = ['new accepted binding', 'future activation', 'future evidence and authority', 'new target complete membership']
            if mode == 'authorized-correction-description':
                item['prerequisites'] = ['separate ordinary acceptance', 'current correction permission', 'exact current revision']
        except (Refusal, KeyError) as error:
            item.update(status='refused', diagnostic=str(error) if isinstance(error, Refusal) else 'MISSING_CODE')
        result.append(item)
    return result
