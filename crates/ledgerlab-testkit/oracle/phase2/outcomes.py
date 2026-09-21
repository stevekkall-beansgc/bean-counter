"""Independent approved-v0 reference equations; no Rust/production imports.

Inputs are synthetic story data, NOT a production schema. Integer atoms and
Fraction arithmetic. Every attempt yields its result and full immutable journal.
The runner never consumes expected results and never rewrites fixtures.
"""
from copy import deepcopy
from fractions import Fraction
import json
import sys

MAX = 10**30 - 1

class Refusal(Exception):
    pass

def need(ok, code):
    if not ok:
        raise Refusal(code)

def bounded(n):
    need(abs(n) <= MAX, 'ARITHMETIC_OVERFLOW')
    return n

def rounded(x):
    x = Fraction(x)
    y = abs(x) + Fraction(1, 2)
    return bounded((y.numerator // y.denominator) * (-1 if x < 0 else 1))

def xp(code, basis, exact):
    return dict(code=code, basis=str(basis), exact=dict(numerator=str(exact.numerator),
                denominator=str(exact.denominator)), rounded=str(rounded(exact)))

def key(r):
    return [r['tenant'], 'sandbox', r['agreement'], r['family'], r['target']]

def request(step):
    return dict(tenant=step.get('tenant', 'demo'), id=step['id'], target=step.get('target', 'base'),
                agreement=step.get('agreement', 'retail'), family=step.get('family', 'success'),
                source=step.get('source', 'urn:correction' if 'revision' in step else 'urn:outcome'),
                occurred=step.get('occurred', 20), evidence=step.get('evidence', ['proof']),
                change=dict(revision=step['revision'], replacement=step.get('replacement'))
                if 'revision' in step else dict(code=step.get('code', 'yes')))

def freeze(story):
    b, p = story['base'], story['policy']
    need(b.get('status', 'succeeded') == 'succeeded', 'TARGET_INELIGIBLE')
    need(b.get('final', True), 'TARGET_NOT_FINAL')
    need(not b.get('capped', False), 'OUTCOME_CAP_COMPOSITION')
    need(b.get('verified_terms', True), 'TERMS_NOT_VERIFIED')
    need(p.get('premium_limit') is not None, 'PREMIUM_BOUND_REQUIRED')
    for family in p['families']:
        if family.get('binding', 'retail') == 'supplier':
            need(b.get('supplier_path', True), 'SUPPLIER_TARGET_PATH')
    return int(b['atoms']) - int(b['booking_discount'])

def calculate(story):
    journal, outputs = [], []
    try:
        basis = freeze(story)
        freeze_code = 'FROZEN'
    except Refusal as e:
        basis, freeze_code = 0, str(e)
    available = story.get('available', True) and freeze_code == 'FROZEN'
    reversed_base = False
    frozen_policy = deepcopy(story['policy'])
    for step in story['steps']:
        if step.get('operation') == 'make_available':
            available = freeze_code == 'FROZEN'
            outputs.append(dict(status='target_available', journal=deepcopy(journal)))
            continue
        if step.get('operation') == 'reverse_base':
            reversed_base = True
            outputs.append(dict(status='base_reversed', journal=deepcopy(journal)))
            continue
        r = request(step)
        receipt, accepted = step.get('received', 21), step.get('accepted', 22)
        try:
            need(step.get('active', True) and step.get('may_read', True)
                 and step.get('scope_verified', True), 'OUTCOME_AUTHORITY')
            same_id = next((i for i, d in enumerate(journal)
                            if (d['request']['tenant'], d['request']['source'], d['request']['id']) ==
                            (r['tenant'], r['source'], r['id'])), None)
            lineage = [d for d in journal if d['key'] == key(r)]
            if same_id is not None:
                need(journal[same_id]['request'] == r, 'IDENTITY_CONFLICT')
                status = dict(status='duplicate', original=same_id)
            elif 'code' in r['change'] and lineage:
                same = deepcopy(r)
                same['id'] = lineage[0]['request']['id']
                need(same == lineage[0]['request'], 'CLAIM_CONFLICT')
                status = dict(status='duplicate', original=journal.index(lineage[0]))
            else:
                need((available or journal) and r['target'] == 'base' and r['tenant'] == 'demo', 'WAITING_DEPENDENCIES')
                need(not reversed_base, 'TARGET_REVERSED')
                family = next((f for f in frozen_policy['families'] if f['family'] == r['family']
                               and f.get('binding', 'retail') == r['agreement']), None)
                need(family is not None, 'POLICY_OUTCOME_FAMILY')
                need(step.get('evidence_verified', True) and (not family.get('evidence_required', True) or r['evidence']),
                     'OUTCOME_EVIDENCE_REQUIRED')
                need(accepted >= 12, 'INVALID_ACCEPTED_ORDER')
                correction = 'revision' in step
                if correction:
                    need(step.get('may_correct', True) and r['source'] == 'urn:correction', 'CORRECTION_UNAUTHORIZED')
                    need(lineage, 'CLAIM_MISSING')
                    need(step['revision'] == lineage[-1]['revision'], 'STALE_CORRECTION')
                    need(accepted >= lineage[-1]['accepted'], 'INVALID_ACCEPTED_ORDER')
                    endpoints = family.get('corrections', [10, 1000, 1100, 1200])
                else:
                    need(step.get('may_submit', True) and r['source'] == 'urn:outcome', 'OUTCOME_AUTHORITY')
                    endpoints = family.get('ordinary', [10, 100, 110, 120])
                need(r['occurred'] <= receipt, 'INVALID_RECEIVED_ORDER')
                need(receipt <= accepted, 'INVALID_ACCEPTED_ORDER')
                need(endpoints[0] <= r['occurred'] < endpoints[1], 'OUTCOME_WINDOW')
                need(receipt <= endpoints[2] and accepted <= endpoints[3], 'OUTCOME_DEADLINE')
                code = step.get('replacement') if correction else r['change']['code']
                if correction:
                    need((code is None and family.get('allow_reversal', True)) or
                         code in family.get('replacements', list(family['codes'])), 'CORRECTION_NOT_PERMITTED')
                need(code is None or code in family['codes'], 'POLICY_OUTCOME_CODE')
                amount = family['codes'].get(code) if code is not None else {'fixed': '0'}
                exact = Fraction(int(amount['fixed'])) if 'fixed' in amount else basis * Fraction(amount['percent']) / 100
                current = rounded(exact)
                live = {tuple(d['key']): d for d in journal}
                live.pop(tuple(key(r)), None)
                binding = family.get('binding', 'retail')
                active = [d['current'] for d in live.values() if d['binding'] == binding] + [str(current)]
                discount = bounded(sum(min(0, int(n)) for n in active))
                premium = bounded(sum(max(0, int(n)) for n in active))
                net = basis if binding == 'retail' else int(story['base']['supplier'])
                need(-discount <= net, 'DISCOUNT_EXCEEDS_BASIS')
                ceiling = int(frozen_policy['premium_limit' if binding == 'retail' else 'supplier_limit'])
                need(premium <= ceiling, 'PREMIUM_LIMIT')
                bounded(net + discount + premium)
                postings, explanations = [], []
                if correction:
                    old = lineage[-1]
                    inverse = -int(old['current'])
                    if inverse:
                        postings.append(dict(atoms=str(inverse), reverses_revision=old['revision']))
                    explanations.append(xp('EXACT_REVERSAL', basis, Fraction(inverse)))
                if current:
                    postings.append(dict(atoms=str(current), reverses_revision=None))
                explanations.append(xp('CLAIM_REVERSED' if code is None else 'ZERO_ROUNDED' if current == 0
                                       else 'OUTCOME_APPLIED', basis, exact))
                bounded(sum(int(p['atoms']) for p in postings))
                journal.append(dict(request=r, key=key(r), revision=len(lineage)+1, current=str(current),
                                    code=code, binding=binding, book=binding, version=frozen_policy['version'],
                                    basis=str(basis), roles=story['roles'][binding], received=receipt, accepted=accepted,
                                    postings=postings, explanations=explanations))
                status = dict(status='accepted')
        except Refusal as e:
            status = dict(status='rejected', code=str(e))
        outputs.append(dict(**status, journal=deepcopy(journal)))
    return dict(freeze=freeze_code, steps=outputs)

if __name__ == '__main__':
    with open(sys.argv[1], encoding='utf-8') as stream:
        stories = json.load(stream)
    print(json.dumps([calculate(s) for s in stories['histories']], separators=(',', ':')))
