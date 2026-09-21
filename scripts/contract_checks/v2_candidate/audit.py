"""Read-only candidate contract audit; no production evaluator/store imports."""
import copy
import hashlib
from datetime import datetime, timedelta
import json
import subprocess
from fractions import Fraction
from pathlib import Path
import jsonschema
from profile import (canonical, content_hash, digest, effect_facts, facts, key,
                     ordered, reference, row_order, scalar_checks, strict)
from reconstruct import CANDIDATE, ROOT, files

SCHEMA=strict((CANDIDATE/'schemas/canonical-records.schema.json').read_bytes())
jsonschema.Draft202012Validator.check_schema(SCHEMA)
VALIDATOR=jsonschema.Draft202012Validator(SCHEMA)
SET_FIELDS={'rules','evidence','postings','live_action_ids','inverse_action_ids','current_before','action_ids','inputs','depends_on','actions','intention_ids'}

def check_order(value,name=''):
    if isinstance(value,dict):
        for k,v in value.items(): check_order(v,k)
    elif isinstance(value,list):
        if name in SET_FIELDS:
            assert value==ordered(value), 'SET_ORDER'
            assert len({canonical(v) for v in value})==len(value), 'SET_DUPLICATE'
        for v in value: check_order(v)

def checked_row(r):
    VALIDATOR.validate(r); scalar_checks(r); check_order(r)
    assert all(len(s.encode())<=128 for s in r['scope']), 'SCOPE_BYTES'
    assert len(canonical(r['body']))<=262144, 'RECORD_BYTES'
    assert key(r['kind'],r['scope'],r['body'])==r['id'], 'RECORD_ID'
    assert content_hash(r['kind'],r['body'])==r['content_hash'], 'CONTENT_HASH'
    if r['kind']=='evidence': assert len(r['body']['utf8'].encode())<=262144, 'EVIDENCE_BYTES'
    if r['kind']=='policy-snapshot':
        b=r['body']; assert len({v['code'] for v in b['rules']})==len(b['rules']), 'OUTCOME_CODE_DUPLICATE'
        assert 'supplier_authorization' in b or b['book']=='retail', 'SUPPLIER_AUTHORITY'
        assert 'delegation' in b or b['roles']['bearer']==b['roles']['payer'], 'PAYER_DELEGATION'
        assert b['window_start']<b['window_end']<=b['report_before']<=b['correct_before'], 'WINDOW_ORDER'
        parse=lambda t:datetime.strptime(t,'%Y-%m-%dT%H:%M:%S.%fZ')
        assert parse(b['window_end'])-parse(b['window_start'])<=timedelta(days=90) and parse(b['report_before'])-parse(b['window_end'])<=timedelta(days=7), 'WINDOW_BOUND'
        assert int(b['policy_version'])>=1, 'POLICY_VERSION'
        assert int(b['max_premium_atoms'])>=0 and int(b['max_discount_atoms'])>=0, 'NEGATIVE_LIMIT'

def round_away(q):
    whole,rem=divmod(abs(q.numerator),q.denominator)
    return (-1 if q<0 else 1)*(whole+(2*rem>=q.denominator))

