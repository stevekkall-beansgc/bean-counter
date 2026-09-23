#!/usr/bin/env python3
"""Synthetic installed-CLI finance example and consumer-side reconciliation.

Uses stdlib CSV/integer arithmetic, never the Rust evaluator for expectations.
No network, customer data, external ingestion or delivery acknowledgment.
"""
import argparse
import csv
import hashlib
import json
from pathlib import Path
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ledger', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path, help='New directory; refuses reuse')
    parser.add_argument('--checks', action='store_true', help='Also run isolated negative and incremental checks')
    args = parser.parse_args()
    binary = args.ledger.resolve(strict=True)
    root = args.output.absolute()
    root.mkdir(mode=0o700)
    root = root.resolve()
    fixtures = Path(__file__).resolve().parents[1] / 'examples' / 'finance'
    inputs = {p.stem: json.loads(p.read_text()) for p in fixtures.glob('*.json')}
    commands = []

    def save(name, data):
        (root / name).write_text(json.dumps(data, indent=2) + '\n')

    def run(*words, expected=0):
        result = subprocess.run([str(binary), 'billing', *words, '--json'], cwd=root,
                                text=True, capture_output=True)
        assert result.returncode == expected, (words, result.returncode, result.stdout, result.stderr)
        value = json.loads(result.stdout)
        commands.append({'args': list(words), 'exit': result.returncode, 'result': value})
        return value

    def billing(*words, expected=0):
        return run('--directory', 'store', *words, expected=expected)

    def statement():
        return billing('statement', '--customer', inputs['setup']['customer'])

    def export(filename, snapshot, mapping='mapping.json', customer=None, expected=0):
        return billing('export-csv', '--customer', customer or inputs['setup']['customer'],
                       '--snapshot', snapshot, '--mapping', mapping, '--output', filename, expected=expected)

    for name, data in inputs.items():
        save(name + '.json', data)
    run('init', 'store', '--setup', 'setup.json')
    # Empty history is a complete zero-row projection, requiring an empty mapping.
    if args.checks:
        save('empty-mapping.json', {'schema': 'ledger-finance-mapping/1', 'accounts': {}})
        empty = statement()
        empty_export = export('empty.csv', empty['snapshot_hash'], 'empty-mapping.json')
        assert empty_export['posting_count'] == '0' and empty_export['net_atoms'] == '0'
    first = billing('accept', 'event.json')
    target = first['receipt']['body']['target']
    assert billing('accept', 'event.json')['receipt'] == first['receipt']
    alias = dict(inputs['event'], id='renamed-delivery')
    save('alias.json', alias)
    assert billing('accept', 'alias.json')['receipt'] == first['receipt']
    second = dict(inputs['event'], id='work-2', operation_id='operation-2')
    save('second.json', second)
    billing('accept', 'second.json')
    unauthorized = dict(inputs['event'], id='unauthorized', operation_id='unauthorized', customer='other-customer')
    save('unauthorized.json', unauthorized)
    billing('accept', 'unauthorized.json', expected=6)
    receipts = {}
    for name, command in [('outcome', 'outcome'), ('correction', 'correct'), ('reversal', 'correct')]:
        data = dict(inputs[name], target=target)
        save(name + '.json', data)
        receipts[name] = billing(command, name + '.json')['receipt']
        assert billing(command, name + '.json')['receipt'] == receipts[name]
    explained = billing('explain', target)
    assert explained['net_atoms'] == '250'
    pinned = statement()
    assert pinned['net_atoms'] == '500' and pinned['cutoff'] == '5'
    save('statement.json', pinned)
    first_export = export('finance.csv', pinned['snapshot_hash'])
    repeated = export('finance-repeat.csv', pinned['snapshot_hash'])
    assert first_export['export_id'] == repeated['export_id']
    assert (root / 'finance.csv').read_bytes() == (root / 'finance-repeat.csv').read_bytes()
    rows = list(csv.DictReader((root / 'finance.csv').open(newline='')))
    assert rows[-1]['row_type'] == 'complete' and all(r['row_type'] == 'posting' for r in rows[:-1])
    postings = rows[:-1]
    # Hand calculation: 250 + 250 - 50 + (50 - 50) + 50 = 500 cents.
    expected_by_decision = {'1': [250], '2': [250], '3': [-50], '4': [-50, 50], '5': [50]}
    for ordinal, amounts in expected_by_decision.items():
        assert sorted(int(r['amount_atoms']) for r in postings if r['decision_ordinal'] == ordinal) == amounts
    assert sum(int(r['amount_atoms']) for r in postings) == 500
    assert rows[-1]['control_net_atoms'] == '500' and rows[-1]['posting_count'] == '6'
    stored = {p['id']: p for e in pinned['entries'] for p in e['postings']}
    assert {r['record_id'] for r in postings} == set(stored)
    for row in postings:
        record = stored[row['record_id']]
        assert row['record_hash'] == record['content_hash']
        assert row['amount_atoms'] == record['body']['amount']['atoms']
        assert row['reverses_record_id'] == record['body'].get('reverses', '')
        assert row['scale'] == '2' and row['currency'] == 'USD'
        if row['reverses_record_id']:
            assert int(row['amount_atoms']) == -int(stored[row['reverses_record_id']]['body']['amount']['atoms'])
    # This explicitly implemented consumer demonstrates deduplication, not a product delivery claim.
    imported = {}
    metadata = {'row_type', 'export_id', 'snapshot_hash', 'cutoff', 'posting_count', 'control_net_atoms'}
    def ingest(new_rows):
        inserted = duplicates = 0
        for row in new_rows:
            if row['row_type'] != 'posting':
                continue
            key = (row['tenant_text'], row['environment_text'], row['record_id'])
            payload = {k: v for k, v in row.items() if k not in metadata}
            if key in imported:
                if imported[key] != payload:
                    raise ValueError('same record ID with changed export payload; manual reconciliation required')
                duplicates += 1
            else:
                imported[key] = payload
                inserted += 1
        return inserted, duplicates
    assert ingest(rows) == (6, 0)
    assert ingest(rows) == (0, 6)
    before = pinned
    if args.checks:
        for mapping in [dict(inputs['mapping'], extra=True), {'schema': 'ledger-finance-mapping/1', 'accounts': {'customer-1': 'missing-merchant'}}, {'schema': 'ledger-finance-mapping/1', 'accounts': {'customer-1': 'a\nb', 'example-company': 'merchant'}}]:
            save('bad-mapping.json', mapping)
            assert export('invalid.csv', pinned['snapshot_hash'], 'bad-mapping.json', expected=3)['complete'] is False
            assert not (root / 'invalid.csv').exists()
        (root / 'bad-mapping.json').write_text('{"schema":"ledger-finance-mapping/1","schema":"ledger-finance-mapping/1","accounts":{}}')
        export('invalid.csv', pinned['snapshot_hash'], 'bad-mapping.json', expected=3)
        export('invalid.csv', 'sha256:' + '0' * 64, expected=3)
        export('invalid.csv', pinned['snapshot_hash'], customer='other-customer', expected=6)
        original_bytes = (root / 'finance.csv').read_bytes()
        assert export('finance.csv', pinned['snapshot_hash'], expected=2)['complete'] is False
        assert (root / 'finance.csv').read_bytes() == original_bytes
        export('absent-parent/export.csv', pinned['snapshot_hash'], expected=2)
        (root / 'output-symlink.csv').symlink_to(root / 'finance.csv')
        export('output-symlink.csv', pinned['snapshot_hash'], expected=2)
        assert (root / 'finance.csv').read_bytes() == original_bytes
        hostile = json.loads(json.dumps(inputs['mapping']))
        hostile['accounts']['customer-1'] = '=HYPERLINK("x,y")'
        save('hostile-mapping.json', hostile)
        changed = export('safe-text.csv', pinned['snapshot_hash'], 'hostile-mapping.json')
        safe_rows = list(csv.DictReader((root / 'safe-text.csv').open(newline='')))
        assert safe_rows[0]['payer_account_text'] == 'text:=HYPERLINK("x,y")'
        assert changed['export_id'] != first_export['export_id']
        try:
            ingest(safe_rows)
        except ValueError:
            pass
        else:
            raise AssertionError('consumer must detect remapped identities')
        assert statement() == before
        newer = dict(inputs['event'], id='work-3', operation_id='operation-3')
        save('newer.json', newer)
        billing('accept', 'newer.json')
        export('stale.csv', pinned['snapshot_hash'], expected=3)
        assert not (root / 'stale.csv').exists()
        current = statement()
        incremental = export('incremental.csv', current['snapshot_hash'])
        assert incremental['export_id'] != first_export['export_id']
        assert ingest(list(csv.DictReader((root / 'incremental.csv').open(newline='')))) == (1, 6)
        save('revoke.json', {'schema': 'ledger-billing-permissions/1', 'expected_revision': '1', 'permissions': [], 'reason': 'Synthetic access refusal check'})
        billing('permissions', 'revoke.json')
        assert export('denied.csv', current['snapshot_hash'], expected=6)['complete'] is False
        assert not (root / 'denied.csv').exists()
    assert not list(root.glob('.ledger-finance-*.tmp'))
    report = {'synthetic': True, 'independent_acceptance': False, 'export_id': first_export['export_id'],
              'snapshot_hash': pinned['snapshot_hash'], 'cutoff': '5', 'posting_rows': 6, 'net_atoms': '500',
              'currency': 'USD', 'scale': 2, 'same_input_repeat_identical': True,
              'consumer_repeat_duplicates': 6, 'extended_checks': args.checks,
              'csv_sha256': hashlib.sha256((root / 'finance.csv').read_bytes()).hexdigest(),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'expected_amounts_by_decision': expected_by_decision, 'commands': commands}
    save('RESULT.json', report)
    print(json.dumps({k: v for k, v in report.items() if k != 'commands'}, indent=2))


if __name__ == '__main__':
    main()
