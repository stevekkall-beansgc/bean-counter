#!/usr/bin/env python3
"""Finite no-append checks on an explicit copy of the supported synthetic fixture.

The fixture must be the quiescent maintained main case before root reversal.
Only the copied installation is opened; no SQL writes or accepted terms change.
"""
import argparse, copy, json, shutil
from pathlib import Path
from workflow_supported import Journey, canonical, now, reference, ATOMS


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--ledger', type=Path, required=True)
    p.add_argument('--fixture', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--resume', action='store_true')
    p.add_argument('--retry-refusals', action='store_true', help='explicitly rerun selected saved refusals after a reviewed fix')
    p.add_argument('--groups', nargs='+', choices=['N1','N2','N3','N4','N6','N7','N8','N9'], default=['N1','N2','N3','N6','N9'])
    a = p.parse_args()
    if a.output.exists() and not a.resume:
        p.error('output exists; explicit --resume required')
    a.output.mkdir(parents=True, exist_ok=True)
    if not (a.output / 'installation').exists():
        shutil.copytree(a.fixture / 'installation', a.output / 'installation', copy_function=shutil.copy2)
        shutil.copytree(a.fixture / 'inputs', a.output / 'inputs', copy_function=shutil.copy2)
    a.resume = True
    a.publication_helper = a.terms_helper = a.ledger
    j = Journey(a)
    load = lambda name: json.loads((a.fixture / (name + '.request.json')).read_bytes())
    original = load('main-work')
    j.store, j.scope, j.source = (original['key'][k] for k in ['logical_store_id', 'scope', 'source'])
    j.binding_ref = original['payload']['binding_ref']
    j.meta['fixture'] = str(a.fixture.resolve())
    j.meta['fixture_operation_key'] = original['key']
    j.save()

    fixture_installation, j.installation = j.installation, a.fixture / 'installation'
    baseline = j.snapshot()
    j.installation = fixture_installation
    current = j.snapshot()
    if a.resume and a.groups == ['N4']:
        assert all(row in current['operations'] for row in baseline['operations'])
        assert all(row in current['documents'] for row in baseline['documents'])
        assert current['heads'] == baseline['heads']
    else:
        assert current == baseline, 'fixture copy already changed before finite refusals'

    def refusal(name, request, codes):
        before = j.snapshot()
        saved = j.inputs / (name + '.request.json')
        if saved.exists():
            request = json.loads(saved.read_bytes())
        path = j.write(name + '.request.json', request)
        previous = [c for c in j.meta['commands'] if c['name'] == name]
        if previous and not a.retry_refusals:
            response = json.loads((j.root / previous[-1]['stdout']).read_bytes())
        else:
            expected = 4 if codes == ['IDENTITY_CONFLICT'] else 3
            response = j.workflow(name, 'submit', path, expected=expected)
        assert response['code'] in codes, (name, response, codes)
        assert j.snapshot() == before, name + ': refusal mutated workflow state'
        j.meta['checks'].append({'check': name, 'code': response['code'], 'no_append': True})
        j.save()

    def fresh(name, request):
        request = copy.deepcopy(request)
        request['key']['operation_id'] = name
        return request

    if 'N1' in a.groups:
        # N1: stale actual outcome revision, with unchanged original permanent claim.
        r = fresh('negative-stale-outcome', load('main-none'))
        r['payload']['expected_revision'] = '999'
        refusal('N1-stale-outcome', r, ['STALE_REVISION'])

    if 'N2' in a.groups:
        # N2: original operation ID with a different normalized quantity.
        r = copy.deepcopy(original)
        r['payload']['facts']['support:quantity'] = '19'
        refusal('N2-changed-original-intent', r, ['IDENTITY_CONFLICT'])

    if 'N3' in a.groups:
        # N3: genuine retained code/occurrence assertions exchanged between fields.
        r = fresh('negative-wrong-fact-field', load('main-discount'))
        r['payload']['expected_revision'] = '3'
        proofs = r['payload']['replacement']['fact_proofs']
        proofs[0]['attestation_ref'], proofs[1]['attestation_ref'] = proofs[1]['attestation_ref'], proofs[0]['attestation_ref']
        refusal('N3-wrong-fact-association', r, ['INTEGRITY_FAILURE', 'AUTHORITY_REQUIRED'])

    if 'N6' in a.groups:
        # N6: same actual app subject/observation, next revision and prior evidence,
        # but final amount4 exceeds the original accepted maximum3.
        r = fresh('negative-app-over-maximum', load('main-final-evidence-2'))
        r['payload']['attestation'].update({'attestation_id': 'negative-app-over-maximum', 'cost_revision': '3',
            'prior_final_evidence_ref': load('main-cost-correct')['payload']['replacement_final_cost_proof']['attestation_ref'],
            'amount': {'currency': 'USD', 'scale': 18, 'atoms': str(4 * ATOMS)}, 'issued_at': now()})
        refusal('N6-original-app-maximum', r, ['EXPOSURE_EXCEEDED'])

    if 'N7' in a.groups:
        r = fresh('negative-terminal-outcome', load('main-none'))
        claim = r['payload']['original_claim_ref']
        with j.connection() as db:
            heads = db.execute("SELECT revision,record_kind,record_id,record_digest FROM workflow_financial_heads WHERE head_kind='decision'").fetchall()
        matches = [(rev, {'kind':kind,'id':id,'digest':digest}) for rev,kind,id,digest in heads
                   if j.document({'kind':kind,'id':id,'digest':digest}).get('subject_ref') == claim]
        assert len(matches) == 1
        r['payload']['expected_revision'] = str(matches[0][0])
        refusal('N7-terminal-outcome-correction', r, ['TARGET_CLOSED', 'AUTHORITY_REQUIRED'])

    if 'N8' in a.groups:
        # The exact new restricted compiler gate requires this existing clause.
        # Its absence is valid dictionary syntax but unsupported before promise.
        r = fresh('negative-unsupported-policy', load('publication'))
        policy = next(d for d in r['payload']['documents'] if d['schema'] == 'ledger-workflow-policy/4')
        del policy['families'][0]['no_success_close']
        r['payload']['root_policy_ref'] = reference('workflow-policy', policy)
        refusal('N8-unsupported-new-publication', r, ['UNSUPPORTED_COMPOSITION'])

    if 'N9' in a.groups:
        # N9: a finite attempt quota refuses before effects, then trusted settings
        # remain usable to reset it. Retained-growth quota below usage permits reads.
        resources = json.loads((a.fixture / 'inputs/resources.json').read_bytes())
        finite = {**resources, 'semantic_work': 1}
        j.workflow('N9-set-finite', 'resources', 'set', j.write('finite-resources.json', finite))
        r = fresh('negative-finite-work', original)
        r['payload'].update({'external_event_id': 'negative-finite-event', 'business_operation_id': 'negative-finite-service',
                              'chain': 'negative-finite-chain', 'occurred_at': now()})
        before = j.snapshot()
        response = j.workflow('N9-attempt-refusal', 'submit', j.write('finite-work.request.json', r), expected=3)
        assert response['code'] == 'PROGRAM_BUDGET_EXCEEDED' and 'semantic_work' in response['message'], response
        assert j.snapshot() == before
        j.workflow('N9-reset-all-null', 'resources', 'set', j.write('all-null-resources.json', resources))
        j.workflow('N9-show-reset', 'resources', 'show')
        low_storage = {**resources, 'retained_operations': 1, 'retained_bytes': 1}
        j.workflow('N9-lower-retained', 'resources', 'set', j.write('low-storage-resources.json', low_storage))
        before = j.snapshot()
        status = j.workflow('N9-original-status-below-quota', 'status', j.write('original.key.json', original['key']))
        retry = j.workflow('N9-original-retry-below-quota', 'submit', j.write('original.request.json', original))
        assert status['receipt'] == retry['receipt'] and j.snapshot() == before
        j.workflow('N9-reset-storage', 'resources', 'set', j.write('all-null-resources.json', resources))
        j.meta['checks'].append({'check': 'N9-quota-reset-and-zero-growth-original-read', 'no_append': True})
    if 'N4' in a.groups:
        # Retain a genuine, unconsumed next-revision proof while current rights
        # still permit it, then test the financial consumer after restriction.
        att = copy.deepcopy(load('main-final-evidence-2')['payload']['attestation'])
        att.update({'attestation_id': 'negative-authority-final-evidence-3', 'cost_revision': '3',
                    'prior_final_evidence_ref': load('main-cost-correct')['payload']['replacement_final_cost_proof']['attestation_ref'],
                    'issued_at': now()})
        retained = j.submit('negative-authority-final-evidence-3', 'evidence.retain',
                            {'schema': 'ledger-workflow-app-cost-attest/1', 'attestation': att})
        from workflow_supported import proof
        valid_replacement = proof(retained)
        control_installation = j.root / 'unrestricted-control-installation'
        if not control_installation.exists():
            shutil.copytree(j.installation, control_installation, copy_function=shutil.copy2)
        binding = j.document(j.binding_ref)
        grant_ref = next(r for r in binding['authority_refs'] if r['kind'] == 'workflow-source-grant')
        enrollment_ref = j.document(grant_ref)['original_ceiling_ref']
        enrollment = j.document(enrollment_ref)
        narrowed = copy.deepcopy(enrollment['ceiling'])
        narrowed['permissions'].remove('correct')
        narrowed['operations'].remove('cost.correct')
        j.submit('negative-source-restrict', 'authority.change', {
            'schema': 'ledger-workflow-source-restrict/1', 'action': 'source.restrict',
            'enrollment_ref': enrollment_ref, 'expected_revision': '1', 'replacement_ceiling': narrowed})
        r = fresh('negative-current-correct-denied', load('main-cost-correct'))
        r['payload']['expected_revision'] = '2'
        r['payload']['replacement_final_cost_proof'] = valid_replacement
        request = j.write('N4-current-correct-denied.request.json', r)
        restricted_installation = j.installation
        j.installation = control_installation
        control_before = j.snapshot()
        previous = [c for c in j.meta['commands'] if c['name'] == 'N4-unrestricted-control']
        if previous:
            control = json.loads((j.root / previous[-1]['stdout']).read_bytes())
        else:
            control = j.workflow('N4-unrestricted-control', 'submit', request)
            assert len(j.snapshot()['operations']) == len(control_before['operations']) + 1
        assert control['receipt']['key'] == r['key']
        assert any(row[0] == r['key']['operation_id'] for row in j.snapshot()['operations'])
        j.installation = restricted_installation
        refusal('N4-current-correct-denied', r, ['AUTHORITY_REQUIRED', 'SOURCE_CEILING_EXCEEDED', 'INTEGRITY_FAILURE'])
        j.meta['checks'].append({'check': 'N4-exact-intent-differential', 'same_request_and_proof': True,
                                'unrestricted_committed': True, 'restricted_no_append': True,
                                'diagnostic_limitation': 'Restricted refusal may use existing generic INTEGRITY_FAILURE; exact authority-specific classification not claimed'})
        before = j.snapshot()
        status = j.workflow('N4-original-read-retained', 'status', j.write('original.key.json', original['key']))
        assert status.get('receipt') and j.snapshot() == before
        j.meta['checks'].append({'check': 'N4-original-read-retained', 'no_append': True})
    j.meta['status'] = 'finite_negative_subset_complete'
    j.meta['requested_groups'] = a.groups
    j.meta['not_covered'] = [g for g in ['N1','N2','N3','N4','N5','N6','N7','N8','N9'] if g not in a.groups]
    j.save()

if __name__ == '__main__':
    main()
