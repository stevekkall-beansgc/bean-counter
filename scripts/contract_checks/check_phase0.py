"""Supplementary Phase 0 oracles. Never imports production Rust or updates fixtures."""
from datetime import datetime, timezone
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import re
import tomllib

import jsonschema
from referencing import Registry, Resource

from canonical_records import canonical

ROOT = Path(__file__).resolve().parents[2]


def strict(raw):
    text = raw.decode('utf-8')
    if text.startswith('\ufeff'):
        raise ValueError('BOM')

    def pairs(items):
        d = {}
        for k, v in items:
            if k in d:
                raise ValueError('duplicate key')
            d[k] = v
        return d

    def integer(s):
        if s == '-0' or abs(int(s)) > 9007199254740991:
            raise ValueError('integer profile')
        return int(s)

    def number(_):
        raise ValueError('unsupported numeric token')

    value = json.loads(text, object_pairs_hook=pairs, parse_int=integer,
                       parse_float=number, parse_constant=number)

    def scalars(v, depth=0):
        if depth > 32:
            raise ValueError('nesting')
        if isinstance(v, str):
            v.encode('utf-8', errors='strict')
        elif isinstance(v, list):
            for x in v:
                scalars(x, depth + 1)
        elif isinstance(v, dict):
            for k, x in v.items():
                scalars(k, depth + 1)
                scalars(x, depth + 1)
    scalars(value)
    return value


def load(path):
    return strict((ROOT / path).read_bytes())


def decimal(raw):
    if not isinstance(raw, str) or len(raw.encode()) > 64 or not re.fullmatch(r'[0-9]+(?:\.[0-9]{1,18})?', raw):
        raise ValueError('INPUT_PRECISION')
    a, _, b = raw.partition('.')
    a = a.lstrip('0') or '0'
    b = b.rstrip('0')
    if len((a + b).lstrip('0') or '0') > 30:
        raise ValueError('INPUT_PRECISION')
    return a + ('.' + b if b else '')


def authority_oracle(overrides):
    # A deliberately small independent truth table for the frozen scenarios,
    # not an implementation of service authority or concurrency.
    v = {'source': 'urn:demo:app', 'grant_active': True, 'assent_present': True,
         'binding_present': True, 'invocation_present': True, **overrides}
    if not v['grant_active'] or not v['assent_present'] or not v['binding_present']:
        return 'rejected'
    if v.get('required_outcome_source') and v['source'] != v['required_outcome_source']:
        return 'rejected'
    if v['source'] != 'urn:demo:app':
        return 'rejected'
    if v.get('mode') == 'real' and v.get('assent_mode') == 'demo':
        return 'rejected'
    if v.get('bearer') != v.get('payer') and not v.get('delegation_present'):
        return 'rejected'
    if v.get('supplier') and (not v['invocation_present'] or v.get('correction_source_authorized') is False):
        return 'rejected'
    if v.get('binding_active') is False and not v.get('historical_invocation_valid'):
        return 'rejected'
    return 'authority_satisfied' if overrides else 'accepted'


