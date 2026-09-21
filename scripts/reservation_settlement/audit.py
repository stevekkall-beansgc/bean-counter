"""Read-only candidate audit. Expected numeric histories are hand-authored inputs."""
import copy
import hashlib
import json
import re
import sys
import unicodedata
from datetime import datetime
from pathlib import Path
from urllib.parse import urlsplit

import jsonschema
from codec import KINDS, canonical, context, digest, materialize, ordered, reference, row, strict

ROOT=Path(__file__).resolve().parents[2]
PACKAGE=ROOT/'contracts/candidates/reservation-settlement-v1'
MAX=10**30-1
REV=2**63-1
SCHEMA=json.loads((PACKAGE/'records.schema.json').read_text())
VALIDATOR=jsonschema.Draft202012Validator(SCHEMA)

class Rejected(ValueError): pass

def require(value,code):
    if not value: raise Rejected(code)

def time(value):
    require(isinstance(value,str) and re.fullmatch(r'\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d{6}Z',value), 'TIME')
    try: return datetime.strptime(value,'%Y-%m-%dT%H:%M:%S.%fZ')
    except ValueError: raise Rejected('TIME') from None

def uint(value,bound=MAX):
    require(isinstance(value,str) and re.fullmatch(r'0|[1-9][0-9]*',value) and int(value)<=bound,'INTEGER')
    return int(value)

def text(value,bound=128,source=False):
    require(isinstance(value,str) and 0<len(value.encode())<=bound and
            not any(unicodedata.category(x)=='Cc' for x in value),'TEXT')
    if source:
        # Python extra isspace C0 values already reject as controls.
        require(urlsplit(value).scheme and not any(x.isspace() for x in value),'SOURCE')

def state_check(s):
    uint(s['revision'],REV)
    values=[uint(s[k]) for k in ('maximum','consumed','held','released')]
    require(values[0]==sum(values[1:]),'CONSERVATION')
    require(s['families']==ordered(s['families']),'FAMILY_ORDER')
    keys=[canonical(f['key']) for f in s['families']]
    require(len(set(keys))==len(keys),'FAMILY_UNIQUENESS')
    for f in s['families']:
        text(f['key']['agreement_id']);text(f['key']['family_id']);time(f['accepted_by'])
        require(('ordinary_receipt' in f)==(f['status']=='claimed'),'FAMILY_RECEIPT')

def record_key(scope, record):
    # The content hash is evidence about an identity, never part of that identity.
    return canonical([scope,record['kind'],record['id']])

def retain_reference(registry,scope,record):
    key=record_key(scope,record)
    require(key not in registry or registry[key]==record,'REFERENCE_IDENTITY_CONFLICT')
    registry[key]=record

def referenced_values(value):
    if isinstance(value,dict):
        if set(value)=={'kind','id','content_hash'}:yield value
        else:
            for item in value.values():yield from referenced_values(item)
    elif isinstance(value,list):
        for item in value:yield from referenced_values(item)

def original_ingress(step,command):
    # Linked economics are synthetic trusted fixture inputs, not v2 wire bytes.
    if command['kind']=='close':return canonical(command)
    return canonical({k:step[k] for k in ('kind','id','family','amount') if k in step})

def lookup_delivery(deliveries,scope,command,ingress,may_read,absence_confirmed):
    if not may_read:return {'status':'unauthorized'}
    key=canonical([scope,command['source'],command['external_id']])
    stored=deliveries.get(key)
    if stored is None:
        return {'status':'absent' if absence_confirmed else 'outcome_unknown'}
    if stored['command']!=canonical(command) or stored['ingress']!=ingress:
        return {'status':'identity_conflict'}
    return {'status':'duplicate','composite':copy.deepcopy(stored['composite'])}

