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
    parser.add_argument('--checks', action='store_true', help='Also run isolated refusal, incremental, and outcome-pricing examples')
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

    def scope_args(words, setup):
        words = list(words)
        command_index = words.index('--directory') + 2 if '--directory' in words else 0
        if command_index >= len(words):
            return words
        command = words[command_index]
        if command in ('accept', 'outcome', 'correct', 'agreement', 'permissions'):
            if '--customer' not in words:
                words[command_index + 1:command_index + 1] = [
                    '--customer', setup['customer'], '--source', setup['source']]
        elif command == 'explain' and '--customer' not in words:
            words[command_index + 1:command_index + 1] = ['--customer', setup['customer']]
        return words

    def run(*words, expected=0):
        setup_path = root / 'setup.json'
        setup = json.loads(setup_path.read_text()) if setup_path.exists() else inputs['setup']
        scoped = scope_args(words, setup)
        result = subprocess.run([str(binary), 'billing', *scoped, '--json'], cwd=root,
                                text=True, capture_output=True)
        assert result.returncode == expected, (words, result.returncode, result.stdout, result.stderr)
        value = json.loads(result.stdout)
        commands.append({'args': scoped, 'exit': result.returncode, 'result': value})
        return value

    def run_in(directory, *words, expected=0):
        setup_path = directory / 'setup.json'
        setup = json.loads(setup_path.read_text()) if setup_path.exists() else inputs['setup']
        scoped = scope_args(words, setup)
        result = subprocess.run([str(binary), 'billing', *scoped, '--json'], cwd=directory,
                                text=True, capture_output=True)
        assert result.returncode == expected, (words, result.returncode, result.stdout, result.stderr)
        value = json.loads(result.stdout)
        commands.append({'cwd': str(directory.relative_to(root)), 'args': scoped,
                         'exit': result.returncode, 'result': value})
        return value

    def run_outcome_examples():
        """Use fresh synthetic stores to show each supported outcome balance."""
        outcome_root = root / 'outcome-pricing'
        outcome_root.mkdir(mode=0o700)
        setup = {
            'schema': 'ledger-local-billing/1', 'scope': ['example-company', 'local'],
            'store_id': 'synthetic-outcome-demo', 'operator': 'synthetic-operator',
            'source': 'urn:synthetic:outcome', 'customer': 'synthetic-customer',
            'host': 'synthetic-host', 'agreement': 'synthetic-outcome-agreement',
            'binding': 'synthetic-outcome-binding', 'price': '0.02',
            'accepted_at': '2026-09-22T00:00:00.000000Z',
            'acceptor': 'synthetic-authorized-acceptor',
            'assent_evidence': 'Synthetic demonstration only: retained assent.',
            'operator_attestation': 'Synthetic demonstration only: authority attested.',
            'finality_attestation': 'Successful work submitted under these terms is final for billing.',
            'permissions': ['read', 'submit', 'correct'],
            'outcome_policy': {
                'version': '1', 'families': [{
                    'family': 'delivery', 'binding_id': 'synthetic-outcome-binding',
                    'source': 'urn:synthetic:outcome', 'correction_source': 'urn:synthetic:outcome',
                    'evidence_required': True,
                    'ordinary': {'starts_at': '2026-09-22T13:00:00.000000Z',
                                 'occurs_before': '2027-01-01T00:00:00.000000Z',
                                 'received_by': '2027-01-02T00:00:00.000000Z',
                                 'accepted_by': '2027-01-03T00:00:00.000000Z'},
                    'corrections': {'starts_at': '2026-09-22T13:00:00.000000Z',
                                    'occurs_before': '2027-02-01T00:00:00.000000Z',
                                    'received_by': '2027-02-02T00:00:00.000000Z',
                                    'accepted_by': '2027-02-03T00:00:00.000000Z'},
                    'codes': [
                        {'code': 'success', 'amount': {'kind': 'fixed', 'money':
                         {'currency': 'USD', 'scale': 2, 'atoms': '98'}}},
                        {'code': 'unsuccessful-by-cutoff', 'amount': {'kind': 'fixed', 'money':
                         {'currency': 'USD', 'scale': 2, 'atoms': '-2'}}}],
                    'replacement_codes': ['success', 'unsuccessful-by-cutoff'],
                    'allow_reversal': True}],
                'limits': [{'binding_id': 'synthetic-outcome-binding',
                            'premium': {'currency': 'USD', 'scale': 2, 'atoms': '98'}}]}}
        base_event = {'schema': 'ledger-event/1', 'id': 'work-1', 'operation_id': 'operation-1',
                      'type': 'content.generated', 'customer': 'synthetic-customer',
                      'occurred_at': '2026-09-22T12:00:00.000000Z'}
        outcomes = {
            'success': {'id': 'outcome-success', 'code': 'success'},
            'unsuccessful': {'id': 'outcome-unsuccessful', 'code': 'unsuccessful-by-cutoff'}}
        corrections = {'schema': 'ledger-billing-correction/2',
                       'customer': setup['customer'], 'source': setup['source'],
                       'id': 'correction-success',
                       'family': 'delivery', 'occurred_at': '2026-09-23T14:00:00.000000Z',
                       'evidence': 'Synthetic only: corrected outcome evidence.',
                       'expected_revision': '1', 'replacement':
                       {'kind': 'code', 'code': 'unsuccessful-by-cutoff'}}
        results = {}
        for case in ('no-outcome', 'success', 'unsuccessful', 'success-corrected'):
            case_dir = outcome_root / case
            case_dir.mkdir(mode=0o700)
            case_setup = dict(setup, store_id='synthetic-' + case)
            (case_dir / 'setup.json').write_text(json.dumps(case_setup, indent=2) + '\n')
            event = dict(base_event)
            (case_dir / 'event.json').write_text(json.dumps(event, indent=2) + '\n')
            run_in(case_dir, 'init', 'store', '--setup', 'setup.json')
            accepted = run_in(case_dir, '--directory', 'store', 'accept', 'event.json')
            target = accepted['receipt']['body']['target']
            if case != 'no-outcome':
                name = 'success' if case in ('success', 'success-corrected') else 'unsuccessful'
                data = dict(outcomes[name], schema='ledger-billing-outcome/2',
                            customer=case_setup['customer'], source=case_setup['source'], target=target,
                            family='delivery', occurred_at='2026-09-23T13:00:00.000000Z',
                            evidence='Synthetic only: operator attestation for this example.')
                outcome_file = case_dir / 'outcome.json'
                outcome_file.write_text(json.dumps(data, indent=2) + '\n')
                first_outcome = run_in(case_dir, '--directory', 'store', 'outcome', 'outcome.json')
                if case == 'success':
                    retry = run_in(case_dir, '--directory', 'store', 'outcome', 'outcome.json')
                    assert retry['receipt'] == first_outcome['receipt']
                if case == 'success-corrected':
                    correction = dict(corrections, target=target)
                    (case_dir / 'correction.json').write_text(json.dumps(correction, indent=2) + '\n')
                    run_in(case_dir, '--directory', 'store', 'correct', 'correction.json')
            statement = run_in(case_dir, '--directory', 'store', 'statement',
                               '--customer', 'synthetic-customer')
            amounts = [int(posting['body']['amount']['atoms'])
                       for entry in statement['entries'] for posting in entry.get('postings', [])]
            expected = {'no-outcome': [2], 'success': [2, 98],
                        'unsuccessful': [2, -2], 'success-corrected': [2, 98, -98, -2]}[case]
            assert amounts == expected, (case, amounts)
            assert statement['complete'] is True and statement['cutoff'] == str(len(statement['entries']))
            assert int(statement['net_atoms']) == sum(expected)
            results[case] = {'posting_atoms': amounts, 'net_atoms': statement['net_atoms'],
                             'complete': statement['complete']}
        return results

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
        save('revoke.json', {'schema': 'ledger-billing-permissions/2',
                             'customer': inputs['setup']['customer'],
                             'source': inputs['setup']['source'], 'change_id': 'revoke-all',
                             'expected_revision': '1', 'permissions': [],
                             'reason': 'Synthetic access refusal check'})
        billing('permissions', 'revoke.json')
        assert export('denied.csv', current['snapshot_hash'], expected=6)['complete'] is False
        assert not (root / 'denied.csv').exists()
        outcome_results = run_outcome_examples()
    assert not list(root.glob('.ledger-finance-*.tmp'))
    report = {'synthetic': True, 'independent_acceptance': False, 'export_id': first_export['export_id'],
              'snapshot_hash': pinned['snapshot_hash'], 'cutoff': '5', 'posting_rows': 6, 'net_atoms': '500',
              'currency': 'USD', 'scale': 2, 'same_input_repeat_identical': True,
              'consumer_repeat_duplicates': 6, 'extended_checks': args.checks,
              'csv_sha256': hashlib.sha256((root / 'finance.csv').read_bytes()).hexdigest(),
              'binary_sha256': hashlib.sha256(binary.read_bytes()).hexdigest(),
              'expected_amounts_by_decision': expected_by_decision,
              'outcome_pricing_examples': outcome_results if args.checks else None,
              'commands': commands}
    save('RESULT.json', report)
    print(json.dumps({k: v for k, v in report.items() if k != 'commands'}, indent=2))


if __name__ == '__main__':
    main()
