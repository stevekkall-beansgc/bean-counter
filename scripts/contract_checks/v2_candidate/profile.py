"""Candidate contract byte/identity reference only; no pricing or admission engine."""
import hashlib
import json
import re
from datetime import datetime
from math import gcd

PROFILE = '2-candidate.4'
SEMANTIC_COMMIT = '1e0ba3f886788c08f427d3aae1d916b341187e76'
PREFIX = dict(zip(
    ['evidence','policy-snapshot','event','base-posting','target-basis','admission',
     'claim','claim-revision','effect','action','obligation','limit-evidence',
     'explanation','replay-input','intention','decision-manifest','receipt'],
    ['ed','po','ev','bp','tb','ad','cl','rv','ef','ac','ob','li','xp','rp','in','dc','rc']))

PREFIX['base-identity']='bi'
PREFIX.update({'binding-snapshot':'bs','base-evaluation':'be','target-snapshot':'ts','base-acceptance':'ba','authority-decision':'au'})

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
    value = json.loads(raw.decode('utf-8'), object_pairs_hook=pairs,
                       parse_int=integer, parse_float=invalid, parse_constant=invalid)
    canonical(value)  # UTF-8, null, unsupported scalar and nesting checks.
    return value

def canonical(value, depth=0):
    if depth > 32: raise ValueError('JSON_DEPTH')
    if isinstance(value, dict):
        return b'{' + b','.join(canonical(k,depth+1)+b':'+canonical(value[k],depth+1)
            for k in sorted(value,key=lambda k:k.encode('utf-16-be'))) + b'}'
    if isinstance(value, list): return b'['+b','.join(canonical(v,depth+1) for v in value)+b']'
    if value is None or not isinstance(value,(str,int,bool)): raise ValueError('JSON_SCALAR')
    if isinstance(value,int) and not isinstance(value,bool) and abs(value)>9007199254740991:
        raise ValueError('JSON_INTEGER')
    return json.dumps(value,ensure_ascii=False,separators=(',',':')).encode('utf-8')

def h(domain,value):
    return hashlib.sha256(('ledgerlab/'+domain+'/'+PROFILE).encode()+b'\0'+canonical(value)).hexdigest()

def digest(domain,value): return 'sha256:'+h(domain,value)
def ident(kind,value): return PREFIX[kind]+'2_'+h(kind,value)
def body(kind,**fields): return {'schema':'ledger-'+kind+'/'+PROFILE,**fields}
def ordered(values): return sorted(values,key=canonical)
def row_order(r): return (r['kind'].encode(),canonical(r['id']))
def reference(row): return {k:row[k] for k in ('kind','id','content_hash')}

def key_input(kind,s,b):
    if kind in ('evidence','policy-snapshot','target-basis','replay-input','binding-snapshot','base-evaluation','target-snapshot'):
        value=[s,b]
    elif kind=='base-identity': value=[s,b['target'],b['original_kind'],b['original_id']]
    elif kind=='event': value=[s,b['data']['source'],b['data']['external_id']]
    elif kind=='base-posting': value=[s,b['event_id'],b['agreement_id'],b['book'],b['ordinal']]
    elif kind=='claim': value=[s,b['agreement_id'],b['family_id'],b['target']]
    elif kind=='claim-revision': value=[b['claim_id'],b['number']]
    elif kind=='effect': value=[b['claim_id'],b['revision_id'],b['slot']]
    elif kind=='action': value=[b['effect_id']]
    elif kind=='obligation': value=[s,b['agreement_id'],b['book'],b['currency'],b['scale'],b['roles']]
    elif kind=='base-acceptance': value=[b['target']]
    elif kind=='authority-decision': value=[b['event_id']]
    elif kind in ('admission','decision-manifest','receipt'): value=[b['event_id']]
    elif kind=='limit-evidence': value=[b['event_id'],0]
    elif kind=='explanation': value=[b['event_id'],b['ordinal']]
    elif kind=='intention': value=[s,b['destination'],b['obligation_id'],b['action_ids']]
    elif kind=='link': return [s,'outcome_of',b['event_id'],b['target']]
    elif kind=='dependency': return [s,b['dependent'],b['input']['kind'],b['input']['id']]
    elif kind=='delivery-key': return [s,b['source'],b['external_id']]
    elif kind=='chain-revision': return [s,b['chain_id'],b['number']]
    else: raise ValueError('UNKNOWN_KIND')
    return value

def key(kind,s,b):
    value=key_input(kind,s,b)
    return ident(kind,value) if kind in PREFIX else value

def content_hash(kind,b):
    return digest('decision-content',b) if kind=='decision-manifest' else digest('record-content',[kind,2,b])

def envelope(kind,scope,b):
    return {'kind':kind,'scope':scope,'id':key(kind,scope,b),'body':b,'content_hash':content_hash(kind,b)}

def facts(event, resolve):
    value={k:v for k,v in event['data'].items() if k!='external_id'}
    value['evidence']=ordered([resolve(i) for i in value['evidence']])
    return value
def effect_facts(action):
    return {k:v for k,v in action.items() if k not in ('schema','event_id','effect_id','policy_snapshot')}
