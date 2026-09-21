"""Independent bounded history oracle, derived from frozen bytes, never core code."""
import copy
import json
from pathlib import Path
import sys
from oracle import canonical, content, digest, identity, indexed_projections, row_key

ROOT = Path(__file__).resolve().parents[3]
FIX = ROOT / 'fixtures/journals/first-slice'
SCOPE = ['demo', 'sandbox']
def load(name):
    return json.loads((FIX / name).read_bytes())
def lines(name):
    return [json.loads(x) for x in (FIX / name).read_bytes().splitlines()]
def replace(v, mapping):
    if isinstance(v, str): return mapping.get(v, v)
    if isinstance(v, list): return [replace(x, mapping) for x in v]
    if isinstance(v, dict): return {k: replace(x, mapping) for k, x in v.items()}
    return v
def seal(row):
    row['content_hash'] = 'sha256:' + content(row)
    return row
def reidentify_document(row, mapping):
    old = row['id']
    seal(row)
    row['id'] = 'doc_' + row['content_hash'][7:]
    mapping[old] = row['id']

def bundle(zero=False, count=2):
    seed = lines('seed-documents.jsonl') + lines('seed-records.jsonl')
    state = load('preseed-state.json')
    mapping = {}
    if zero:
        policy = next(r for r in seed if r.get('document_type') == 'policy')
        policy['body']['rules'][1]['amount']['percent'] = '100'
        reidentify_document(policy, mapping)
        seed = replace(seed, mapping)
        binding = next(r for r in seed if r.get('document_type') == 'binding')
        reidentify_document(binding, mapping)
        seed = replace(seed, mapping)
        state = replace(state, mapping)
        for r in seed: seal(r)
    accepted = []
    receipts = []
    commands = []
    for revision in range(1, count + 1):
        rows = replace(lines('accepted-records.jsonl'), mapping)
        snap = next(r for r in rows if r['kind'] == 'document')
        snap['body']['documents'].sort(key=canonical)
        reidentify_document(snap, mapping)
        rows = replace(rows, mapping)
        op = 'generation-' + str(revision)
        event = next(r for r in rows if r['kind'] == 'event')
        event_id = identity('event', ['demo', 'sandbox', 'urn:demo:app', op])
        claim_id = identity('claim', [SCOPE, 'urn:demo:app', op, 'completion', 'completion'])
        ids = {event['id']: event_id}
        for r in rows:
            b, k = r['body'], r['kind']
            if k == 'claim': ids[r['id']] = claim_id
            elif k == 'decision-manifest': ids[r['id']] = identity('decision', [event_id])
            elif k == 'receipt': ids[r['id']] = identity('receipt', [event_id])
            elif k == 'explanation': ids[r['id']] = identity(k, [event_id, b['ordinal']])
            elif k == 'snapshot-ref': ids[r['id']] = identity(k, [event_id, b['purpose'], b['document_id']])
            elif k == 'control-transition': ids[r['id']] = identity(k, [SCOPE, 'chain', 'demo-slice', str(revision)])
            elif k == 'effect':
                effect = identity(k, [SCOPE, b['agreement_id'], b['component'], claim_id, 'self', 'original'])
                ids[r['id']] = effect
                ids[b['action_id']] = identity('action', [effect])
        rows = replace(rows, ids)
        event = next(r for r in rows if r['kind'] == 'event')
        event['body']['id'] = event['body']['operation_id'] = op
        commands.append(copy.deepcopy(event['body']))
        for r in rows:
            b, k = r['body'], r['kind']
            if k == 'delivery-key':
                b['external_id'] = op
                b['ingress'] = copy.deepcopy(event['body'])
                b['ingress_hash'] = 'sha256:' + digest('ingress', b['ingress'])
                r['id'][-1] = op
            elif k == 'claim': b['operation_id'] = op
            elif k in ('receipt', 'decision-manifest', 'chain-revision'): b['revision'] = str(revision)
            elif k == 'control-transition':
                b.update(from_revision=str(revision-1), to_revision=str(revision), from_event_count=str(revision-1), to_event_count=str(revision))
            if k == 'chain-revision': r['id'][-1] = str(revision)
            if k == 'action' and zero and b['kind'] == 'discount': b['amount']['atoms'] = '-100'
            if k == 'explanation' and zero and b['code'] == 'DISCOUNT_APPLIED':
                b['rounded_atoms'] = '-100'
                b['unrounded_atoms']['numerator'] = '-100'
                for item in b['inputs']:
                    if item['name'] == 'percent':
                        item['value'] = '100'
                        item['exact']['numerator'] = '100'
        for r in rows:
            if r['kind'] == 'effect':
                which = 1 if r['body']['component'] == 'generation.base' else 2
                facts = replace(load('effect-facts-'+str(which)+'.json'), ids)
                if zero and which == 2: facts['amount']['atoms'] = '-100'
                r['body']['facts_hash'] = 'sha256:' + digest('effect-facts', facts)
        actions = sorted([r['id'] for r in rows if r['kind'] == 'action'])
        intention = next(r for r in rows if r['kind'] == 'intention')
        new_intention = identity('intention', [SCOPE, 'fake', intention['body']['obligation_id'], actions])
        rows = replace(rows, {intention['id']: new_intention})
        for r in rows:
            if r['kind'] == 'intention':
                r['body']['action_ids'] = actions
                r['body']['payload']['actions'].sort(key=canonical)
        if zero: rows = [r for r in rows if r['kind'] != 'intention']
        for r in rows:
            if r['kind'] == 'explanation': r['body']['input_refs'].sort(key=canonical)
            seal(r)
        manifest = next(r for r in rows if r['kind'] == 'decision-manifest')
        members = sorted([r for r in rows if r['kind'] not in ('decision-manifest', 'receipt')] + [r for r in seed if r['kind']=='document'], key=row_key)
        manifest['body']['members'] = [{k:r[k] for k in ('kind','id','content_hash')} for r in members]
        seal(manifest)
        receipt = next(r for r in rows if r['kind'] == 'receipt')
        receipt['body'].update(action_ids=actions, intention_ids=[] if zero else [new_intention], decision_hash=manifest['content_hash'], content_hash=next(r['content_hash'] for r in rows if r['kind'] == 'event'))
        seal(receipt)
        receipts.append(receipt['body'])
        accepted += rows
    journal = {row_key(r): r for r in seed + accepted}
    journal = [journal[k] for k in sorted(journal)]
    return dict(seed=seed, state=state, journal=journal, indexes=indexed_projections(journal), commands=commands, receipts=receipts)

if __name__ == '__main__':
    # The transformation's identity case must reproduce every frozen byte.
    frozen = bundle(False, 1)
    expected = sorted(lines('seed-documents.jsonl') + lines('seed-records.jsonl') + lines('accepted-records.jsonl'), key=row_key)
    assert canonical(frozen['journal']) == canonical(expected)
    assert canonical(frozen['receipts'][0]) == (FIX / 'receipt.json').read_bytes()
    print(canonical(bundle(sys.argv[1] == 'zero', int(sys.argv[2]))).decode())
