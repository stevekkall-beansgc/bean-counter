#!/usr/bin/env python3
"""Read-only, standard-library oracle. Never imports a production/contract evaluator.

The frozen journal supplies expected bytes. Independent projections, integer/Fraction
arithmetic and SHA-256 validate those bytes before the Rust harness may use them.
This is deliberately a fixture oracle, not a second general policy implementation.
"""
import copy
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import re
import sys

TABLES = {
    'document': 'documents', 'party': 'parties', 'binding-record': 'bindings',
    'source-grant-record': 'source_grants', 'snapshot-ref': 'snapshots',
    'event': 'events', 'delivery-key': 'delivery_keys', 'claim': 'claims',
    'effect': 'effects', 'action': 'actions', 'action-source': 'action_sources',
    'action-dependency': 'action_dependencies', 'explanation': 'explanations',
    'intention': 'intentions', 'control-transition': 'control_transitions',
    'chain-revision': 'chain_revisions', 'decision-manifest': 'decision_manifests',
    'receipt': 'accepted_receipts',
}
ZERO_TABLES = ('offers', 'payer_delegations', 'invocations', 'invocation_heads',
               'reservations', 'stage_heads', 'links', 'action_links',
               'intention_dependencies', 'dispatch_attempts', 'delivery_observations',
               'inbox', 'projection_rows', 'projection_checkpoints', 'transfer_sessions',
               'fake_receipts', 'diagnostics')
PREFIX = dict(event='ev', claim='cl', decision='dc', receipt='rc', effect='ef',
              action='ac', obligation='ob', intention='in', document='doc',
              explanation='xp', **{'snapshot-ref': 'sr', 'control-transition': 'ct'})


def require(condition, message):
    if not condition:
        raise ValueError(message)


def strict(raw):
    text = raw.decode('utf-8', errors='strict')
    require(not text.startswith('\ufeff'), 'BOM')

    def pairs(items):
        result = {}
        for key, value in items:
            require(key not in result, 'duplicate key: ' + key)
            result[key] = value
        return result

    def integer(token):
        require(token != '-0' and abs(int(token)) <= 9007199254740991, 'unsafe integer')
        return int(token)

    def forbidden(token):
        raise ValueError('non-integral numeric token: ' + token)

    value = json.loads(text, object_pairs_hook=pairs, parse_int=integer,
                       parse_float=forbidden, parse_constant=forbidden)
    # Encoding also rejects escaped lone surrogates throughout keys and values.
    canonical(value)
    return value


def canonical(value):
    if isinstance(value, dict):
        keys = sorted(value, key=lambda k: k.encode('utf-16-be'))
        return b'{' + b','.join(canonical(k) + b':' + canonical(value[k]) for k in keys) + b'}'
    if isinstance(value, list):
        return b'[' + b','.join(map(canonical, value)) + b']'
    require(value is None or isinstance(value, (str, bool, int)), 'non-profile value')
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8')


def digest(domain, value):
    return hashlib.sha256(b'ledgerlab/' + domain.encode('ascii') + b'/1\0' + canonical(value)).hexdigest()


def identity(domain, value):
    return PREFIX[domain] + '_' + digest(domain, value)


def content(row):
    kind, body = row['kind'], row['body']
    if kind == 'document':
        return digest('document', [row['document_type'], 1, body])
    if kind == 'event':
        return digest('event-content', body)
    if kind == 'decision-manifest':
        return digest('decision-content', body)
    return digest('record-content', [kind, 1, body])


def row_key(row):
    return row['kind'].encode('utf-8'), canonical(row['id'])


def journal(raw):
    require(raw.endswith(b'\n'), 'missing final journal LF')
    lines = raw[:-1].split(b'\n')
    rows = [strict(line) for line in lines]
    require(all(canonical(row) == line for row, line in zip(rows, lines)), 'noncanonical journal')
    keys = [row_key(row) for row in rows]
    require(keys == sorted(set(keys)), 'journal order/duplicate key')
    for row in rows:
        required_keys = {'kind', 'scope', 'id', 'body', 'content_hash'}
        if row['kind'] == 'document':
            required_keys.add('document_type')
        require(set(row) == required_keys, 'unknown/missing envelope field')
        require(row['scope'] == ['demo', 'sandbox'], 'fixture scope')
        require(row['content_hash'] == 'sha256:' + content(row), 'record digest: ' + row['kind'])
    return rows


