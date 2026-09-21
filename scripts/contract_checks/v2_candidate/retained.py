"""Lossless candidate codec projections; never rates or changes original values."""
import hashlib
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
        aliases[b['document_id']] = row['id']
    return aliases
