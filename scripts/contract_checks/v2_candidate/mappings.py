"""Original Evaluation identity preservation and canonical projection closure."""
from profile import canonical, ordered, reference, strict
from retained import evaluation_projection, original_identities


def verify_mappings(base, material, seed, deref):
    b=base['body'];assert b['source_state']=='accepted', 'ORIGINAL_EVALUATION_REQUIRED'
    original=strict(b['original_evaluation_utf8'].encode())
    assert evaluation_projection(original)==material, 'ORIGINAL_EVALUATION_PROJECTION'
    state=original['event']
    assert state['scope']==base['scope'], 'ORIGINAL_EVALUATION_SCOPE'
    for old,new in [('event_id','original_event_id'),('event_hash','original_event_hash'),('event_utf8','original_event_utf8'),('ingress_hash','original_ingress_hash'),('ingress_utf8','original_ingress_utf8')]:
        assert state[old]==b[new], 'ORIGINAL_EVENT_PROJECTION'
    assert state['source']==material['source_authority']['source'], 'ORIGINAL_EVENT_SOURCE'
    identities=original_identities(material,state['event_id'])
    mappings=b['identity_mappings']
    keys=[(m['original_kind'],m['original_id']) for m in mappings]
    assert len(keys)==len(set(keys)), 'DUPLICATE_ORIGINAL_MAPPING'
    assert set(keys)==identities, 'ORIGINAL_MAPPING_CLOSURE'
    projected=[canonical([m['projection']['kind'],m['projection']['id']]) for m in mappings]
    assert len(projected)==len(set(projected)), 'AMBIGUOUS_PROJECTION_MAPPING'
    actions={a['id']:a for a in material['actions']}
    assert len(actions)==len(material['actions']), 'DUPLICATE_ORIGINAL_ACTION'
    assert len({a['effect_id'] for a in material['actions']})==len(actions), 'DUPLICATE_ORIGINAL_EFFECT'
    original_bindings={p['binding']['id']:p['binding'] for p in material['bundle']['policies']}
    rows={};mapped_postings=[];mapped_indices=[]
    for m in mappings:
        assert m['target']==b['event_id'] and m['original_target']==state['event_id'], 'MAPPING_TARGET'
        row=deref(m['projection']);body=row['body'];kind=m['original_kind'];identity=m['original_id']
        rows[kind,identity]=row
        if kind=='event' and identity==state['event_id']:
            assert row['kind']=='event' and row['id']==b['event_id'], 'EVENT_MAPPING'
        elif kind=='action' and identity in actions and actions[identity]['book'] in ('retail','supplier'):
            a=actions[identity];binding=a['binding']
            assert row['kind']=='base-posting', 'ACTION_MAPPING_KIND'
            assert body['event_id']==b['event_id'], 'MAPPING_TARGET'
            expected=dict(event_id=b['event_id'],agreement_id=binding['agreement'],book=a['book'],binding_id=binding['id'],roles=binding['roles'],amount=a['amount'],ordinal=[v['id'] for v in material['actions'] if v['binding']['id']==binding['id']].index(identity))
            assert {k:v for k,v in body.items() if k!='schema'}==expected, 'ORIGINAL_ACTION_PROJECTION'
            mapped_postings.append(reference(row))
        elif kind=='obligation' and any(a['obligation_id']==identity and a['book'] in ('retail','supplier') for a in actions.values()):
            assert row['kind']=='obligation', 'OBLIGATION_MAPPING_KIND'
            own=[a for a in actions.values() if a['obligation_id']==identity]
            for a in own:
                expected=dict(agreement_id=a['binding']['agreement'],book=a['book'],roles=a['binding']['roles'],currency=a['amount']['currency'],scale=a['amount']['scale'])
                assert {k:v for k,v in body.items() if k!='schema'}==expected, 'ORIGINAL_OBLIGATION_PROJECTION'
        elif kind=='binding' and original_bindings[identity]['book'] in ('retail','supplier'):
            assert row['kind']=='binding-snapshot' and body['binding_id']==identity and strict(body['binding_utf8'].encode())==original_bindings[identity], 'ORIGINAL_BINDING_MAPPING'
        elif kind=='document':
            assert row['kind']=='evidence' and body['document_id']==identity, 'ORIGINAL_DOCUMENT_MAPPING'
        else:
            assert row['kind']=='base-identity', 'ORIGINAL_IDENTITY_MAPPING'
            assert all(body[k]==m[k] for k in ('target','original_target','original_kind','original_id')), 'ORIGINAL_IDENTITY_MAPPING'
            mapped_indices.append(row['id'])
    assert ordered(mapped_postings)==b['postings'], 'BASE_ACTION_MEMBERSHIP'
    assert set(mapped_indices)=={r['id'] for r in seed if r['kind']=='base-identity'}, 'UNMAPPED_ORIGINAL_INDEX'
    return rows
