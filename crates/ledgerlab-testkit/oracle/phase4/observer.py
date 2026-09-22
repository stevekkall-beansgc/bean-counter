"""Fail-closed adapter assertions; unconnected until integration supplies evidence.

No SQL access here. Full typed row inventories must be independently collected
from actual source stores. Engine physical layout/WAL is not application state.
"""
import copy
from reference import candidate_digest, digest

COMMON = set('installation documents parties source_grants bindings authority_heads binding_heads chains events snapshots delivery_keys claims effects actions action_sources action_dependencies explanations intentions control_transitions chain_revisions decision_manifests accepted_receipts delivery_state dispatcher_head dispatch_attempts delivery_observations reconciliation_reports delivery_quarantines outcome_records outcome_heads outcome_deliveries outcome_members outcome_anchors'.split())
PG_ONLY = set('migration_history outcome_scope_locks acceptance_delivery_namespace outcome_held_intentions'.split())
HEADS = ['Admission','Authority','Binding','Reservation','Target','Claim','BindingAggregate','InvocationConsumption','BaseReversal']


def no_change(before, after, reopened, backend, attempts):
    assert backend in ('sqlite','postgres17','postgres18')
    required = COMMON | (PG_ONLY if backend != 'sqlite' else set())
    for snapshot in (before, after, reopened):
        assert set(snapshot) == {'tables','schema','controls','destination'}
        assert set(snapshot['tables']) == required, 'missing/unexpected table'
        assert snapshot['schema'], 'schema inventory missing'
        assert snapshot['controls'], 'migration/control values missing'
        for table in snapshot['tables'].values():
            assert set(table) == {'columns','rows'}
            assert table['columns'], 'column inventory missing'
            assert len(set(table['columns'])) == len(table['columns'])
            for row in table['rows']:
                assert len(row) == len(table['columns']), 'column omitted'
                for cell in row:
                    assert isinstance(cell, list) and len(cell) == 2
                    assert cell[0] in ('null','integer','text','blob','boolean'), 'untyped value'
    assert before == after == reopened, 'authoritative state changed'
    assert attempts == {'writes':0,'network':0,'dispatch':0,'forbidden_reads':0}, 'forbidden attempt'


def assert_result(actual, source, candidate, numeric):
    expected_top = {'committed','mode','source_digest','candidate_digest','source_revision',
                    'observation','historical_receipt_refs','status','steps','latest'}
    assert set(actual) == expected_top
    assert actual['committed'] is False and actual['mode'] == 'hypothetical'
    assert actual['status'] == 'complete'
    assert actual['source_digest'] == digest('source',source)
    assert actual['candidate_digest'] == candidate_digest(candidate)
    assert actual['source_revision'] == source['revision']
    assert actual['observation'] == source['observation']
    assert actual['historical_receipt_refs'] == source['receipt_refs']
    assert len(actual['steps']) == len(source['steps'])
    live = {}
    for index, (row, step) in enumerate(zip(actual['steps'], source['steps'])):
        for field in ('delta','retail','supplier'):
            assert row[field] == numeric[field][index], (index,field)
        assert row['step'] == step['id'] and row['kind'] == step['kind']
        assert row['source_capacity'] == step['capacity']
        if step['kind'] != 'close':
            family = step['family']; info = source['terms']['families'][family]
            assert row['family'] == family
            assert row['code'] == step.get('code','reverse')
            for field in ('book','binding','roles'): assert row[field] == info[field]
            assert row['basis_name'] == 'original_final_booked_retail_net'
            assert row['basis'] == '8000'
            assert row['replacement'] == numeric['replacement'][index]
            assert row['inverse'] == numeric['inverse'][index]
            # All principal fixture components are exact integral atoms. Fractional
            # half-boundary cases are independently literal in test_reference.py.
            assert row['rational'] == [numeric['replacement'][index],'1']
            assert row['reason'] == ('CLAIM_REVERSED' if step['kind']=='reverse' else ('ZERO_ROUNDED' if row['replacement']=='0' else 'OUTCOME_APPLIED'))
            assert row['inverse_reason'] == ('NOT_APPLICABLE' if step['kind']=='ordinary' else 'EXACT_REVERSAL')
            assert row['nonzero_components'] == [v for v in [row['inverse'],row['replacement']] if v!='0']
            live[family] = row['replacement']
        assert row['live'] == live
        common = {'step','kind','delta','retail','supplier','live','source_capacity'}
        extra = {'family','code','book','binding','roles','basis_name','basis','rational','inverse','replacement','reason','inverse_reason','nonzero_components'}
        assert set(row) == (common if step['kind']=='close' else common|extra)
    assert actual['latest'] == live


def run_adapter(adapter, source, candidates, expected):
    """No expected values are ever passed into adapter.compare().

    Adapter must own trusted scoped reads, anchor verification and complete
    provenance extraction. This seam cannot certify a caller-supplied may_read.
    """
    before = copy.deepcopy(adapter.inventory())
    observed = adapter.compare(copy.deepcopy(candidates))
    after = copy.deepcopy(adapter.inventory())
    adapter.reopen()
    no_change(before, after, adapter.inventory(), adapter.backend, adapter.attempts())
    assert len(observed) == len(candidates)
    for actual, candidate in zip(observed,candidates):
        assert_result(actual,source,candidate,expected[candidate['label']])
