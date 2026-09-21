"""Hash/reference checks deliberately separated from economic validity."""
from profile import canonical, content_hash, digest, effect_facts, facts, key, ordered, reference, row_order, strict
from semantics import base_receipt
from retained import document_fields, original_hash, validate_documents

def integrity(history,check_row):
    rows=history['seed']+[r for d in history['decisions'] for r in d['records']]
    validate_documents(rows)
    known={canonical(r['id']):r for r in rows}
    assert len(known)==len(rows), 'DUPLICATE_RECORD'
    def refs(v):
        if isinstance(v,dict):
            if set(v)=={'kind','id','content_hash'}:
                assert reference(known[canonical(v['id'])])==v, 'REFERENCE_HASH'
            else:
                for x in v.values():refs(x)
        elif isinstance(v,list):
            for x in v:refs(x)
    for r in rows:
        check_row(r);refs(r['body'])
        if r['kind']=='effect':assert r['body']['facts_hash']==digest('effect-facts',effect_facts(known[canonical(r['body']['action_id'])]['body']))
        if r['kind']=='claim':assert r['body']['facts_hash']==digest('claim-facts',facts(known[canonical(r['body']['first_event'])]['body']))
        if r['kind']=='delivery-key':assert r['body']['ingress_hash']==digest('ingress',r['body']['ingress'])
        if r['kind']=='base-acceptance':assert r['body']['original_receipt_utf8']==canonical(base_receipt(r['body'])).decode()
    for d in history['decisions']:
        def one(k):return next(r for r in d['records'] if r['kind']==k)
        m=one('decision-manifest');r=one('receipt');rp=one('replay-input')
        members={canonical([v['kind'],v['id']]):v for v in rp['body']['inputs']}
        members.update({canonical([v['kind'],v['id']]):reference(v) for v in d['records'] if v['kind'] not in ('receipt','decision-manifest')})
        assert m['body']['members']==sorted(members.values(),key=row_order)
        assert r['body']['decision_hash']==m['content_hash'] and r['body']['event_hash']==one('event')['content_hash']
        assert d['receipt_utf8']==canonical(r['body']).decode()


def rebuild_hash_graph(history):
    """Adversarial helper: repair ALL derived identities/hashes/references, not economics.

    Tests call the independent integrity checker (and Node) before semantic audit.
    No expected golden bytes or history-input values are used as repair inputs.
    """
    maps={}; previous=None
    def replace(v):
        if isinstance(v,str):
            if v in maps:return maps[v]
            if v.startswith('{') or v.startswith('['):
                try:return canonical(replace(strict(v.encode()))).decode()
                except (ValueError,UnicodeError):pass
            return v
        if isinstance(v,list):return [replace(x) for x in v]
        if isinstance(v,dict):return {k:replace(x) for k,x in v.items()}
        return v
    def sort_sets(v):
        from audit import SET_FIELDS
        if isinstance(v,dict):
            for k,x in v.items():
                sort_sets(x)
                if k in SET_FIELDS and isinstance(x,list):v[k]=ordered(x)
        elif isinstance(v,list):
            for x in v:sort_sets(x)
    for iteration in range(100):
        rows=history['seed']+[r for d in history['decisions'] for r in d['records']]
        for r in rows:r['body']=replace(r['body']);r['id']=replace(r['id'])
        known={canonical(r['id']):r for r in rows}
        # Reconstruct derivative references, payloads, membership and byte echoes.
        for r in rows:
            b=r['body'];k=r['kind']
            if k=='evidence':
                expected=document_fields(b['document_type'],strict(b['utf8'].encode()))
                for field in ('document_id','document_hash'):
                    if b[field]!=expected[field]:maps[b[field]]=expected[field]
                b.update(expected)
            if k=='base-evaluation':
                original=strict(b['original_event_utf8'].encode());ingress=strict(b['original_ingress_utf8'].encode())
                b['original_event_id']='ev_'+original_hash('event',r['scope']+[known[canonical(b['event_id'])]['body']['data']['source'],original['id']])
                b['original_event_hash']='sha256:'+original_hash('event-content',original)
                b['original_ingress_hash']='sha256:'+original_hash('ingress',ingress)
            if k=='effect': b['facts_hash']=digest('effect-facts',effect_facts(known[canonical(b['action_id'])]['body']))
            if k=='claim': b['facts_hash']=digest('claim-facts',facts(known[canonical(b['first_event'])]['body']))
            if k=='delivery-key':
                b['ingress']=known[canonical(b['event_id'])]['body'];b['ingress_hash']=digest('ingress',b['ingress'])
            if k=='base-acceptance':
                b['members']=sorted([reference(v) for v in history['seed'] if v['kind']!='base-acceptance'],key=row_order)
                b['original_receipt_utf8']=canonical(base_receipt(b)).decode()
        prior=list(history['seed'])
        for d in history['decisions']:
            def one(k):return next(r for r in d['records'] if r['kind']==k)
            rp=one('replay-input');rp['body']['inputs']=ordered([reference(v) for v in prior]+[reference(one(k)) for k in ('event','admission','authority-decision')]+[reference(v) for v in d['records'] if v['kind']=='evidence'])
            m=one('decision-manifest');r=one('receipt')
            for intention in (v for v in d['records'] if v['kind']=='intention'):
                ib=intention['body']; actions=[known[canonical(i)] for i in ib['action_ids']]
                ib['amount']['atoms']=str(sum(int(a['body']['amount']['atoms']) for a in actions))
                ib['payload']['amount']=dict(ib['amount']);ib['payload']['actions']=ordered([dict(action_id=a['id'],amount=a['body']['amount']) for a in actions])
            members={canonical([v['kind'],v['id']]):v for v in rp['body']['inputs']}
            members.update({canonical([v['kind'],v['id']]):reference(v) for v in d['records'] if v['kind'] not in ('receipt','decision-manifest')})
            m['body']['members']=sorted(members.values(),key=row_order)
            r['body']['event_hash']=one('event')['content_hash'];r['body']['decision_hash']=m['content_hash']
            prior+=d['records']
        for r in rows:
            sort_sets(r['body']);old_id=r['id'];old_hash=r['content_hash'];new_id=key(r['kind'],r['scope'],r['body']);new_hash=content_hash(r['kind'],r['body'])
            if isinstance(old_id,str) and old_id!=new_id:maps[old_id]=new_id
            if old_hash!=new_hash:maps[old_hash]=new_hash
            r['id']=new_id;r['content_hash']=new_hash
        history['seed'].sort(key=row_order)
        for d in history['decisions']:
            d['records'].sort(key=row_order);d['receipt_utf8']=canonical(next(r for r in d['records'] if r['kind']=='receipt')['body']).decode()
        current=canonical(history)
        if current==previous:return history
        previous=current
    raise AssertionError('adversarial hash graph did not converge')
