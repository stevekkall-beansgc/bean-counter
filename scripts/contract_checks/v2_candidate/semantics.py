"""Candidate semantic constraints transcribed from semantic commit 1e0ba3f.

This is a retained-record audit, not an acceptance, pricing or persistence API.
"""
from fractions import Fraction
from profile import canonical, digest, ordered, reference, row_order, strict
from retained import validate_documents, original_hash, event_projection, binding_projection, family_projection

MAX=10**30-1

def bounded(n):
    assert abs(n)<=MAX, 'ARITHMETIC_OVERFLOW'
    return n

def exact_percentage(basis,rate):
    n,d=int(rate['numerator']),int(rate['denominator'])
    # Approved ExactRatio::mul cross-cancels operands before bounding products.
    product=Fraction(basis)*Fraction(n,d)
    assert max(abs(product.numerator).bit_length(),product.denominator.bit_length())<=512, 'MATH_TEMPORARY_BOUND'
    result=product/100
    assert max(abs(result.numerator).bit_length(),result.denominator.bit_length())<=512, 'MATH_BOUND'
    return result

def window(w,occurred,received,accepted):
    assert occurred<=received, 'INVALID_RECEIVED_ORDER'
    assert received<=accepted, 'INVALID_ACCEPTED_ORDER'
    assert w['starts_at']<=occurred<w['occurs_before'], 'OUTCOME_WINDOW'
    assert received<=w['received_by'] and accepted<=w['accepted_by'], 'OUTCOME_DEADLINE'

def base_receipt(b):
    return dict(schema='ledger-base-receipt/2-candidate.3',target=b['target'],base_evaluation=b['base_evaluation'],target_snapshot=b['target_snapshot'],accepted_at=b['accepted_at'],membership_hash=digest('base-membership',b['members']))

