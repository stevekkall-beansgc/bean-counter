"""Candidate-only canonical reconstruction; no production pricing or authorization."""
import hashlib
import json
from copy import deepcopy

PROFILE = 'reservation-settlement/1'
KINDS = ['reservation-observation', 'reservation-transition', 'reservation-receipt']
PREFIX = dict(zip(KINDS, ['rso1_', 'rst1_', 'rsr1_']))

def canonical(value, depth=0):
    if depth > 32:
        raise ValueError('DEPTH')
    if isinstance(value, dict):
        return b'{' + b','.join(canonical(k, depth+1)+b':'+canonical(value[k], depth+1)
            for k in sorted(value, key=lambda k:k.encode('utf-16-be'))) + b'}'
    if isinstance(value, list):
        return b'[' + b','.join(canonical(x, depth+1) for x in value) + b']'
    if value is None or not isinstance(value, (str, int, bool)):
        raise ValueError('SCALAR')
    if isinstance(value, int) and not isinstance(value, bool) and abs(value) > 9007199254740991:
        raise ValueError('JSON_INTEGER')
    return json.dumps(value, ensure_ascii=False, separators=(',', ':')).encode('utf-8')

def strict(raw):
    def pairs(items):
        result = {}
        for k,v in items:
            if k in result: raise ValueError('DUPLICATE_KEY')
            result[k] = v
        return result
    def integer(s):
        if s == '-0' or abs(int(s)) > 9007199254740991: raise ValueError('JSON_INTEGER')
        return int(s)
    def invalid(_): raise ValueError('JSON_NUMBER')
    if raw.startswith(b'\xef\xbb\xbf'): raise ValueError('BOM')
    value = json.loads(raw.decode('utf-8'), object_pairs_hook=pairs, parse_int=integer,
                       parse_float=invalid, parse_constant=invalid)
    canonical(value)
    return value

def digest(domain, value):
    return 'sha256:'+hashlib.sha256(('ledgerlab/'+domain+'/'+PROFILE).encode()+b'\0'+canonical(value)).hexdigest()

def reference(row):
    return {k:row[k] for k in ('kind','id','content_hash')}

def ordered(values):
    return sorted(values, key=canonical)

def row(kind, scope, body):
    if kind == KINDS[0]:
        c = body['command']; identity = [scope,c['source'],c['external_id']]
    elif kind == KINDS[1]:
        identity = [scope,body['invocation_id'],body['after']['revision']]
    else:
        identity = [scope,body['source'],body['external_id']]
    return {'kind':kind,'scope':scope,'id':PREFIX[kind]+digest(kind,identity)[7:],
            'body':body,'content_hash':digest('record-content',[kind,1,body])}

def body(kind, **fields):
    return {'schema':'ledger-'+kind+'/'+PROFILE,**fields}

def synthetic_reference(kind, label):
    # A synthetic trust-boundary test input, not an alleged accepted v2 record.
    prefixes = {'base-acceptance':'ba2_', 'binding-snapshot':'bs2_', 'target-snapshot':'ts2_',
                'evidence':'ed2_', 'receipt':'rc2_', 'authority-decision':'au2_'}
    hashed = digest('synthetic-input',[kind,label])[7:]
    return {'kind':kind,'id':prefixes[kind]+hashed,'content_hash':'sha256:'+hashed}

def context(history):
    name = history['name']
    anchor = {key:synthetic_reference(kind,name+':'+key) for key,kind in
              [('base_acceptance','base-acceptance'),('binding_snapshot','binding-snapshot'),
               ('target_snapshot','target-snapshot'),('invocation_authorization','evidence')]}
    target = 'ev2_'+digest('synthetic-target',name)[7:]
    keys = {f['id']:{'agreement_id':'supplier-agreement','family_id':f['id'],'target':target}
            for f in history['families']}
    economics = {s['id']:synthetic_reference('receipt',name+':'+s['id'])
                 for s in history['steps'] if s['kind'] != 'close'}
    authority = synthetic_reference('evidence',name+':grant')
    evidence = synthetic_reference('evidence',name+':authority-evidence')
    return anchor,keys,economics,authority,evidence