def indexed_projections(rows):
    """Logical duplicate columns per design §12; obtain actuals from SQL columns.

    Hex fields preserve byte columns without JSON escaping ambiguity. SQL NULL
    optional columns are represented by omission, counters by canonical strings.
    No indexed column may be reconstructed from the body's JSON by the adapter.
    """
    fields = {
        'party': 'role_metadata_doc',
        'source-grant-record': 'principal_id source grant_doc',
        'binding-record': 'agreement_id version policy_doc assent_doc offer_id roles_doc context_doc currency scale',
        'snapshot-ref': 'event_id document_id purpose',
        'claim': 'source operation_id kind token facts_hash event_id',
        'effect': 'agreement_id component claim_id namespace facts_hash action_id',
        'action': 'event_id decision_id effect_id obligation_id kind book component binding_id snapshot_doc roles_doc reverses allocation_parent',
        'action-source': 'action_id event_id',
        'action-dependency': 'action_id input_action_id',
        'explanation': 'event_id ordinal code rule_id',
        'intention': 'event_id obligation_id destination_id idempotency_key',
        'control-transition': 'control_kind control_id from_revision to_revision event_id document_id',
        'chain-revision': 'chain_id revision event_id decision_id',
        'decision-manifest': 'event_id chain_id revision',
        'receipt': 'event_id decision_id',
        'delivery-key': 'source external_id ingress_hash canonical_event_id kind',
    }
    by_event = {r['body']['canonical_event_id']: r['body'] for r in rows if r['kind'] == 'delivery-key'}
    claims = {r['body']['event_id']: r['body'] for r in rows if r['kind'] == 'claim'}
    projections = []
    for row in sorted(rows, key=row_key):
        kind, body = row['kind'], row['body']
        columns = {k: body[k] for k in fields.get(kind, '').split() if k in body}
        if kind == 'document':
            columns['kind'] = row['document_type']
        elif kind == 'event':
            columns.update(source=body['source'], external_id=body['id'], operation_id=body['operation_id'],
                           kind=body['type'], chain_id=body['chain'], decision_id=identity('decision', [row['id']]),
                           ingress_bytes_hex=canonical(by_event[row['id']]['ingress']).hex(),
                           ingress_hash=by_event[row['id']]['ingress_hash'], claim_facts_hash=claims[row['id']]['facts_hash'])
        elif kind == 'effect':
            columns['match_key_bytes_hex'] = canonical(body['match_key']).hex()
        elif kind == 'action':
            columns.update(body['amount'])
        elif kind == 'decision-manifest':
            columns['decision_hash'] = row['content_hash']
        projections.append({'kind': kind, 'scope': row['scope'], 'id': row['id'],
                            'schema_version': 1, 'content_hash': row['content_hash'],
                            'canonical_bytes_hex': canonical(body).hex(), 'columns': columns})
    return projections


def normalized_quantity(token):
    require(isinstance(token, str) and len(token) <= 64 and
            re.fullmatch(r'[0-9]+(?:\.[0-9]{1,18})?', token), 'quantity syntax')
    whole, _, fraction = token.partition('.')
    fraction = fraction.rstrip('0')
    whole = whole.lstrip('0') or '0'
    coefficient = (whole + fraction).lstrip('0') or '0'
    require(len(coefficient) <= 30, 'quantity coefficient')
    return whole + ('.' + fraction if fraction else '')


def normalize_slice(raw):
    event = strict(raw)
    allowed = {'schema', 'id', 'source', 'operation_id', 'type', 'customer', 'chain',
               'status', 'quantity', 'unit', 'links', 'evidence', 'extensions'}
    require(isinstance(event, dict) and not (event.keys() - allowed), 'unknown field')
    require(all(value is not None for value in event.values()), 'null field')
    require(event['schema'] == 'ledger-event/1', 'schema')
    event.setdefault('operation_id', event['id'])
    for name, value in [('status', 'succeeded'), ('quantity', '1'), ('unit', 'call'),
                        ('links', []), ('evidence', []), ('extensions', {})]:
        event.setdefault(name, value)
    event['quantity'] = normalized_quantity(event['quantity'])
    return event


def nearest(value):
    value = Fraction(value)
    quotient, remainder = divmod(abs(value.numerator), value.denominator)
    return (-1 if value < 0 else 1) * (quotient + (2 * remainder >= value.denominator))