def main():
    schemas = {}
    resources = []
    paths = list((ROOT / 'contracts/schemas/v1').glob('*.json')) + [ROOT / 'release/artifact-manifest.schema.json']
    for path in paths:
        schema = strict(path.read_bytes())
        jsonschema.Draft202012Validator.check_schema(schema)
        assert schema['$id'] not in schemas, 'duplicate schema identifier'
        schemas[schema['$id']] = schema
        resources.append((schema['$id'], Resource.from_contents(schema)))
    registry = Registry().with_resources(resources)
    # Inspect every reference; there is deliberately no network retrieve callback.
    def references(v, base):
        if isinstance(v, dict):
            if '$ref' in v:
                registry.resolver(base).lookup(v['$ref'])
            for x in v.values():
                references(x, base)
        elif isinstance(v, list):
            for x in v:
                references(x, base)
    for urn, schema in schemas.items():
        references(schema, urn)

    def validate(name, value):
        urn = f'urn:ledgerlab:{name}:1'
        jsonschema.Draft202012Validator(schemas[urn], registry=registry).validate(value)
    for name in ['offer', 'invocation', 'payer-delegation']:
        validate(name, load(f'fixtures/authority/{name}-shape.json'))
    for row in (ROOT / 'fixtures/journals/first-slice/seed-documents.jsonl').read_bytes().splitlines():
        doc = strict(row)
        validate(doc['document_type'], doc['body'])
        if doc['document_type'] != 'policy':
            validate('terms', doc['body'])

    cases = load('fixtures/canonical/cases.json')
    rejected = 0
    def must_reject(fn):
        nonlocal rejected
        try:
            fn()
        except (ValueError, UnicodeError, jsonschema.ValidationError, AssertionError):
            rejected += 1
        else:
            raise AssertionError('negative fixture accepted')
    for name in cases['invalid']:
        must_reject(lambda name=name: strict((ROOT / 'fixtures/canonical/invalid' / name).read_bytes()))
    for c in cases['schema_invalid']:
        must_reject(lambda c=c: validate('event', c['event']))
    event = load('fixtures/canonical/valid/first-slice-input.json')
    validate('event', event)
    equiv = load('fixtures/canonical/valid/decimal-equivalent.json')
    validate('event', equiv)
    equiv['quantity'] = decimal(equiv['quantity'])
    assert equiv == event
    event.update(status='succeeded', unit='call', links=[], evidence=[], extensions={})
    assert canonical(event).encode() == (ROOT / 'fixtures/canonical/expected/first-slice.json').read_bytes()
    unicode = load('fixtures/canonical/valid/unicode-order.json')
    assert canonical(unicode).encode() == (ROOT / 'fixtures/canonical/expected/unicode-order.json').read_bytes()
    for invalid in ['-0', '-1', '+1', '1e1', ' 1', '1.0000000000000000000', '9' * 31, '0' * 65]:
        must_reject(lambda invalid=invalid: decimal(invalid))
    assert decimal('000.000') == '0'
    stamp = datetime.fromisoformat('2026-09-20T10:00:00-04:00').astimezone(timezone.utc)
    assert stamp.isoformat(timespec='microseconds').replace('+00:00', 'Z') == '2026-09-20T14:00:00.000000Z'
    # Internal IDs obey the prose prefix constraint even where the structural
    # repair schema shares the external-ID definition.
    rows = [strict(line) for line in (ROOT / 'fixtures/journals/first-slice/accepted-records.jsonl').read_bytes().splitlines()]
    def internal_ids(v):
        if isinstance(v, dict):
            for k, x in v.items():
                if k in {'event_id', 'canonical_event_id'}:
                    assert re.fullmatch(r'ev_[0-9a-f]{64}', x)
                if k == 'sources':
                    assert all(re.fullmatch(r'ev_[0-9a-f]{64}', eid) for eid in x)
                internal_ids(x)
        elif isinstance(v, list):
            for x in v:
                internal_ids(x)
    for row in rows:
        internal_ids(row)
    must_reject(lambda: internal_ids({'event_id': 'external-name'}))

    economic_count = 0
    for path in (ROOT / 'fixtures/journals').glob('*/economics.json'):
        v = strict(path.read_bytes())
        totals = {k: 0 for k in v['expected_book_totals']}
        for posting in v['postings']:
            totals[posting['book']] += int(posting['atoms'])
            assert set(posting['roles']) == {'provider', 'cost_originator', 'bearer', 'payer', 'beneficiary', 'recipient'}
            if posting['component'].endswith('.discount') and path.parent.name != 'reversal':
                rate = Fraction(20, 100) if path.parent.name == 'first-party' else Fraction(10, 100)
                assert int(posting['atoms']) == -Fraction(posting['basis_atoms']) * rate
        assert totals == {k: int(n) for k, n in v['expected_book_totals'].items()}
        if 'cap_atoms' in v:
            credit = -max(0, int(v['prior_net_atoms']) + int(v['closure_atoms']) - int(v['cap_atoms']))
            assert credit == int(v['cap_credit_atoms'])
            assert Fraction(int(v['closure_atoms']) + credit) * Fraction(v['share_percent']) / 100 == int(v['share_atoms'])
            assert totals['retail'] - totals['supplier'] - totals['cost_observation'] == int(v['margin_atoms'])
        if path.parent.name == 'reversal':
            assert [int(n) for n in v['reversal_atoms']] == [-int(n) for n in v['original_atoms']]
            assert not v['replenishes_reservation'] and not v['reopens_closed_stage']
        economic_count += 1
    assert economic_count == 5
    math_vectors = load('fixtures/math/vectors.json')
    checked_math = load('work/validation/document-checks.json')['arithmetic_vectors']
    assert {v['name']: v['expected'] for v in math_vectors['vectors']} == checked_math
    authorities = load('fixtures/authority/cases.json')['cases']
    assert len({v['name'] for v in authorities}) == len(authorities)
    for c in authorities:
        assert authority_oracle(c['overrides']) == c['expected'], c['name']
        if c['expected'] == 'rejected':
            assert c['accepted_row_delta'] == 0

    schedule = load('fixtures/failures/first-slice.json')
    assert sum(w['items'] for w in schedule['writes']) == 27  # 25 immutable + mutable head/delivery
    assert all(w['failpoints'] == ['before_each', 'after_each'] for w in schedule['writes'])
    for source in load('docs/design/source-digests.json'):
        assert hashlib.sha256((ROOT / source['path']).read_bytes()).hexdigest() == source['sha256']
    frozen = load('contracts/freeze.json')
    for path, digest in frozen['files'].items():
        assert hashlib.sha256((ROOT / path).read_bytes()).hexdigest() == digest, 'frozen contract drift: ' + path
    present = {str(p.relative_to(ROOT)) for folder in ['contracts/schemas', 'fixtures'] for p in (ROOT / folder).rglob('*') if p.is_file()}
    assert present <= set(frozen['files']), 'unfrozen schema/fixture added'
    targets = tomllib.loads((ROOT / 'release/targets.toml').read_text())
    assert len(targets['native']) == 3 and len(targets['build_only']) == 1
    assert targets['oci']['platforms'] == ['linux/amd64', 'linux/arm64']
    assert not targets['certified']
    compatibility = load('contracts/compatibility.json')
    assert compatibility['msrv'] is None and not compatibility['implemented_storage_write_versions']
    assert len(list((ROOT / 'docs/adr').glob('[0-9]*.md'))) == 21
    result = {'status': 'passed', 'schemas': len(schemas), 'additional_negative_checks': rejected,
              'economic_journals': economic_count, 'authority_scenarios': len(authorities),
              'arithmetic_groups': len(math_vectors['vectors']),
              'write_boundaries': 27, 'before_after_failpoint_positions': 54,
              'frozen_files': len(frozen['files']), 'production_behavior_tested': False}
    (ROOT / 'work/validation').mkdir(parents=True, exist_ok=True)
    (ROOT / 'work/validation/phase0-checks.json').write_text(json.dumps(result, indent=2) + '\n')
    print('PASS: ' + json.dumps(result))


if __name__ == '__main__':
    main()