def verify(history):
    assert set(history)=={'status','name','seed','decisions','probes'}
    assert history['status']=='candidate-not-frozen'
    known={}; heads={}; claim_intents={}; reversed_actions=set(); scope=None
    def lookup(identity,kind=None):
        r=known[canonical(identity)]
        if kind: assert r['kind']==kind, 'REF_KIND'
        assert r['scope']==scope, 'REF_SCOPE'
        return r
    def deref(v):
        r=lookup(v['id'],v['kind']); assert reference(r)==v, 'REF_HASH'; return r
    def insert(rows):
        nonlocal scope
        assert sorted(rows,key=row_order)==rows, 'ROW_ORDER'
        for r in rows:
            checked_row(r)
            scope=scope or r['scope']; assert scope==r['scope'], 'SCOPE'
            k=canonical(r['id']); assert k not in known, 'IDENTITY_REUSED'; known[k]=r
    insert(history['seed'])
    for r in history['seed']:
        b=r['body']
        if r['kind']=='target-basis':
            lookup(b['target'],'event'); lookup(b['finality_evidence'],'evidence')
            postings=[deref(x) for x in b['postings']]
            assert all(p['kind']=='base-posting' and p['body']['event_id']==b['target'] and p['body']['agreement_id']==b['agreement_id'] and p['body']['book']==b['book'] and p['body']['amount']['currency']==b['amount']['currency'] and p['body']['amount']['scale']==b['amount']['scale'] and p['body']['roles']['payer']==b['payer'] for p in postings), 'BASIS_POSTINGS'
            assert sum(int(p['body']['amount']['atoms']) for p in postings)==int(b['amount']['atoms'])>=0, 'FROZEN_BASIS'
    for ix,decision in enumerate(history['decisions']):
        assert set(decision)=={'records','receipt_utf8'}
        rows=decision['records']
        assert not {r['kind'] for r in rows} & {'evidence','policy-snapshot','base-posting','target-basis','obligation'}, 'UNREFERENCED_NEW_SEED'
        prior_rows=list(known.values()); insert(rows)
        def all_kind(k): return [r for r in rows if r['kind']==k]
        def one(k):
            rs=all_kind(k); assert len(rs)==1, 'RECORD_CARDINALITY:'+k; return rs[0]
        event=one('event'); eb=event['body']; data=eb['data']; E=event['id']
        revision=one('claim-revision'); rb=revision['body']; cl=lookup(rb['claim_id'],'claim'); cb=cl['body']
        prior=heads.get(cl['id']); correction=data['type']=='correction'
        assert correction==bool(prior), 'CLAIM_ALREADY_EXISTS'
        assert all(cb[k]==data[k] for k in ('target','agreement_id','book','family_id')), 'CLAIM_SLOT'
        policy=lookup(rb['policy_snapshot'],'policy-snapshot'); pb=policy['body']; basis=lookup(rb['basis'],'target-basis'); bb=basis['body']
        assert all(pb[k]==data[k] for k in ('agreement_id','book','family_id')), 'POLICY_SLOT'
        assert bb['target']==data['target'] and bb['agreement_id']==data['agreement_id'] and bb['book']==data['book'], 'BASIS_SLOT'
        assert bb['payer']==data['payer']==pb['roles']['payer'], 'PAYER'
        assert pb['currency']==bb['amount']['currency'] and pb['scale']==bb['amount']['scale'], 'POLICY_MONEY_UNIT'
        assert bb['stage']=='uncapped', 'CAP_OUTCOME_INCOMPATIBLE'
        assert bb['finality']=='final', 'TARGET_NOT_FINAL'
        assert bb['amount']['currency']==rb['amount']['currency'] and bb['amount']['scale']==rb['amount']['scale'], 'MONEY_UNIT'
        assert rb['event_id']==E and rb['code']==data['code'], 'REVISION_EVENT'
        assert rb['original_receipt']==cb['original_receipt'], 'ORIGINAL_RECEIPT'
        assert int(pb['max_discount_atoms'])<=int(bb['amount']['atoms']), 'DISCOUNT_LIMIT_BASIS'
        adm=one('admission'); ab=adm['body']
        assert all(int(ab[k])>=1 for k in ('credential_revision','grant_revision','target_guard_revision')), 'AUTHORITY_REVISION'
        assert rb['admission']==adm['id'] and ab['event_id']==E and ab['policy_snapshot']==policy['id'] and ab['basis']==basis['id'], 'ADMISSION_REFS'
        assert ab['permission']==data['type'] and ab['authorized_source']==data['source']==pb['correction_source' if correction else 'submission_source'], 'SOURCE_AUTHORITY'
        assert all(ab[k]==data[k] for k in ('agreement_id','payer','book','family_id')), 'ADMISSION_SCOPE'
        assert data['occurred_at']<=ab['received_at']<=ab['accepted_at'], 'CLOCK_ORDER'
        assert pb['window_start']<=data['occurred_at']<pb['window_end'], 'OUTCOME_WINDOW'
        if correction:
            assert prior['id']==data['expected_revision']==rb['previous'] and data['claim_id']==cl['id'], 'STALE_CLAIM_REVISION'
            assert int(rb['number'])==int(prior['body']['number'])+1, 'REVISION_SEQUENCE'
            assert rb['policy_snapshot']==prior['body']['policy_snapshot'] and rb['basis']==prior['body']['basis'], 'PIN_CHANGED'
            assert data['corrected_at']<=ab['received_at']<pb['correct_before'] and data['corrected_at']<pb['correct_before'], 'CORRECTION_TOO_LATE'
            assert data['corrected_at']>=lookup(prior['body']['admission'],'admission')['body']['accepted_at'], 'CORRECTION_ORDER'
            assert not all_kind('claim'), 'CLAIM_REINSERTED'
        else:
            assert rb['number']=='1' and 'previous' not in rb, 'INITIAL_REVISION'
            assert cb['first_event']==E and cb['facts_hash']==digest('claim-facts',facts(eb)), 'CLAIM_FACTS'
            assert ab['received_at']<pb['report_before'], 'REPORT_TOO_LATE'
            assert one('claim')['id']==cl['id']
        for proof,kind in [(ab['authentication'],'authentication'),(ab['grant'],'grant'),(pb['assent'],'assent')]:
            assert lookup(proof,'evidence')['body']['purpose']==kind, 'EVIDENCE_PURPOSE'
        for proof in data['evidence']:
            assert lookup(proof,'evidence')['body']['purpose']==('correction' if correction else 'outcome'), 'OUTCOME_EVIDENCE'
        assert data['evidence'], 'EVIDENCE_REQUIRED'
        if pb['book']=='supplier': assert lookup(pb['supplier_authorization'],'evidence')['body']['purpose']=='supplier_authorization', 'SUPPLIER_AUTHORITY'
        link=one('link')['body']; assert link['event_id']==E and link['target']==data['target'], 'LINK'
        actions=all_kind('action'); effects=all_kind('effect'); live=[a for a in actions if a['body']['slot']=='replacement']; inverses=[a for a in actions if a['body']['slot']=='inverse']
        assert rb['live_action_ids']==ordered([a['id'] for a in live]) and rb['inverse_action_ids']==ordered([a['id'] for a in inverses]), 'ACTION_MEMBERSHIP'
        amount=int(rb['amount']['atoms']); assert len(live)==(amount!=0), 'ZERO_ACTION'
        if live: assert live[0]['body']['amount']==rb['amount'], 'REPLACEMENT_AMOUNT'
        expected_originals=prior['body']['live_action_ids'] if prior else []
        assert ordered([a['body']['reverses'] for a in inverses])==expected_originals, 'INVERSE_CLOSURE'
        assert len(effects)==len(actions), 'EFFECT_COUNT'
        for a in actions:
            b=a['body']; assert int(b['amount']['atoms'])!=0, 'ZERO_ACTION'
            assert b['revision_id']==revision['id'] and b['claim_id']==cl['id'] and b['event_id']==E, 'ACTION_OWNER'
            assert b['book']==data['book'] and b['agreement_id']==data['agreement_id'] and b['family_id']==data['family_id'] and b['roles']==pb['roles'], 'ACTION_PARTIES'
            assert b['policy_snapshot']==policy['id'] and b['basis']==basis['id'], 'ACTION_PINS'
            assert b['amount']['currency']==rb['amount']['currency'] and b['amount']['scale']==rb['amount']['scale'], 'ACTION_UNIT'
            o=lookup(b['obligation_id'],'obligation')['body']
            assert all(o[k]==b[k] for k in ('agreement_id','book','roles')) and o['currency']==b['amount']['currency'] and o['scale']==b['amount']['scale'], 'OBLIGATION'
            effect=lookup(b['effect_id'],'effect')['body']
            assert effect['action_id']==a['id'] and effect['facts_hash']==digest('effect-facts',effect_facts(b)) and effect['revision_id']==revision['id'] and effect['claim_id']==cl['id'] and effect['slot']==b['slot'], 'EFFECT_FACTS'
            if b['slot']=='inverse':
                old=lookup(b['reverses'],'action')['body']; assert b['reverses'] not in reversed_actions, 'ALREADY_REVERSED'; reversed_actions.add(b['reverses'])
                assert int(b['amount']['atoms'])==-int(old['amount']['atoms']), 'EXACT_INVERSE'
                assert all(b[k]==old[k] for k in ('obligation_id','book','agreement_id','family_id','roles','policy_snapshot','basis')), 'INVERSE_PROVENANCE'
            else: assert 'reverses' not in b, 'ORIGINAL_REVERSES'
        limits=one('limit-evidence'); lb=limits['body']; before=[v for v in heads.values() if all(lookup(v['body']['claim_id'],'claim')['body'][k]==cb[k] for k in ('target','agreement_id','book'))]
        assert lb['event_id']==E and all(lb[k]==cb[k] for k in ('target','agreement_id','book')), 'LIMIT_SCOPE'
        assert lb['currency']==bb['amount']['currency'] and lb['scale']==bb['amount']['scale'], 'LIMIT_UNIT'
        assert lb['current_before']==ordered([reference(v) for v in before]), 'INCOMPLETE_AGGREGATE'
        assert lb.get('replacing')==(prior['id'] if prior else None), 'LIMIT_REPLACING'
        after=[v for v in before if v['body']['claim_id']!=cl['id']]+[revision]
        for label,values in [('before',before),('after',after)]:
            amounts=[int(v['body']['amount']['atoms']) for v in values]
            assert int(lb[label+'_premium'])==sum(max(n,0) for n in amounts) and int(lb[label+'_discount'])==sum(max(-n,0) for n in amounts), 'LIMIT_TOTAL'
        assert lb['maximum_premium']==pb['max_premium_atoms'] and lb['maximum_discount']==pb['max_discount_atoms'], 'LIMIT_PIN'
        assert int(lb['after_premium'])<=int(lb['maximum_premium']) and int(lb['after_discount'])<=int(lb['maximum_discount']), 'AGGREGATE_LIMIT'
        # All families on this target/book pin the SAME aggregate limits.
        for v in before:
            oldp=lookup(v['body']['policy_snapshot'],'policy-snapshot')['body']
            assert all(oldp[k]==pb[k] for k in ('max_premium_atoms','max_discount_atoms')), 'LIMIT_TERMS_MISMATCH'
        xp=one('explanation'); xb=xp['body']; rule=next(r for r in pb['rules'] if r['code']==data['code'])
        if rule['kind']=='percentage': assert (abs(int(bb['amount']['atoms']))*abs(int(rule['rate']['numerator']))).bit_length()<=512, 'MATH_TEMPORARY_BOUND'
        exact=Fraction(int(rule['fixed_atoms'])) if rule['kind']=='fixed' else Fraction(int(bb['amount']['atoms']))*Fraction(int(rule['rate']['numerator']),int(rule['rate']['denominator']))
        assert max(abs(exact.numerator).bit_length(),exact.denominator.bit_length())<=512, 'MATH_BOUND'
        assert xb['unrounded_atoms']=={'numerator':str(exact.numerator),'denominator':str(exact.denominator)} and round_away(exact)==amount and xb['rounded_atoms']==str(amount), 'EXPLANATION_MATH'
        assert xb['event_id']==E and xb['claim_id']==cl['id'] and xb['revision_id']==revision['id'] and xb['limit_evidence']==limits['id'] and xb['policy_snapshot']==policy['id'] and xb['basis']==basis['id'], 'EXPLANATION_REFS'
        assert xb['action_ids']==ordered([a['id'] for a in actions]), 'EXPLANATION_ACTIONS'
        expected_code=('CORRECTION_ZERO' if not amount else 'CORRECTION_REPLACED') if correction else ('ZERO_ADJUSTMENT' if not amount else ('FIXED_APPLIED' if rule['kind']=='fixed' else 'PERCENTAGE_APPLIED'))
        assert xb['code']==expected_code, 'EXPLANATION_CODE'
        deps=[]
        for a in actions:
            deps.append(dict(dependent=a['id'],input=reference(basis),reason='frozen_basis'))
            if a['body']['slot']=='inverse': deps.append(dict(dependent=a['id'],input=reference(lookup(a['body']['reverses'],'action')),reason='exact_inverse'))
        deps += [dict(dependent=xp['id'],input=reference(basis),reason='frozen_basis'),dict(dependent=xp['id'],input=reference(limits),reason='aggregate_limit')]
        if prior: deps.append(dict(dependent=xp['id'],input=reference(prior),reason='prior_revision'))
        assert ordered([{k:v for k,v in d['body'].items() if k!='schema'} for d in all_kind('dependency')])==ordered(deps), 'DEPENDENCY_CLOSURE'
        replay=one('replay-input'); replayb=replay['body']
        assert replayb['event_id']==E and replayb['received_at']==ab['received_at'] and replayb['accepted_at']==ab['accepted_at'], 'REPLAY_CONTEXT'
        assert replayb['inputs']==ordered([reference(r) for r in prior_rows]+[reference(event),reference(adm)]), 'REPLAY_INCOMPLETE'
        for v in replayb['inputs']: deref(v)
        net=sum(int(a['body']['amount']['atoms']) for a in actions); intents=all_kind('intention')
        assert len(intents)==(net!=0), 'INTENTION_CARDINALITY'
        if intents:
            i=intents[0]; ib=i['body']; assert ib['amount']=={**rb['amount'],'atoms':str(net)} and ib['event_id']==E and ib['idempotency_key']==i['id'], 'INTENTION_AMOUNT'
            assert ib['action_ids']==ordered([a['id'] for a in actions]) and all(a['body']['obligation_id']==ib['obligation_id'] for a in actions), 'INTENTION_ACTIONS'
            assert ib['depends_on']==ordered(claim_intents.get(cl['id'],[])), 'EXPORT_DEPENDENCIES'
            assert ib['payload']=={'schema':'ledger-obligation-delta/2-candidate.1','obligation_id':ib['obligation_id'],'amount':ib['amount'],'actions':ordered([{'action_id':a['id'],'amount':a['body']['amount']} for a in actions])}, 'INTENTION_PAYLOAD'
            claim_intents.setdefault(cl['id'],[]).append(i['id'])
        delivery=one('delivery-key')['body']; assert delivery['event_id']==E and delivery['ingress']==eb and delivery['ingress_hash']==digest('ingress',eb) and delivery['source']==data['source'] and delivery['external_id']==data['external_id'], 'DELIVERY_BYTES'
        manifest=one('decision-manifest'); mb=manifest['body']; receipt=one('receipt'); recb=receipt['body']; chain=one('chain-revision')['body']
        assert chain['number']==str(ix+1)==mb['revision']==recb['revision'] and chain['event_id']==E and chain['decision_id']==manifest['id'], 'CHAIN_REVISION'
        assert all(b['chain_id']==data['chain_id'] for b in (chain,mb,recb)) and data['chain_id']==history['name'], 'CHAIN_ID'
        assert mb['event_id']==E and mb['accepted_at']==ab['accepted_at'] and mb['explanation_ids']==[xp['id']], 'MANIFEST_CONTEXT'
        expected={canonical([r['kind'],r['id']]):reference(r) for r in rows if r['kind'] not in ('decision-manifest','receipt')}
        expected.update({canonical([r['kind'],r['id']]):r for r in replayb['inputs']})
        assert mb['members']==sorted(expected.values(),key=row_order), 'MANIFEST_INCOMPLETE'
        for v in mb['members']: deref(v)
        assert recb['event_id']==E and recb['decision_id']==manifest['id'] and recb['event_hash']==event['content_hash'] and recb['decision_hash']==manifest['content_hash'], 'RECEIPT_HASHES'
        assert recb['claim_id']==cl['id'] and recb['claim_revision']==revision['id'] and recb['action_ids']==ordered([a['id'] for a in actions]) and recb['intention_ids']==ordered([i['id'] for i in intents]) and recb['accepted_at']==ab['accepted_at'], 'RECEIPT_MEMBERSHIP'
        assert delivery['original_receipt']==receipt['id'] and decision['receipt_utf8']==canonical(recb).decode(), 'RECEIPT_BYTES'
        if not prior: assert cb['original_receipt']==receipt['id'], 'FIRST_RECEIPT'
        heads[cl['id']]=revision
        assert len(canonical(decision))<=4*1024*1024, 'DECISION_BYTES'
    for p in history['probes']:
        assert p['new_records']==[], 'REJECTED_OR_RETRY_APPENDED'
        if 'candidate' in p:
            jsonschema.Draft202012Validator({'$ref':'#/$defs/event','$defs':SCHEMA['$defs']}).validate(p['candidate'])
        if p['kind'] in ('identity_retry','semantic_retry','changed_facts','stale_correction'):
            original=next(r for r in history['decisions'][p['step']]['records'] if r['kind']=='event')
            cb=p['candidate']['data']
            if p['kind']=='identity_retry': assert canonical(p['candidate'])==canonical(original['body']) and p['expected']=='DUPLICATE_IDENTITY'
            if p['kind']=='semantic_retry': assert facts(p['candidate'])==facts(original['body']) and cb['external_id']!=original['body']['data']['external_id'] and p['expected']=='DUPLICATE_CLAIM'
            if p['kind']=='changed_facts': assert facts(p['candidate'])!=facts(original['body']) and p['expected']=='CLAIM_CONFLICT'
            if p['kind']=='stale_correction': assert cb['expected_revision']!=heads[cb['claim_id']]['id'] and p['expected']=='STALE_CLAIM_REVISION'
        if p['kind'].endswith('_retry'):
            checked_row(p['current_policy'])
            assert p['current_policy']['body']['policy_version']=='2'
            assert p['original_receipt_utf8']==history['decisions'][p['step']]['receipt_utf8'], 'RETRY_RECEIPT'
            assert p['current_policy_version']=='2', 'RETRY_VERSION_PROBE'
        if p['kind']=='aggregate_limit':
            assert p['current_before']==ordered([reference(v) for v in heads.values()])
            proposed=int(p['proposed_atoms']); pb=lookup(p['policy_snapshot'],'policy-snapshot')['body']
            assert next(r['fixed_atoms'] for r in pb['rules'] if r['code']=='success')==p['proposed_atoms']
            assert sum(max(0,int(v['body']['amount']['atoms'])) for v in heads.values())+max(proposed,0)>int(pb['max_premium_atoms']) and p['expected']=='AGGREGATE_LIMIT'
        if p['kind']=='timing':
            pb=lookup(p['policy_snapshot'],'policy-snapshot')['body']
            if p['expected']=='OUTCOME_WINDOW': assert not pb['window_start']<=p['occurred_at']<pb['window_end']
            elif p['expected']=='REPORT_TOO_LATE': assert p['received_at']>=pb['report_before']
            else: assert False, 'TIMING_REASON'
        if p['kind']=='correction_timing': assert p['corrected_at']>=lookup(p['policy_snapshot'],'policy-snapshot')['body']['correct_before'] and p['expected']=='CORRECTION_TOO_LATE'
        if p['kind']=='cap': assert lookup(p['basis'],'target-basis')['body']['stage']==p['stage']=='capped' and p['expected']=='CAP_OUTCOME_INCOMPATIBLE'
    return len(known)

