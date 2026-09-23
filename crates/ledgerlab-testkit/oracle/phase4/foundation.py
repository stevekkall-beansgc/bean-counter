"""Independent supplier-foundation observation checker, not full Phase 4.

Consumes actual adapter observations; never produces acceptance commands or
uses an implementation evaluator to manufacture expected economics.
"""
import json
from observer import COMMON, PG_ONLY

R3_SQLITE = {
    'r3_journals', 'r3_segments', 'r3_segment_pages', 'r3_objects',
    'r3_object_pages', 'r3_heads', 'r3_head_versions', 'r3_commands',
    'r3_namespaces', 'r3_deliveries', 'r3_index_pages', 'r3_index_roots',
    'r3_held_intentions', 'r3_storage_profile',
}


def stable(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=True, separators=(',', ':'), allow_nan=False)


def inventory(snapshot, backend):
    """Accept the existing backend observers' full column/row inventories.

    This checks the application inventory; schema/control metadata must also
    be supplied separately by the harness for complete no-change evidence.
    Opaque database-produced row strings are compared byte-for-byte, not parsed
    as monetary summaries. Backend-specific table sets are never equated.
    """
    assert backend in ('sqlite', 'postgres17', 'postgres18')
    if backend == 'sqlite':
        assert type(snapshot) is list
        assert all(type(row) is list and len(row) == 3 for row in snapshot)
        assert len({row[0] for row in snapshot}) == len(snapshot), 'duplicate table'
        tables = {name: {'columns': columns, 'rows': rows} for name, columns, rows in snapshot}
        assert tables['sqlite_schema']['rows'], 'schema inventory missing'
        assert tables['user_version']['columns'] == ['value']
        version = tables['user_version']['rows']
        assert len(version) == 1 and re.fullmatch(r'0|[1-9][0-9]*',version[0])
        application = COMMON | (R3_SQLITE if version == ['5'] else set())
        assert set(tables) in (application | {'sqlite_schema','user_version'}, application | {'sqlite_schema','user_version','_sqlx_migrations'})
    else:
        assert type(snapshot) is dict and set(snapshot) == COMMON | PG_ONLY
        tables = snapshot
    for name, table in tables.items():
        assert type(table) is dict and set(table) == {'columns', 'rows'}
        assert type(table['columns']) is list and type(table['rows']) is list
        if name not in ('sqlite_schema','user_version'):
            assert table['columns'], 'missing columns'
            assert len(set(map(stable, table['columns']))) == len(table['columns'])
        assert all(type(row) is str for row in table['rows']), 'row encoding changed'
    return stable(snapshot)


def no_change(evidence):
    backend = evidence['backend']
    assert inventory(evidence['B0'], backend) == inventory(evidence['B1'], backend) == inventory(evidence['B2'], backend), 'source rows changed'
    assert evidence['attempted_writes'] == 0 and type(evidence['attempted_writes']) is int
    assert evidence['external_calls'] == 0 and type(evidence['external_calls']) is int
    # Separate exact metadata accommodates existing observer representations.
    metadata = evidence['metadata']
    assert set(metadata) == {'B0', 'B1', 'B2'}
    assert stable(metadata['B0']) == stable(metadata['B1']) == stable(metadata['B2']), 'source metadata changed'
    required = {'user_version', 'schema'} if backend == 'sqlite' else {'schema'}
    for value in metadata.values():
        assert type(value) is dict and set(value) == required
        assert value['schema'], 'schema inventory missing'
        if backend == 'sqlite':
            version = value['user_version']
            assert type(version) is list and len(version) == 1 and type(version[0]) is str
            assert re.fullmatch(r'0|[1-9][0-9]*',version[0])
            assert version == next(r[2] for r in evidence['B0'] if r[0]=='user_version')
            assert value['schema'] == next(r[2] for r in evidence['B0'] if r[0]=='sqlite_schema')
        else:
            schema = value['schema']
            assert type(schema) is dict and set(schema) == {'indexes','constraints','triggers'}
            assert schema['indexes'] and schema['constraints'], 'incomplete schema inventory'
            for kind,width in [('indexes',2),('constraints',3),('triggers',3)]:
                assert type(schema[kind]) is list
                assert all(type(row) is list and len(row)==width and all(type(cell) is str for cell in row) for row in schema[kind])

# Literal economics live outside executable checks and are never generated from
# the report. No prior nine-step retail story participates in this foundation.
import hashlib
import re
import sys
from fractions import Fraction
from pathlib import Path

EXPECTED = json.loads(Path(__file__).with_name('foundation-expected.json').read_text())
ASSUMPTIONS = 'Fixed booked final base, original parties, currency/scale, membership, evidence, windows, limits, capacity, authority assumptions and complete activity. Amounts substituted for comparison; no assent, eligibility change or posting authorized. No behavioral forecast.'


def wire(value):
    # All descriptor keys are fixed ASCII. UTF-8 values retain their exact bytes.
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(',', ':'), allow_nan=False).encode('utf-8')


def private_digest(domain, payload):
    return hashlib.sha256(b'ledgerlab-comparison/1\0' + len(domain).to_bytes(8, 'big') + domain.encode() + len(payload).to_bytes(8, 'big') + payload).hexdigest()


def amount_key(row):
    return row['agreement'], row['family'], row['code']