def materialize(history):
    """Transcribe hand-authored expected states, not compute expected economics."""
    anchor,keys,economics,grant,evidence = context(history)
    scope = history.get('scope',['synthetic','sandbox'])
    unit = history.get('unit',{'currency':'USD','scale':2})
    steps=[]; prior=[]; first=None; previous=None; ordinary={}
    def state(expected):
        rev,consumed,held,released,statuses=expected
        families=[]
        for f in history['families']:
            item={'key':keys[f['id']],'accepted_by':f['accepted_by'],'status':statuses[f['id']]}
            if item['status']=='claimed': item['ordinary_receipt']=ordinary.get(f['id'], synthetic_reference('receipt',history['name']+':unclaimed:'+f['id']))
            families.append(item)
        return {'revision':rev,'maximum':history['maximum'],'consumed':consumed,'held':held,
                'released':released,'families':ordered(families)}
    current=state(history['initial'])
    for index,s in enumerate(history['steps']):
        kind=s['kind']
        command={'kind':kind,'source':'urn:synthetic:settlement','external_id':s['id'],
                 'invocation_id':'invocation-one'}
        if kind!='close': command['economic_ingress_hash']=digest('synthetic-ingress',{k:s[k] for k in ('kind','id','family','amount') if k in s})
        if kind in ('ordinary','post_hoc'): command['family']=keys[s['family']]
        if kind=='close': command.update(reason=s['reason'],expected_revision=s.get('expected_revision',current['revision']))
        permissions=s.get('permissions',['read','submit','correct','close'])
        observed=body(KINDS[0],command=command,request_hash=digest('request',command),anchor=anchor,
            unit=unit,before=deepcopy(current),authority={'principal':'synthetic-operator','grant':grant,
            'grant_revision':s.get('grant_revision','1'),'active':s.get('active',True),
            'permissions':sorted(permissions),'evidence':[evidence]},
            received_at=s.get('received_at',s['time']),accepted_at=s['time'])
        if kind!='close': observed['economic_receipt']=economics[s['id']]
        if index: observed.update(registration=first,previous=previous)
        observed.update(deepcopy(s.get('observation_patch',{})))
        observation=row(KINDS[0],scope,observed)
        if kind=='ordinary': ordinary[s['family']]=economics[s['id']]
        result=state(s['expected'])
        records=[observation]
        if 'transition' in s:
            consume,release=s['transition']
            transition=row(KINDS[1],scope,body(KINDS[1],invocation_id=command['invocation_id'],
                observation=reference(observation),before=deepcopy(current),after=deepcopy(result),
                consume=consume,release=release))
            records.append(transition)
        # Fixture external replay closure is declared at its trusted input boundary.
        external=list(anchor.values())+[grant,evidence]
        if kind!='close': external.append(economics[s['id']])
        replay={canonical(r):r for r in external+prior+[reference(r) for r in records]}
        receipt_body=body(KINDS[2],source=command['source'],external_id=command['external_id'],
            invocation_id=command['invocation_id'],request_hash=observed['request_hash'],
            observation=reference(observation),result=result,replay=ordered(replay.values()))
        if len(records)==2: receipt_body['transition']=reference(records[1])
        if kind!='close': receipt_body['economic_receipt']=economics[s['id']]
        receipt_body.update(deepcopy(s.get('receipt_patch',{})))
        receipt=row(KINDS[2],scope,receipt_body); records.append(receipt)
        steps.append({'records':records,'canonical_utf8':[canonical(r).decode() for r in records]})
        prior += external+[reference(r) for r in records]
        first=first or reference(receipt); previous=reference(receipt); current=result
    return {'name':history['name'],'steps':steps}