def freeze(seed,lookup,deref):
    def only(kind):
        records=[r for r in seed if r['kind']==kind]
        assert len(records)==1, 'BASE_RECORD_CARDINALITY:'+kind
        return records[0]
    aliases=validate_documents(seed)
    acceptance=only('base-acceptance'); ab=acceptance['body']
    target=only('target-snapshot'); tb=target['body']; base=only('base-evaluation'); bb=base['body']
    assert ab['members']==sorted([reference(r) for r in seed if r['kind']!='base-acceptance'],key=row_order), 'BASE_MEMBERSHIP'
    assert ab['target_snapshot']==reference(target) and ab['base_evaluation']==reference(base) and tb['base_evaluation']==reference(base), 'TARGET_FREEZE_REFERENCE'
    assert ab['accepted_at']==tb['accepted_at'], 'TARGET_FREEZE_TIME'
    assert ab['original_receipt_utf8']==canonical(base_receipt(ab)).decode(), 'BASE_RECEIPT'
    event=lookup(tb['target'],'event')['body']['data']
    assert event['type']=='base' and event['status']=='succeeded', 'TARGET_INELIGIBLE'
    assert bb['event_id']==ab['target']==tb['target'], 'BASE_TARGET'
    assert event['occurred_at']<=bb['received_at']<=tb['accepted_at'], 'INVALID_ACCEPTED_ORDER'
    material=strict(bb['evaluation_utf8'].encode())
    assert canonical(material).decode()==bb['evaluation_utf8'], 'BASE_EVALUATION_BYTES'
    assert {'event','bundle','context','actions','explanations','deltas','consumptions','invocations','received_at','source_authority','costs'} <= set(material), 'BASE_EVALUATION_FIELDS'
    assert not any(r['operation']['kind']=='cap' for p in material['bundle']['policies'] for r in p['rules']), 'OUTCOME_CAP_COMPOSITION'
    assert material['received_at']==bb['received_at'], 'BASE_RECEIVED'
    original=strict(bb['original_event_utf8'].encode());ingress=strict(bb['original_ingress_utf8'].encode())
    assert original==material['event'], 'BASE_EVENT_BYTES'
    assert {k:v for k,v in ingress.items() if k!='chain'}=={k:v for k,v in original.items() if k!='chain'}, 'ORIGINAL_INGRESS'
    assert ingress.get('chain',original['chain'])==original['chain'], 'ORIGINAL_INGRESS'
    assert bb['original_event_id']=='ev_'+original_hash('event',only('event')['scope']+[event['source'],original['id']]), 'ORIGINAL_EVENT_ID'
    assert bb['original_event_hash']=='sha256:'+original_hash('event-content',original) and bb['original_ingress_hash']=='sha256:'+original_hash('ingress',ingress), 'ORIGINAL_EVENT_HASH'
    assert original.get('source',event['source'])==event['source'], 'BASE_AUTHORITY'
    assert event_projection(original,event['source'],aliases)==event, 'BASE_EVENT_PROJECTION'
    assert material['source_authority']['source']==event['source'] and material['source_authority']['active'], 'BASE_AUTHORITY'
    lookup(aliases[material['source_authority']['grant']],'evidence')
    bindings={r['body']['binding_id']:r for r in seed if r['kind']=='binding-snapshot'}
    assert tb['bindings']==bb['bindings']==ordered([reference(r) for r in bindings.values()]), 'FROZEN_BINDINGS'
    originals={p['binding']['id']:p['binding'] for p in material['bundle']['policies']}
    assert len(originals)==len(material['bundle']['policies']), 'DUPLICATE_BINDING'
    assert set(bindings)=={k for k,b in originals.items() if b['book'] in ('retail','supplier')}, 'FROZEN_BINDINGS'
    for name,r in bindings.items():
        b=r['body'];original=originals[name]
        assert strict(b['binding_utf8'].encode())==original, 'BASE_BINDING_MATERIAL'
        assert {k:v for k,v in b.items() if k not in ('schema','binding_utf8','booked_net','supplier_invocation')}==binding_projection(original,aliases), 'BASE_BINDING_PROJECTION'
    currency,scale=material['bundle']['currency'],material['bundle']['scale']
    def same_unit(m):
        assert m['currency']==currency and m['scale']==scale, 'POLICY_CURRENCY'
    postings=[deref(r) for r in bb['postings']]
    assert bb['postings']==ordered([reference(r) for r in seed if r['kind']=='base-posting']), 'ORIGINAL_BASE_POSTINGS'
    assert {p['id'] for p in postings}=={a['id'] for a in material['actions'] if a['book'] in ('retail','supplier')}, 'BASE_ACTION_MEMBERSHIP'
    for p in postings:
        b=p['body'];same_unit(b['amount']); a=next(a for a in material['actions'] if a['id']==p['id'])
        assert a['amount']==b['amount'] and a['book']==b['book'] and a['binding']['id']==b['binding_id'] and a['binding']['roles']==b['roles'], 'ORIGINAL_BASE_EVALUATION'
        binding=originals[b['binding_id']]
        assert a['binding']==binding and binding['agreement']==b['agreement_id'] and binding['book']==b['book'], 'BASE_ACTION_BINDING'
        rules=next(p['rules'] for p in material['bundle']['policies'] if p['binding']['id']==b['binding_id'])
        assert any(r['component']==a['component'] and r['on']==event['work_type'] for r in rules), 'BASE_ACTION_COMPONENT'
        assert b['event_id']==tb['target'], 'BASE_POSTING_TARGET'
    bases=[r for r in seed if r['kind']=='target-basis']
    for r in bases:
        b=r['body'];same_unit(b['amount']); bound=[p for p in postings if p['body']['book']==b['book'] and p['body']['agreement_id']==b['agreement_id']]
        assert b['postings']==ordered([reference(p) for p in bound]), 'BASIS_POSTINGS'
        assert b['amount']['atoms']==str(sum(int(p['body']['amount']['atoms']) for p in bound)), 'FROZEN_BASIS'
        assert int(b['amount']['atoms'])>=0, 'NEGATIVE_BASIS'
        assert b['target']==tb['target'] and b['finality']=='final' and b['finality_evidence']==tb['finality_evidence'], 'TARGET_NOT_FINAL'
    retail=[r for r in bases if r['body']['book']=='retail']; assert len(retail)==1, 'RETAIL_BASIS'
    assert tb['retail_basis']==reference(retail[0]), 'RETAIL_BASIS'
    families=[deref(f) for f in tb['families']]
    assert tb['families']==ordered([reference(r) for r in seed if r['kind']=='policy-snapshot']), 'FROZEN_MEMBERSHIP'
    source_policy=strict(tb['policy_utf8'].encode())
    assert source_policy['document']==tb['policy_document']==tb['verified_policy_document'], 'TERMS_NOT_VERIFIED'
    proofs=[lookup(v,'evidence') for v in tb['policy_evidence']]
    documents=[v for v in proofs if v['body']['document_id']==source_policy['document']]
    assert len(documents)==1 and documents[0]['body']['document_hash']==tb['policy_document_hash'], 'POLICY_DOCUMENT_EVIDENCE'
    assert strict(documents[0]['body']['utf8'].encode())=={k:v for k,v in source_policy.items() if k!='document'}, 'POLICY_DOCUMENT_TERMS'
    projected_families=ordered([family_projection(f) for f in source_policy['families']])
    projected_limits=ordered([dict(binding_id=l['binding_id'],premium=l['premium']) for l in tb['limits']])
    assert ordered(source_policy['limits'])==projected_limits, 'POLICY_LIMIT_PROJECTION'
    for f in source_policy['families']:
        for c in f['codes']:
            if c['amount']['kind']=='fixed':same_unit(c['amount']['money'])
    slots=set(); versions=set()
    limits={l['binding_id']:l for l in tb['limits']}
    assert len(limits)==len(tb['limits']) and set(limits)<=set(bindings) and all(f['body']['binding_id'] in limits for f in families), 'PREMIUM_BOUND_REQUIRED'
    for f in families:
        p=f['body'];assert p['currency']==currency and p['scale']==scale, 'POLICY_CURRENCY'
        b=bindings[p['binding_id']]['body']; slot=(p['agreement_id'],p['family_id'])
        assert slot not in slots, 'POLICY_AMBIGUOUS_MATCH'
        slots.add(slot); versions.add(p['policy_version'])
        assert all(p[k]==b[k] for k in ('agreement_id','book','roles','assent')), 'FAMILY_BINDING'
        assert p['submission_source'] in b['sources'] and p['correction_source'] in b['correction_sources'], 'OUTCOME_AUTHORITY'
        assert any(r['on']==event['work_type'] for r in next(p0['rules'] for p0 in material['bundle']['policies'] if p0['binding']['id']==p['binding_id'])), 'TARGET_INELIGIBLE'
        assert p['assent'] in tb['verified_assents'], 'TERMS_NOT_VERIFIED'
        if 'offer' in b: assert b['offer'] in tb['verified_offers'], 'TERMS_NOT_VERIFIED'
        if 'delegation' in b: assert b['delegation'] in tb['verified_delegations'], 'TERMS_NOT_VERIFIED'
        assert b['roles']['bearer']==b['roles']['payer'] or 'delegation' in b, 'PAYER_DELEGATION'
        for w in (p['ordinary'],p['corrections']):
            assert event['occurred_at']<=w['starts_at']<w['occurs_before']<=w['received_by']<=w['accepted_by'], 'OUTCOME_WINDOW'
        assert p['max_premium_atoms']==limits[p['binding_id']]['premium']['atoms'] and p['max_discount_atoms']==limits[p['binding_id']]['discount_capacity']['atoms'], 'FROZEN_LIMITS'
        assert set(p['replacement_codes'])<={r['code'] for r in p['rules']}, 'POLICY_OUTCOME_CODE'
    assert versions=={source_policy['version']}, 'FROZEN_POLICY_VERSION'
    assert projected_families==ordered([{k:p['body'][k] for k in projected_families[0]} for p in families]), 'POLICY_FAMILY_PROJECTION'
    for binding_id,row in bindings.items():
        b=row['body'];same_unit(b['booked_net'])
        net=sum(int(p['body']['amount']['atoms']) for p in postings if p['body']['binding_id']==binding_id)
        assert net==int(b['booked_net']['atoms'])>=0, 'BINDING_DISCOUNT_CAPACITY'
        if binding_id not in limits:continue
        limit=limits[binding_id];same_unit(limit['premium']);same_unit(limit['discount_capacity'])
        assert b['booked_net']==limit['discount_capacity'], 'BINDING_DISCOUNT_CAPACITY'
        premium=int(limit['premium']['atoms']); assert premium>=0, 'PREMIUM_BOUND_REQUIRED'
        if b['book']=='supplier':
            i=b['supplier_invocation']
            same_unit(i['held']);same_unit(i['maximum_exposure']);same_unit(b['maximum_exposure'])
            assert i in material['invocations'] and i['binding_id']==binding_id and i['operation_id']==event['operation_id'] and i['source']==event['source'], 'SUPPLIER_TARGET_PATH'
            assert material['event'].get('binding_id')==binding_id and material['event'].get('invocation_id')==i['id'] and i.get('completion_event',tb['target'])==tb['target'], 'SUPPLIER_TARGET_PATH'
            assert i['outcome_deadline'] and premium<=int(i['held']['atoms'])-net, 'EXPOSURE_EXCEEDED'
            assert bounded(net+premium)<=int(b['maximum_exposure']['atoms']), 'EXPOSURE_EXCEEDED'
            assert int(i['held']['atoms'])<=int(i['maximum_exposure']['atoms'])<=int(b['maximum_exposure']['atoms']), 'SUPPLIER_EXPOSURE'
            assert i['chain']==event['chain_id'] and i['customer']==event['customer'] and i['unit']==b['unit'], 'INVOCATION_SCOPE'
            assert i['authorized_at']<=i['attested_start']<i['start_before'], 'INVOCATION_EXPIRED'
            assert Fraction(event['quantity'])<=Fraction(i['maximum_quantity'])<=Fraction(b['maximum_quantity']), 'QUANTITY'
    for v in tb['verified_assents']+tb['verified_offers']+tb['verified_delegations']+[tb['finality_evidence']]: lookup(v,'evidence')
    for v in bb['predecessors']: deref(v)
    return target,retail[0],bindings,acceptance
