"""Lossless candidate codec projections; never rates or changes original values."""
import hashlib
import copy
import re
from profile import canonical, ordered, strict


def original_hash(domain, value):
    return hashlib.sha256(('ledgerlab/' + domain + '/1').encode() + b'\0' + canonical(value)).hexdigest()


def document_fields(kind, value):
    digest = original_hash('document', [kind, 1, value])
    return dict(document_type=kind, document_version=1, document_id='doc_' + digest,
                document_hash='sha256:' + digest, utf8=canonical(value).decode())


def event_projection(event, source, document_alias):
    result = dict(type='base', source=source, external_id=event['id'], chain_id=event['chain'],
                  work_type=event['type'])
    for field in ('customer', 'operation_id', 'occurred_at', 'status', 'quantity', 'unit'):
        result[field] = event[field]
    result['evidence'] = ordered([document_alias[d] for d in event['evidence']])
    return result


def binding_projection(binding, document_alias):
    result = {k: v for k, v in binding.items() if k not in ('id', 'agreement', 'assent', 'offer')}
    result.update(binding_id=binding['id'], agreement_id=binding['agreement'], assent=document_alias[binding['assent']])
    if 'offer' in binding:
        result['offer'] = document_alias[binding['offer']]
    if 'payer_delegation' in binding['roles']:
        result['delegation'] = document_alias[binding['roles']['payer_delegation']]
    # Source vectors are preserved in binding_utf8; index projections use sets.
    for field in ('sources', 'event_types', 'correction_sources', 'allowed_modifiers'):
        result[field] = ordered(list(set(result[field])))
    return result


def family_projection(family):
    result = {k: family[k] for k in ('binding_id', 'evidence_required', 'ordinary', 'corrections', 'allow_reversal')}
    result.update(family_id=family['family'], submission_source=family['source'],
                  correction_source=family['correction_source'], replacement_codes=ordered(family['replacement_codes']))
    rules = []
    for code in family['codes']:
        amount = code['amount']
        rules.append(dict(code=code['code'], kind='fixed', fixed_atoms=amount['money']['atoms'])
                     if amount['kind'] == 'fixed' else dict(code=code['code'], kind='percentage', rate=amount['rate']))
    result['rules'] = ordered(rules)
    return result


def validate_documents(rows):
    aliases = {}
    for row in rows:
        if row['kind'] != 'evidence':
            continue
        b = row['body']
        expected = document_fields(b['document_type'], strict(b['utf8'].encode()))
        assert all(b[k] == v for k, v in expected.items()), 'DOCUMENT_HASH'
        # Multiple contextual uses may retain the same original document.
        # Stable alias selection for display/indexing only. Set uniqueness below
        # is always checked on resolved original identities, never these aliases.
        aliases[b['document_id']] = min(aliases.get(b['document_id'],row['id']),row['id'])
    return aliases


def resolved_evidence(ids, lookup):
    documents=[lookup(i,'evidence')['body']['document_id'] for i in ids]
    assert len(documents)==len(set(documents)), 'DUPLICATE_DOCUMENT_EVIDENCE'
    return ordered(documents)


def evaluation_projection(original):
    """Exhaustive tag projection; all original identities are copied unchanged."""
    value=copy.deepcopy(original)
    value['event']=strict(value['event']['event_utf8'].encode())
    book={'Retail':'retail','Supplier':'supplier','CostObservation':'cost_observation','Allocation':'allocation'}
    kinds={v:v.lower() for v in ('Charge','Cost','Premium','Discount','Credit','Share','Allocation','Reversal')}
    def binding(b):b['book']=book[b['book']]
    def price(p):
        k,v=next(iter(p.items()))
        return dict(kind='fixed',value=v) if k=='Fixed' else dict(kind=k.lower(),**v)
    def operation(o):
        if isinstance(o,str):
            assert o=='ObserveCost';return dict(kind='observe_cost')
        k,v=next(iter(o.items()))
        if k in ('Base','Premium'):return dict(kind=k.lower(),price=price(v))
        if k in ('Discount','LinkedDiscount'):
            ak,av=next(iter(v['amount'].items()));v['amount']=dict(kind=ak.lower(),value=av)
            if 'mode' in v:v['mode']=v['mode'].lower()
        return dict(kind={'LinkedDiscount':'linked_discount'}.get(k,k.lower()),**v)
    def predicate(p):
        k,v=next(iter(p.items()));return dict(kind=k.lower(),value=v.lower() if k=='Funding' else v)
    for p in value['bundle']['policies']:
        binding(p['binding'])
        for r in p['rules']:
            r['operation']=operation(r['operation']);r['when']=[predicate(p) for p in r['when']]
            if 'matcher' in r:
                m=r['matcher'];r['matcher']=dict(kind='acquisition_optimization') if m=='AcquisitionOptimization' else dict(kind='direct',**m['Direct'])
    value['context']['funding']=value['context']['funding'].lower()
    for a in value['actions']:
        binding(a['binding']);a['book']=book[a['book']];a['kind']=kinds[a['kind']]
    for d in value['deltas']:d['book']=book[d['book']]
    return value


def original_identities(material, original_event_id):
    """All economically used internal IDs, plus binding and invocation labels.

    External event labels/parties/components remain exact values without a new
    identity namespace. Opaque event extensions are never economic references.
    """
    prefixes={'ev':'event','cl':'claim','ac':'action','ef':'effect','ob':'obligation','lk':'link','doc':'document'}
    result={('event',original_event_id)}
    def walk(v):
        if isinstance(v,dict):
            for k,x in v.items():
                if k!='extensions':walk(x)
        elif isinstance(v,list):
            for x in v:walk(x)
        elif isinstance(v,str):
            m=re.fullmatch(r'(ev|cl|ac|ef|ob|lk|doc)_[0-9a-f]{64}',v)
            if m:result.add((prefixes[m[1]],v))
    walk(material)
    result.update(('binding',p['binding']['id']) for p in material['bundle']['policies'])
    result.update(('invocation',i['id']) for i in material['invocations'])
    return result