def validate(h,v,deliveries_out=None):
    require(v['name']==h['name'] and len(v['steps'])==len(h['steps']),'HISTORY')
    anchor,keys,economics,grant,evidence=context(h)
    expected_scope=h.get('scope',['synthetic','sandbox'])
    expected_unit=h.get('unit',{'currency':'USD','scale':2})
    for part in expected_scope:text(part)
    prior={}; current=None; previous=None; registration=None; last_time=None
    live={}; original={}; record_count=0
    deliveries={} if deliveries_out is None else deliveries_out
    journal_identities=set(); references={}
    for i,(s,step) in enumerate(zip(h['steps'],v['steps'])):
        rows=step['records']; record_count+=len(rows)
        require(len(rows) in (2,3),'RECORD_SET')
        require([r['kind'] for r in rows]==([KINDS[0],KINDS[2]] if len(rows)==2 else KINDS),'RECORD_ORDER')
        require(step['canonical_utf8']==[canonical(r).decode() for r in rows],'CANONICAL_BYTES')
        for r in rows:
            try: VALIDATOR.validate(r)
            except jsonschema.ValidationError: raise Rejected('SCHEMA') from None
            require(r==row(r['kind'],r['scope'],r['body']),'HASH_ID')
            require(r['scope']==expected_scope,'SCOPE')
            require(len(canonical(r['body']))<=256*1024,'BODY_LIMIT')
        require(sum(len(canonical(r)) for r in rows)<=4*1024*1024,'DECISION_LIMIT')
        o=rows[0]['body']; receipt=rows[-1]['body']; command=o['command'];kind=command['kind']
        text(command['source'],256,True);text(command['external_id']);text(command['invocation_id'])
        delivery=canonical([expected_scope,command['source'],command['external_id']])
        # Accepted-history steps contain new decisions only. Retries must return
        # retained bytes and append no step, even when command bytes are equal.
        require(delivery not in deliveries,'DELIVERY_IDENTITY_REUSED')
        for record in rows:
            identity=record_key(expected_scope,record)
            require(identity not in journal_identities,'RECORD_IDENTITY_REUSED')
            journal_identities.add(identity)
            retain_reference(references,expected_scope,reference(record))
            for dependency in referenced_values(record['body']):
                retain_reference(references,expected_scope,dependency)
        require(command['external_id']==s['id'] and kind==s['kind'],'COMMAND')
        require(command['invocation_id']=='invocation-one','INVOCATION')
        require(command['source']=='urn:synthetic:settlement','SOURCE_AUTHORITY')
        require(o['request_hash']==digest('request',command),'REQUEST_HASH')
        require(o['anchor']==anchor,'ANCHOR')
        require(o['unit']==expected_unit,'UNIT')
        require(time(o['received_at'])<=time(o['accepted_at']),'TIME_ORDER')
        if last_time is not None:require(time(o['accepted_at'])>=last_time,'TIME_ORDER')
        last_time=time(o['accepted_at'])
        auth=o['authority']
        text(auth['principal'])
        require(auth['grant']==grant and auth['evidence']==[evidence],'AUTHORITY_EVIDENCE')
        require(auth['active'] and uint(auth['grant_revision'],REV)>0,'AUTHORITY')
        require('read' in auth['permissions'],'READ_AUTHORITY')
        permission={'register':'submit','ordinary':'submit','post_hoc':'correct','close':'close'}[kind]
        require(permission in auth['permissions'],'WRITE_AUTHORITY')
        require(auth['permissions']==sorted(set(auth['permissions'])),'PERMISSION_ORDER')
        state_check(o['before'])
        if i==0:
            require(kind=='register' and 'registration' not in o and 'previous' not in o,'REGISTRATION')
            c=uint(h['base_consumed']);released=uint(h['base_released']);maximum=uint(h['maximum'])
            require(c+released<=maximum,'BASE_CAPACITY')
            require(uint(h['premium_ceiling'])<=maximum-c-released,'FROZEN_CEILING')
            expected={'revision':'0','maximum':h['maximum'],'consumed':h['base_consumed'],
                      'held':str(maximum-c-released),'released':h['base_released'],
                      'families':ordered([{'key':keys[f['id']],'accepted_by':f['accepted_by'],'status':'open'} for f in h['families']])}
            require(o['before']==expected,'BASE_CHECKPOINT')
            current=expected
        else:
            require(kind!='register' and o.get('registration')==registration and o.get('previous')==previous,'PREFIX')
            require(o['before']==current,'CURRENT_HEAD')
        result=copy.deepcopy(current);consume=release=0
        if kind in ('ordinary','post_hoc'):
            require(command['family']==keys[s['family']],'FAMILY')
            member=next(f for f in result['families'] if f['key']==command['family'])
            amount=s['amount']
            require(re.fullmatch(r'0|-?[1-9][0-9]*',amount) and abs(int(amount))<=MAX,'ECONOMIC_ATOMS')
            amount=int(amount)
            proposed=dict(live);proposed[s['family']]=amount
            require(sum(max(x,0) for x in proposed.values())<=int(h['premium_ceiling']) and
                    sum(max(-x,0) for x in proposed.values())<=int(h['base_consumed']),'ECONOMIC_CEILING')
            if kind=='ordinary':
                require(member['status']=='open','ORDINARY_CLOSED')
                require(time(o['accepted_at'])<=time(member['accepted_by']),'ORDINARY_DEADLINE')
                consume=max(amount,0)
                require(consume<=int(current['held']),'HELD_CAPACITY')
                member['status']='claimed';member['ordinary_receipt']=economics[s['id']]
                original[s['family']]=economics[s['id']]
            else:
                require(member['status']=='claimed' and member['ordinary_receipt']==original.get(s['family']),'POST_HOC_ORIGINAL')
            live=proposed
        elif kind=='close':
            require(uint(command['expected_revision'],REV)==int(current['revision']),'EXPECTED_REVISION')
            if command['reason']=='deadline':
                require(time(o['accepted_at'])>max(time(f['accepted_by']) for f in current['families']),'CLOSE_DEADLINE')
            else: require(s.get('early_authorized') is True,'CLOSE_AUTHORITY')
            for f in result['families']:
                if f['status']=='open':f['status']='closed'
            release=int(current['held'])
        result['consumed']=str(int(current['consumed'])+consume)
        result['held']=str(int(current['held'])-consume-release)
        result['released']=str(int(current['released'])+release)
        result['families']=ordered(result['families'])
        changed=result!=current
        if changed:
            require(int(current['revision'])<REV,'REVISION_EXHAUSTED')
            result['revision']=str(int(current['revision'])+1)
        require((len(rows)==3)==changed,'TRANSITION_PRESENCE')
        if changed:
            t=rows[1]['body']
            require(t['observation']==reference(rows[0]) and t['invocation_id']==command['invocation_id'],'TRANSITION_REFERENCE')
            require(t['before']==current and t['after']==result,'TRANSITION_STATE')
            require(t['consume']==str(consume) and t['release']==str(release),'TRANSITION_DELTA')
            require(receipt.get('transition')==reference(rows[1]),'RECEIPT_TRANSITION')
        else: require('transition' not in receipt,'NOOP_TRANSITION')
        state_check(receipt['result']);require(receipt['result']==result,'RESULT')
        require(receipt['observation']==reference(rows[0]),'RECEIPT_OBSERVATION')
        require(all(receipt[k]==command[k] for k in ('source','external_id','invocation_id')) and
                receipt['request_hash']==o['request_hash'],'RECEIPT_COMMAND')
        if kind=='close':require('economic_receipt' not in receipt and 'economic_receipt' not in o,'CLOSE_RECEIPT')
        else:
            require(command['economic_ingress_hash']==digest('synthetic-ingress',{k:s[k] for k in ('kind','id','family','amount') if k in s}),'ECONOMIC_INGRESS')
            require(o['economic_receipt']==economics[s['id']] and receipt['economic_receipt']==o['economic_receipt'],'ECONOMIC_RECEIPT')
        external=list(anchor.values())+[grant,evidence]
        if kind!='close':external.append(economics[s['id']])
        replay=dict(prior)
        for r in external+[reference(r) for r in rows[:-1]]:
            retain_reference(replay,expected_scope,r)
        require(receipt['replay']==ordered(replay.values()),'REPLAY_CLOSURE')
        require(len(receipt['replay'])<=1024,'REPLAY_LIMIT')
        prior=replay
        retain_reference(prior,expected_scope,reference(rows[-1]))
        composite={'settlement_receipt_utf8':canonical(rows[-1]).decode()}
        if 'economic_receipt' in receipt:composite['economic_receipt']=receipt['economic_receipt']
        deliveries[delivery]={'command':canonical(command),'ingress':original_ingress(s,command),'composite':composite}
        current=result;previous=reference(rows[-1]);registration=registration or previous
    return record_count