def main():
    review=strict((CANDIDATE/'review-manifest.json').read_bytes())
    assert review['status']=='candidate-not-frozen' and review['independent_review']=='pending'
    expected_paths={str(p.relative_to(ROOT)) for p in CANDIDATE.rglob('*') if p.is_file() and p.name!='review-manifest.json'}
    expected_paths.update(['docs/design/CANONICAL-RECORDS-V2-CANDIDATE.md','docs/adr/candidates/outcome-records-v2.md','PHASE-2-CANONICAL-CANDIDATE.md','scripts/check-contracts.sh','scripts/check-candidate-contracts.sh'])
    expected_paths.update(str(p.relative_to(ROOT)) for p in Path(__file__).parent.iterdir() if p.suffix in ('.py','.mjs'))
    assert set(review['files'])==expected_paths, 'REVIEW_INVENTORY'
    for name,expected in review['files'].items():
        raw=(ROOT/name).read_bytes()
        assert expected=={'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()}, 'REVIEW_FILE_CHANGED:'+name
    constructed=files(); golden=CANDIDATE/'goldens'
    assert {p.name for p in golden.iterdir()}==set(constructed), 'GOLDEN_INVENTORY'
    histories=[]; records=0
    for name,expected in constructed.items():
        actual=(golden/name).read_bytes(); assert actual==expected, 'BYTE_RECONSTRUCTION:'+name
        parsed=strict(actual); assert canonical(parsed)==actual, 'CANONICAL_BYTES'
        if name not in ('vectors.json','inventory.json'):
            histories.append(parsed); records+=verify(parsed)
    reasons=strict((CANDIDATE/'reason-codes.json').read_bytes())
    codes=set().union(*(set(reasons[k]) for k in ('accepted','duplicate','conflict','rejected')))
    for h in histories:
        for p in h['probes']: assert p['expected'] in codes, 'UNKNOWN_REASON'
    # Duplicate keys, malformed encodings, nulls, precision and unsafe integers
    # are rejected before JSON schema or digest comparisons.
    malformed=[b'{"a":1,"a":2}',b'\xef\xbb\xbf{}',b'{"x":1.0}',b'{"x":1e0}',b'{"x":-0}',b'{"x":9007199254740992}',b'{"x":null}',b'{"x":"\\ud800"}',b'{"x":"\xff"}',b'{"x":NaN}']
    negative=0
    for raw in malformed:
        try: strict(raw)
        except (ValueError,UnicodeError): negative+=1
        else: raise AssertionError('malformed input accepted')
    # Rehash each corrupted row to show hash equality is not semantic validity.
    mutations=[('claim-revision','amount',{'currency':'USD','scale':2,'atoms':'2501'}),('action','book','supplier'),('action','amount',{'currency':'USD','scale':2,'atoms':'0'}),('admission','authorized_source','urn:attacker'),('admission','received_at','2026-09-23T12:00:00.000000Z'),('explanation','rounded_atoms','999'),('limit-evidence','after_premium','0'),('limit-evidence','maximum_premium','99999'),('receipt','action_ids',[]),('delivery-key','external_id','changed')]
    fixed=next(h for h in histories if h['name']=='fixed-success-fee')
    for kind,field,value in mutations:
        altered=copy.deepcopy(fixed); row=next(r for r in altered['decisions'][0]['records'] if r['kind']==kind)
        row['body'][field]=value; row['content_hash']=content_hash(kind,row['body'])
        try: verify(altered)
        except (AssertionError,ValueError,KeyError,jsonschema.ValidationError): negative+=1
        else: raise AssertionError('rehashed malformed record accepted: '+kind+'/'+field)
    # Missing prior family cannot be hidden by valid local arithmetic.
    multi=copy.deepcopy(next(h for h in histories if h['name']=='two-rule-families'))
    limit=next(r for r in multi['decisions'][1]['records'] if r['kind']=='limit-evidence')
    limit['body']['current_before']=[];limit['content_hash']=content_hash(limit['kind'],limit['body'])
    try: verify(multi)
    except AssertionError as e: assert str(e)=='INCOMPLETE_AGGREGATE'; negative+=1
    else: raise AssertionError('aggregate omission accepted')
    correction=copy.deepcopy(next(h for h in histories if h['name']=='correction-replacement'))
    rows=correction['decisions'][1]['records']
    inverse=next(r for r in rows if r['kind']=='action' and r['body']['slot']=='inverse')
    inverse['body']['amount']['atoms']='-2499'
    inverse['content_hash']=content_hash('action',inverse['body'])
    effect=next(r for r in rows if r['kind']=='effect' and r['body']['slot']=='inverse')
    effect['body']['facts_hash']=digest('effect-facts',effect_facts(inverse['body']))
    effect['content_hash']=content_hash('effect',effect['body'])
    try: verify(correction)
    except AssertionError as e: assert str(e)=='EXACT_INVERSE'; negative+=1
    else: raise AssertionError('inexact rehashed inverse accepted')
    stale=copy.deepcopy(next(h for h in histories if h['name']=='correction-replacement'))
    event=next(r for r in stale['decisions'][2]['records'] if r['kind']=='event')
    old=next(r for r in stale['decisions'][0]['records'] if r['kind']=='claim-revision')
    event['body']['data']['expected_revision']=old['id']; event['content_hash']=content_hash('event',event['body'])
    try: verify(stale)
    except AssertionError as e: assert str(e)=='STALE_CLAIM_REVISION'; negative+=1
    else: raise AssertionError('stale correction accepted')
    subprocess.run(['node',str(Path(__file__).with_name('check_hashes.mjs')),str(ROOT)],check=True)
    print(json.dumps({'status':'passed','candidate_not_frozen':True,'histories':len(histories),'records':records,'decisions':sum(len(h['decisions']) for h in histories),'negative_checks':negative,'record_kinds':len(SCHEMA['oneOf'])}))

if __name__=='__main__': main()