def amounts(rows):
    assert type(rows) is list and len(rows) == 5
    result = {}
    for row in rows:
        assert set(row) == {'agreement', 'family', 'code', 'amount'}
        key = amount_key(row)
        assert key not in result
        value = row['amount']
        assert type(value) is dict
        if set(value) == {'fixed'}:
            m = value['fixed']
            assert set(m) == {'currency','scale','atoms'} and m['currency'] == 'USD' and type(m['scale']) is int and m['scale'] == 2
            assert type(m['atoms']) is str and re.fullmatch(r'0|-?[1-9][0-9]*',m['atoms'])
            assert abs(int(m['atoms'])) <= 10**30-1
        else:
            assert set(value) == {'percent'}
            p = value['percent']; assert set(p) == {'numerator','denominator'}
            assert all(type(x) is str for x in p.values())
            assert re.fullmatch(r'0|-?[1-9][0-9]*',p['numerator']) and re.fullmatch(r'[1-9][0-9]*',p['denominator'])
            ratio = Fraction(int(p['numerator']),int(p['denominator']))
            assert [str(ratio.numerator),str(ratio.denominator)] == [p['numerator'],p['denominator']]
            assert max(abs(ratio.numerator).bit_length(),ratio.denominator.bit_length()) <= 512
        result[key] = value
    original = {amount_key(row):row['amount'] for row in EXPECTED['original_amounts']}
    assert set(result) == set(original), 'changed family/code membership'
    return result


def candidate_fingerprint(candidate, provenance):
    rows = [[*amount_key(row),row['amount']] for row in sorted(candidate['amounts'],key=amount_key)]
    payload = wire([provenance['snapshot'],provenance['activity'],provenance['semantics']]) + wire(rows)
    return private_digest('candidate',payload)


def report(value):
    assert set(value) == {'milestone','committed','provenance','basis','historical_receipt_refs','bindings','original','candidates'}
    assert value['milestone'] == 'PHASE-4 FOUNDATION ONLY' and value['committed'] is False
    assert stable(value['basis']) == stable(EXPECTED['basis'])
    assert stable(sorted(value['bindings'],key=lambda b:b['binding'])) == stable(EXPECTED['bindings'])
    candidates = value['candidates']
    assert type(candidates) is list and len(candidates) == 2
    provenance = value['provenance']
    assert set(provenance) == {'semantics','snapshot','activity','draft_candidates','source_build','assumptions','committed','report_provenance'}
    assert provenance['semantics'] == 'ledgerlab-private-comparison/1'
    assert provenance['committed'] is False and provenance['assumptions'] == ASSUMPTIONS
    assert type(provenance['source_build']) is str and provenance['source_build'].endswith('/supplier-foundation-1')
    for key in ('snapshot','activity','report_provenance'):
        assert type(provenance[key]) is str and re.fullmatch('[0-9a-f]{64}',provenance[key])
    assert provenance['draft_candidates'] == [c['fingerprint'] for c in candidates]
    descriptor = {k:v for k,v in provenance.items() if k != 'report_provenance'}
    assert provenance['report_provenance'] == private_digest('report',wire(descriptor))
    count = len(value['original']['result']['steps'])
    assert 0 <= count <= 3
    refs = value['historical_receipt_refs']
    assert type(refs) is list and len(refs) == count + 1
    assert all(type(pair) is list and len(pair)==2 for pair in refs)
    assert len({pair[0] for pair in refs}) == len(refs)
    assert all(type(pair[0]) is str and pair[0] for pair in refs)
    economic = [pair[1] for pair in refs if pair[1] is not None]
    assert len(economic) == 1 + min(count,2) and len(set(economic)) == len(economic)
    assert all(type(ref) is str and ref for ref in economic)
    original = amounts(value['original']['amounts'])
    assert original == amounts(EXPECTED['original_amounts'])
    seen = []
    for index, candidate in enumerate([value['original'],*candidates]):
        assert set(candidate) == {'key','fingerprint','amounts','result'}
        assert type(candidate['key']) is str and 0 < len(candidate['key'].encode()) <= 128
        actual = amounts(candidate['amounts'])
        assert candidate['fingerprint'] == candidate_fingerprint(candidate,provenance)
        assert all(actual[k] == v for k,v in original.items() if k[0] == 'agreement-supplier'), 'changed supplier terms'
        if index:
            assert actual != original, 'candidate does not vary unobserved retail terms'
            assert actual not in seen, 'duplicate alternatives'
            seen.append(actual)
        result = candidate['result']
        assert set(result) == {'status','steps','latest'} and result['status']=='complete'
        assert stable(result['steps']) == stable(EXPECTED['steps'][:count]), 'step economics or provenance differs'
        latest = result['latest']
        assert set(latest) == {'bindings','families','projected_reservations'}
        normalized = {'bindings':sorted(latest['bindings'],key=lambda b:b['binding']),
                      'families':sorted(latest['families'],key=lambda f:(f['agreement'],f['family'])),
                      'projected_reservations':sorted(latest['projected_reservations'],key=lambda r:r['binding'])}
        assert stable(normalized) == stable(EXPECTED['latest_by_prefix'][count]), 'latest values differ'
    return count


def check(value, with_evidence=False):
    if with_evidence:
        no_change(value)
        return report(value['report'])
    return report(value)


def load(raw):
    def pairs(items):
        result = {}
        for key,value in items:
            assert key not in result, 'duplicate JSON key'
            result[key] = value
        return result
    def invalid(_):
        raise ValueError('nonfinite/fractional JSON number')
    return json.loads(raw,object_pairs_hook=pairs,parse_constant=invalid,parse_float=invalid)


if __name__ == '__main__':
    with_evidence = sys.argv[1:] == ['--evidence']
    assert sys.argv[1:] in ([],['--evidence'])
    count = check(load(sys.stdin.buffer.read()),with_evidence)
    print(json.dumps({'status':'passed','scope':'supplier-foundation-only','steps':count,'scenarios':3,'inventories_checked':with_evidence,'full_phase4':False}))