def mutate(history,attack):
    h=copy.deepcopy(history)
    for change in attack['changes']:
        target=h
        for key in change['path'][:-1]:target=target[key]
        target[change['path'][-1]]=change['value']
    return h

def validate_lookups(histories,vectors,cases):
    byname={h['name']:(h,v) for h,v in zip(histories,vectors)}
    for case in cases:
        h,v=byname[case['history']];end=case['after_step']+1;deliveries={}
        if end:validate({**h,'steps':h['steps'][:end]},{**v,'steps':v['steps'][:end]},deliveries)
        old=copy.deepcopy(deliveries)
        step=case['request_step'];command=copy.deepcopy(v['steps'][step]['records'][0]['body']['command'])
        ingress=original_ingress(h['steps'][step],command)
        command.update(case.get('command_patch',{}))
        if 'ingress_override' in case:ingress=case['ingress_override'].encode()
        reply=lookup_delivery(deliveries,h.get('scope',['synthetic','sandbox']),command,ingress,
                              case['may_read'],case['absence_confirmed'])
        require(reply['status']==case['expected'],'LOOKUP_STATUS:'+case['name'])
        if case['expected']=='duplicate':
            original=v['steps'][case['original_step']]['records'][-1]
            composite={'settlement_receipt_utf8':canonical(original).decode()}
            if 'economic_receipt' in original['body']:composite['economic_receipt']=original['body']['economic_receipt']
            require(reply=={'status':'duplicate','composite':composite},'ORIGINAL_COMPOSITE:'+case['name'])
        else:require(set(reply)=={'status'},'LOOKUP_DISCLOSURE')
        require(deliveries==old,'LOOKUP_APPENDED')
    return len(cases)