def allocate(total, weights):
    quotas = [abs(total) * Fraction(w, sum(weights)) for w in weights]
    amounts = [q.numerator // q.denominator for q in quotas]
    # Input indexes stand for ascending recipient/effect UTF-8 tie keys a,b,c.
    order = sorted(range(len(weights)), key=lambda i: (-(quotas[i] - amounts[i]), i))
    for index in order[:abs(total) - sum(amounts)]:
        amounts[index] += 1
    return [(-1 if total < 0 else 1) * value for value in amounts]


def audit_math(root):
    r = nearest
    cap = -max(0, 135 + 200 - 315)
    share = r(Fraction((200 + cap) * 25, 100))
    actual = {
        'first_slice': [100, -r(Fraction(100 * 20, 100)), 80],
        'priority': 100 + 20 - r(Fraction(100 * 20, 100)),
        'unit_fraction': r(Fraction('0.07') * Fraction('1.5') * 100),
        'signed_ties': [r(Fraction(s) * 100) for s in ['1.005', '-1.005']],
        'signed_non_ties': [r(Fraction(s) * 100) for s in ['1.0049', '-1.0049']],
        'additive': 100 - 2 * r(Fraction(100, 10)),
        'sequential': 90 - r(Fraction(90, 10)),
        'equal_allocation': [allocate(100, [1, 1, 1]), allocate(-100, [1, 1, 1])],
        'weighted_allocation': allocate(11, [1, 2, 3]),
        'uncapped': [72 + 27 + 36 + 200, 10 + 5, 15 + 50, 335 - (20 + 15 + 65)],
        'capped': [cap, 200 + cap, share, 15 + share, 315 - (20 + 15 + 15 + share)],
        'share_ceiling': min(r(Fraction(1000 * 25, 100)), 50),
        'reversal': [-amount for amount in [-20, 500]],
        'rounding_stage': [3 * r(Fraction('0.004') * 100), r(Fraction('0.012') * 100)],
        'overflow_boundary': (10**30 - 1) + 1 > 10**30 - 1,
    }
    vectors = strict((root / 'fixtures/math/vectors.json').read_bytes())['vectors']
    require(set(actual) == {v['name'] for v in vectors}, 'math vector coverage')
    for vector in vectors:
        require(actual[vector['name']] == vector['expected'], 'math: ' + vector['name'])
    # Properties independent of the examples: conservation/sign and signed symmetry.
    for total in range(-128, 129):
        for weights in ([1, 1, 1], [0, 2, 5], [1, 2, 3]):
            require(sum(allocate(total, weights)) == total, 'allocation conservation')
            require(allocate(-total, weights) == [-x for x in allocate(total, weights)], 'allocation sign')
        for denominator in range(1, 18):
            require(r(Fraction(total, denominator)) == -r(Fraction(-total, denominator)), 'round sign')


def audit(root):
    frozen = strict((root / 'contracts/freeze.json').read_bytes())['files']
    for path, expected in frozen.items():
        require(hashlib.sha256((root / path).read_bytes()).hexdigest() == expected, 'frozen drift: ' + path)
    folder = root / 'fixtures/journals/first-slice'
    files = strict((folder / 'file-digests.json').read_bytes())['files']
    require(set(files) == {p.name for p in folder.iterdir()} - {'file-digests.json'}, 'fixture file inventory')
    for name, entry in files.items():
        raw = (folder / name).read_bytes()
        require(len(raw) == entry['bytes'] and hashlib.sha256(raw).hexdigest() == entry['sha256'], 'fixture drift: ' + name)
        if name.endswith('.json'):
            require(canonical(strict(raw)) == raw, 'canonical file: ' + name)
    vectors = strict((folder / 'vectors.json').read_bytes())['vectors']
    require(len(vectors) == 60, 'vector count')
    require([v['name'] for v in vectors] == sorted({v['name'] for v in vectors}), 'vector ordering')
    for vector in vectors:
        encoded = canonical(vector['value'])
        framed = b'ledgerlab/' + vector['domain'].encode('ascii') + b'/1\0' + encoded
        require(encoded == vector['canonical_utf8'].encode('utf-8'), 'vector utf8: ' + vector['name'])
        require(encoded.hex() == vector['canonical_hex'] and framed.hex() == vector['hash_input_hex'], 'vector framing')
        require(hashlib.sha256(framed).hexdigest() == vector['sha256'], 'vector digest')
        if 'id' in vector:
            require(identity(vector['domain'], vector['value']) == vector['id'], 'vector identity')
    seed = journal((folder / 'seed-documents.jsonl').read_bytes()) + journal((folder / 'seed-records.jsonl').read_bytes())
    accepted = journal((folder / 'accepted-records.jsonl').read_bytes())
    require(len(seed) == 10 and len(accepted) == 25, 'journal row counts')
    require(not ({row_key(r) for r in seed} & {row_key(r) for r in accepted}), 'seed/accept collision')
    rows = seed + accepted
    by_kind = lambda kind: [r for r in rows if r['kind'] == kind]
    one = lambda kind: by_kind(kind)[0]['body']
    event = one('event')
    scope = ['demo', 'sandbox']
    eid = identity('event', [*scope, event['source'], event['id']])
    cid = identity('claim', [scope, event['source'], event['operation_id'], 'completion', 'completion'])
    did, rid = identity('decision', [eid]), identity('receipt', [eid])
    ids = {'event': eid, 'claim': cid, 'decision-manifest': did, 'receipt': rid}
    for kind, expected in ids.items():
        require(by_kind(kind)[0]['id'] == expected, 'derived identity: ' + kind)
    for row in by_kind('document'):
        require(row['id'] == identity('document', [row['document_type'], 1, row['body']]), 'document identity')
    documents = {r['document_type']: r for r in by_kind('document')}
    policy = documents['policy']['body']
    roles = {k: v for k, v in documents['roles']['body'].items() if k != 'schema'}
    binding = documents['binding']['body']
    facts = {'schema': 'ledger-claim-facts/1', **{k: event[k] for k in
             ('type', 'chain', 'customer', 'status', 'quantity', 'unit', 'links', 'evidence')}}
    require(canonical(facts) == (folder / 'claim-facts.json').read_bytes(), 'claim projection')
    require(one('claim')['facts_hash'] == 'sha256:' + digest('claim-facts', facts), 'claim facts hash')
    base = Fraction(policy['rules'][0]['amount']['fixed']) * 10**policy['scale']
    discount = -Fraction(nearest(base)) * Fraction(policy['rules'][1]['amount']['percent']) / 100
    expected_atoms = {'generation.base': nearest(base), 'generation.discount': nearest(discount)}
    require(list(expected_atoms.values()) == [100, -20] and sum(expected_atoms.values()) == 80, 'first slice arithmetic')
    actions = by_kind('action')
    effects = {r['id']: r['body'] for r in by_kind('effect')}
    for row in actions:
        action = row['body']
        effect_id = identity('effect', [scope, binding['agreement_id'], action['component'], cid, 'self', 'original'])
        require(action['effect_id'] == effect_id and row['id'] == identity('action', [effect_id]), 'effect/action ID')
        require(int(action['amount']['atoms']) == expected_atoms[action['component']], 'action arithmetic')
        require(action['roles'] == roles and action['roles_doc'] == documents['roles']['id'], 'roles projection')
        effect = effects[effect_id]
        projection = {'schema': 'ledger-effect-facts/1',
                      **{k: effect[k] for k in ('scope', 'agreement_id', 'component', 'claim_id', 'match_key', 'namespace')},
                      **{k: action[k] for k in ('kind', 'book', 'amount', 'roles', 'sources', 'links', 'inputs')}}
        require(effect['facts_hash'] == 'sha256:' + digest('effect-facts', projection), 'effect facts')
        ordinal = 1 if action['component'] == 'generation.base' else 2
        require(canonical(projection) == (folder / f'effect-facts-{ordinal}.json').read_bytes(), 'effect projection bytes')
        obligation = identity('obligation', [scope, binding['agreement_id'], action['book'], 'USD', 2, roles])
        require(action['obligation_id'] == obligation, 'obligation identity')
    action_ids = sorted(r['id'] for r in actions)
    intention = one('intention')
    require(intention['id'] == identity('intention', [scope, 'fake', obligation, action_ids]), 'intention identity')
    require(intention['idempotency_key'] == intention['id'], 'stable destination key')
    require(int(intention['amount']['atoms']) == sum(expected_atoms.values()), 'intention amount')
    require(intention['payload']['amount'] == intention['amount'], 'payload amount')
    require(sum(int(a['amount']['atoms']) for a in intention['payload']['actions']) == 80, 'payload breakdown')
    for row in by_kind('snapshot-ref'):
        body = row['body']
        require(row['id'] == identity('snapshot-ref', [eid, body['purpose'], body['document_id']]), 'snapshot ref ID')
    for row in by_kind('explanation'):
        body = row['body']
        require(row['id'] == identity('explanation', [eid, body['ordinal']]), 'explanation ID')
        value = [base, discount][body['ordinal']]
        require(body['unrounded_atoms'] == {'numerator': str(value.numerator), 'denominator': str(value.denominator)}, 'explanation rational')
        require(body['rounded_atoms'] == str(nearest(value)), 'explanation rounding')
    transition = one('control-transition')
    require(transition['id'] == identity('control-transition', [scope, 'chain', 'demo-slice', '1']), 'transition ID')
    for row in rows:
        body = row['body']
        if row['kind'] in ('action-source', 'action-dependency', 'delivery-key', 'chain-revision'):
            fields = {'action-source': ['action_id', 'event_id'], 'action-dependency': ['action_id', 'input_action_id'],
                      'delivery-key': ['source', 'external_id'], 'chain-revision': ['chain_id', 'revision']}[row['kind']]
            require(row['id'] == [scope, *[body[f] for f in fields]], 'composite key')
    mapping = one('delivery-key')
    require(mapping['ingress'] == event and mapping['ingress_hash'] == 'sha256:' + digest('ingress', event), 'ingress')
    manifest, receipt = one('decision-manifest'), one('receipt')
    members = [dict(kind=r['kind'], id=r['id'], content_hash=r['content_hash']) for r in rows
               if r['kind'] not in ('decision-manifest', 'receipt', 'party', 'source-grant-record', 'binding-record')]
    members.sort(key=row_key)
    require(len(members) == 29 and manifest['members'] == members, 'complete manifest')
    require(receipt['decision_hash'] == 'sha256:' + digest('decision-content', manifest), 'receipt manifest hash')
    require(receipt['content_hash'] == 'sha256:' + digest('event-content', event), 'receipt event hash')
    require(canonical(receipt) == (folder / 'receipt.json').read_bytes(), 'receipt bytes')
    raw_input = (root / 'fixtures/canonical/valid/first-slice-input.json').read_bytes()
    require(normalize_slice(raw_input) == event, 'first slice normalization')
    equivalent = (root / 'fixtures/canonical/valid/decimal-equivalent.json').read_bytes()
    require(normalize_slice(equivalent) == event, 'equivalent quantity')
    unicode_input = strict((root / 'fixtures/canonical/valid/unicode-order.json').read_bytes())
    require(canonical(unicode_input) == (root / 'fixtures/canonical/expected/unicode-order.json').read_bytes(), 'UTF-16 order')
    cases = strict((root / 'fixtures/canonical/cases.json').read_bytes())
    for name in cases['invalid']:
        try:
            strict((root / 'fixtures/canonical/invalid' / name).read_bytes())
        except (ValueError, UnicodeError):
            pass
        else:
            raise ValueError('invalid JSON accepted: ' + name)
    for case in cases['schema_invalid']:
        try:
            normalize_slice(canonical(case['event']))
        except (ValueError, UnicodeError):
            pass
        else:
            raise ValueError('invalid input accepted: ' + case['name'])
    audit_math(root)
    return seed, accepted


def emit(key, raw):
    print(key + '\t' + raw.hex())


def bundle(root):
    seed, accepted = audit(root)
    folder = root / 'fixtures/journals/first-slice'
    for name, filename in [('pre_state', 'preseed-state.json'), ('operational', 'post-acceptance-state.json'),
                           ('receipt', 'receipt.json')]:
        emit(name, (folder / filename).read_bytes())
    before = strict((folder / 'preseed-state.json').read_bytes())
    after = copy.deepcopy(before)
    after['chain'] = strict((folder / 'post-acceptance-state.json').read_bytes())['chain']
    emit('post_state', canonical(after))
    delivery_enabled = copy.deepcopy(after)
    delivery_enabled['dispatch_enabled'] = True
    delivery_enabled['dispatch_hold'] = False
    emit('delivery_enabled_state', canonical(delivery_enabled))
    for row in sorted(seed, key=row_key):
        emit('seed_row', canonical(row))
    for row in sorted(seed + accepted, key=row_key):
        emit('post_row', canonical(row))
    for projection in indexed_projections(seed):
        emit('seed_index', canonical(projection))
    for projection in indexed_projections(seed + accepted):
        emit('post_index', canonical(projection))
    for kind, table in TABLES.items():
        print(f'count\t{table}\t{sum(r["kind"] == kind for r in seed)}\t{sum(r["kind"] == kind for r in accepted)}')
    for table in ZERO_TABLES:
        print(f'count\t{table}\t0\t0')
    for table in ('installation', 'authority_heads', 'binding_heads', 'chains', 'dispatcher_head'):
        print(f'count\t{table}\t1\t0')
    print('count\tdelivery_state\t0\t1')
    raw = (root / 'fixtures/canonical/valid/first-slice-input.json').read_bytes()
    event = strict(raw)
    emit('input', raw)
    variants = {'equivalent': {'quantity': '1.00'}, 'identity_conflict': {'quantity': '2'},
                'semantic_duplicate': {'id': 'generation-alias'},
                'semantic_conflict': {'id': 'generation-conflict', 'quantity': '2'},
                'unauthorized_source': {'source': 'urn:unauthorized:app'},
                'zero': {'id': 'generation-failed', 'operation_id': 'generation-failed', 'status': 'failed', 'quantity': '0'},
                'unknown': {'price': '1000'}, 'null': {'quantity': None}}
    for name, changed in variants.items():
        emit(name, canonical({**event, **changed}))
    alias = normalize_slice(canonical({**event, **variants['semantic_duplicate']}))
    emit('alias_ingress', canonical(alias))
    emit('alias_ingress_hash', ('sha256:' + digest('ingress', alias)).encode())
    zero = normalize_slice(canonical({**event, **variants['zero']}))
    emit('zero_event_id', identity('event', ['demo', 'sandbox', zero['source'], zero['id']]).encode())
    zero_operational = strict((folder / 'post-acceptance-state.json').read_bytes())
    del zero_operational['delivery_state']
    emit('zero_operational', canonical(zero_operational))
    emit('duplicate_key', canonical(event)[:-1] + b',"quantity":"1"}')
    for path in sorted((root / 'fixtures/canonical/invalid').iterdir()):
        emit('invalid:' + path.stem, path.read_bytes())
    schedule = strict((root / 'fixtures/failures/first-slice.json').read_bytes())
    for write in schedule['writes']:
        require(write['failpoints'] == ['before_each', 'after_each'], 'unsupported failpoint schedule')
        print(f'write\t{write["name"]}\t{write["items"]}')
    intention = next(r['body'] for r in accepted if r['kind'] == 'intention')
    emit('intention_id', intention['id'].encode())
    emit('payload', canonical(intention['payload']))
    emit('payload_digest', ('sha256:' + digest('intention-payload', intention['payload'])).encode())



def verify_zero(root, raw):
    seed, accepted = audit(root)
    actual = journal(raw)
    event = normalize_slice((root / 'fixtures/canonical/valid/first-slice-input.json').read_bytes())
    event.update(id='generation-failed', operation_id='generation-failed', status='failed', quantity='0')
    scope = ['demo', 'sandbox']
    old_event = next(r['id'] for r in accepted if r['kind'] == 'event')
    new_event = identity('event', [*scope, event['source'], event['id']])
    old_claim = next(r['id'] for r in accepted if r['kind'] == 'claim')
    new_claim = identity('claim', [scope, event['source'], event['operation_id'], 'completion', 'completion'])
    replacements = {old_event: new_event, old_claim: new_claim, 'generation-1': 'generation-failed'}
    for domain in ('decision', 'receipt'):
        replacements[identity(domain, [old_event])] = identity(domain, [new_event])

    def replace(value):
        if isinstance(value, dict):
            return {k: replace(v) for k, v in value.items()}
        if isinstance(value, list):
            return [replace(v) for v in value]
        return replacements.get(value, value) if isinstance(value, str) else value

    expected = []
    removed = {'effect', 'action', 'action-source', 'action-dependency', 'intention', 'explanation', 'decision-manifest', 'receipt'}
    for original in accepted:
        if original['kind'] in removed:
            continue
        row = replace(original)
        body = row['body']
        if row['kind'] == 'event':
            row['body'] = event
        elif row['kind'] == 'delivery-key':
            body['ingress'] = event
            body['ingress_hash'] = 'sha256:' + digest('ingress', event)
        elif row['kind'] == 'claim':
            facts = {'schema': 'ledger-claim-facts/1', **{k: event[k] for k in ('type', 'chain', 'customer', 'status', 'quantity', 'unit', 'links', 'evidence')}}
            body['facts_hash'] = 'sha256:' + digest('claim-facts', facts)
        elif row['kind'] == 'snapshot-ref':
            row['id'] = body['id'] = identity('snapshot-ref', [new_event, body['purpose'], body['document_id']])
        row['content_hash'] = 'sha256:' + content(row)
        expected.append(row)
    explanations = [r for r in actual if r['kind'] == 'explanation']
    require(len(explanations) == 1, 'one failed-work explanation')
    explanation = explanations[0]
    body = explanation['body']
    require(explanation['id'] == body['id'] == identity('explanation', [new_event, 0]), 'zero explanation identity')
    required = {'schema': 'ledger-explanation/1', 'scope': scope, 'event_id': new_event,
                'ordinal': 0, 'code': 'FAILED_WORK', 'binding_id': 'demo-retail-v1', 'action_ids': []}
    require(all(body.get(k) == v for k, v in required.items()), 'zero explanation fields')
    require(body['outcome'] in ('skipped', 'zero'), 'zero explanation outcome')
    if 'rule_id' in body:
        require(isinstance(body['rule_id'], str) and re.fullmatch(r'[a-z][a-z0-9_.-]{0,63}', body['rule_id']), 'zero rule ID')
    # Frozen design does not specify exact FAILED_WORK display inputs. Permit only
    # retained references; no invented economics. This is explicitly not a new golden.
    require(set(body) <= set(required) | {'id', 'outcome', 'input_refs', 'inputs', 'rule_id'}, 'zero explanation extra economics')
    require(isinstance(body['input_refs'], list) and isinstance(body['inputs'], list), 'zero inputs')
    documents = {r['id'] for r in seed + expected if r['kind'] == 'document'}
    require(body['input_refs'] == sorted(set(body['input_refs'])) and set(body['input_refs']) <= documents, 'zero references')
    for item in body['inputs']:
        require(set(item) == {'kind', 'name', 'value'} and item['kind'] in ('binding_field', 'document_ref', 'source_id', 'boolean'), 'zero tagged input')
        require(isinstance(item['name'], str) and 0 < len(item['name'].encode()) <= 128, 'zero input name')
        if item['kind'] == 'document_ref':
            require(item['value'] in documents, 'zero input reference')
        elif item['kind'] == 'boolean':
            require(isinstance(item['value'], bool), 'zero boolean')
        else:
            require(isinstance(item['value'], str) and 0 < len(item['value'].encode()) <= 256, 'zero string input')
    expected.append(explanation)
    manifest = replace(next(r for r in accepted if r['kind'] == 'decision-manifest'))
    manifest['body']['explanation_ids'] = [explanation['id']]
    manifest['body']['members'] = sorted([{'kind': r['kind'], 'id': r['id'], 'content_hash': r['content_hash']}
        for r in expected + seed if r['kind'] not in ('party', 'source-grant-record', 'binding-record')], key=row_key)
    require(len(manifest['body']['members']) == 20, 'zero manifest cardinality')
    manifest['content_hash'] = 'sha256:' + content(manifest)
    expected.append(manifest)
    receipt = replace(next(r for r in accepted if r['kind'] == 'receipt'))
    receipt['body'].update(action_ids=[], intention_ids=[], content_hash='sha256:' + digest('event-content', event), decision_hash=manifest['content_hash'])
    receipt['content_hash'] = 'sha256:' + content(receipt)
    expected.append(receipt)
    require(len(expected) == 16, 'zero immutable delta')
    require(actual == sorted(seed + expected, key=row_key), 'zero journal differs from independent completion projection')
    return canonical(receipt['body'])

if __name__ == '__main__':
    root = Path(sys.argv[2]).resolve()
    if sys.argv[1] == 'audit':
        audit(root)
        print('PASS independent oracle: freeze, 60 vectors, 25 rows, 29 members, 80 atoms, 15 math vectors')
    elif sys.argv[1] == 'bundle':
        bundle(root)
    elif sys.argv[1] == 'verify-zero':
        raw = sys.stdin.buffer.read()
        emit('receipt', verify_zero(root, raw))
        for projection in indexed_projections(journal(raw)):
            emit('index', canonical(projection))
    else:
        raise SystemExit('expected audit, bundle or verify-zero')