def verify_inventory():
    manifest=json.loads((PACKAGE/'review-manifest.json').read_text())
    require(manifest['status']=='candidate-not-frozen' and manifest['profile']=='reservation-settlement/1','CANDIDATE_STATUS')
    require(manifest['reviewed_base']=='6194376a053b8a27887a9b09459054a7af3a1769','REVIEW_BASE')
    expected={str(p.relative_to(ROOT)) for p in PACKAGE.iterdir() if p.is_file() and p.name!='review-manifest.json'}
    expected.update(str(p.relative_to(ROOT)) for p in (ROOT/'scripts/reservation_settlement').iterdir() if p.suffix in ('.py','.mjs'))
    expected.update(['scripts/check-reservation-settlement.sh','PRODUCT-PHASE-3-COORDINATOR-BLOCKER.md'])
    require(set(manifest['files'])==expected,'REVIEW_INVENTORY')
    for path,info in manifest['files'].items():
        raw=(ROOT/path).read_bytes()
        require(info=={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()},'REVIEW_BYTES:'+path)
    registry=(ROOT/'contracts/freeze.json').read_bytes()
    require(hashlib.sha256(registry).hexdigest()==manifest['frozen_registry_sha256'],'FROZEN_REGISTRY')
    frozen=json.loads(registry)['files']
    require(len(frozen)==159,'FROZEN_COUNT')
    for path,digest in frozen.items():
        require(hashlib.sha256((ROOT/path).read_bytes()).hexdigest()==digest,'FROZEN_BYTES:'+path)

def main():
    verify_inventory()
    package=json.loads((PACKAGE/'histories.json').read_text())
    histories=package['histories'];vectors=json.loads((PACKAGE/'vectors.json').read_text())
    require(vectors==[materialize(h) for h in histories],'VECTOR_RECONSTRUCTION')
    records=sum(validate(h,v) for h,v in zip(histories,vectors))
    lookups=validate_lookups(histories,vectors,json.loads((PACKAGE/'lookup-cases.json').read_text()))
    byname={h['name']:h for h in histories}
    attacks=json.loads((PACKAGE/'adversarial.json').read_text())
    results=[]
    for a in attacks:
        h=mutate(byname[a['history']],a)
        try:validate(h,materialize(h))
        except (Rejected,ValueError) as e:
            require(str(e)==a['expected'],f"ATTACK_REASON:{a['name']}:{e}")
            results.append({'name':a['name'],'rejected':str(e)})
        else:raise AssertionError('ATTACK_ACCEPTED:'+a['name'])
    raw_negatives=[b'{"x":1,"x":2}',b'{"x":-0}',b'{"x":1.0}',b'{"x":1e0}',b'{"x":null}',b'{"x":9007199254740992}',b'\xef\xbb\xbf{}',b'"\\ud800"',b'"\xff"']
    for raw in raw_negatives:
        try:strict(raw)
        except (ValueError,UnicodeError):pass
        else:raise AssertionError('STRICT_ACCEPTED')
    print(json.dumps({'status':'passed','histories':len(histories),'steps':sum(len(h['steps']) for h in histories),
          'records':records,'adversarial_cases':len(results),'rehashable_cases':len(results)-1,'strict_json_negatives':len(raw_negatives),'non_appending_lookup_cases':lookups}))

if __name__=='__main__':
    if '--emit-attacks' in sys.argv:
        histories={h['name']:h for h in json.loads((PACKAGE/'histories.json').read_text())['histories']}
        cases=[]
        for attack in json.loads((PACKAGE/'adversarial.json').read_text()):
            h=mutate(histories[attack['history']],attack)
            try:v=materialize(h)
            except ValueError:continue  # Null is rejected before canonical hashing.
            cases.append({'name':attack['name'],'history':h,'vector':v})
        print(json.dumps(cases,ensure_ascii=False))
    else:main()
