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
                     ordered, reference, row_order, strict)
from reconstruct import CANDIDATE, ROOT, files
from semantics import freeze, window, bounded, exact_percentage
from scalars import Validator
from retained import validate_documents, resolved_evidence

SCHEMA=strict((CANDIDATE/'schemas/canonical-records.schema.json').read_bytes())
jsonschema.Draft202012Validator.check_schema(SCHEMA)
VALIDATOR=Validator(SCHEMA)
SET_FIELDS={'rules','evidence','postings','live_action_ids','inverse_action_ids','current_before','action_ids','inputs','depends_on','actions','intention_ids','families','bindings','limits','replacement_codes','sources','correction_sources','predecessors','verified_assents','verified_offers','verified_delegations','verified_evidence','event_types','allowed_modifiers','policy_evidence','identity_mappings'}

def check_order(value,name=''):
    if isinstance(value,dict):
        for k,v in value.items(): check_order(v,k)
    elif isinstance(value,list):
        if name in SET_FIELDS:
            assert value==ordered(value), 'SET_ORDER'
            assert len({canonical(v) for v in value})==len(value), 'SET_DUPLICATE'
        for v in value: check_order(v)

def checked_row(r):
    VALIDATOR.validate(r); check_order(r)
    assert all(len(s.encode())<=128 for s in r['scope']), 'SCOPE_BYTES'
    assert len(canonical(r['body']))<=262144, 'RECORD_BYTES'
    assert key(r['kind'],r['scope'],r['body'])==r['id'], 'RECORD_ID'
    assert content_hash(r['kind'],r['body'])==r['content_hash'], 'CONTENT_HASH'
    if r['kind']=='evidence': assert len(r['body']['utf8'].encode())<=262144, 'EVIDENCE_BYTES'
    if r['kind']=='policy-snapshot':
        b=r['body']; assert len({v['code'] for v in b['rules']})==len(b['rules']), 'OUTCOME_CODE_DUPLICATE'
        assert 'supplier_authorization' in b or b['book']=='retail', 'SUPPLIER_AUTHORITY'
        assert 'delegation' in b or b['roles']['bearer']==b['roles']['payer'], 'PAYER_DELEGATION'
        for w in (b['ordinary'],b['corrections']):
            assert w['starts_at']<w['occurs_before']<=w['received_by']<=w['accepted_by'], 'WINDOW_ORDER'
        assert int(b['max_premium_atoms'])>=0 and int(b['max_discount_atoms'])>=0, 'NEGATIVE_LIMIT'

def round_away(q):
    whole,rem=divmod(abs(q.numerator),q.denominator)
    return (-1 if q<0 else 1)*(whole+(2*rem>=q.denominator))

def verify(history, trusted_base=None):
    assert set(history)=={'status','name','target_admission','seed','decisions','probes'}
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
    if history['target_admission']=='rejected':
        assert not history['decisions'] and not any(r['kind']=='base-acceptance' for r in history['seed']), 'REJECTED_TARGET_RECEIPT'
        material=strict(next(r for r in history['seed'] if r['kind']=='base-evaluation')['body']['evaluation_utf8'].encode())
        assert any(r['operation']['kind']=='cap' for p in material['bundle']['policies'] for r in p['rules']), 'OUTCOME_CAP_COMPOSITION'
        assert all(p['expected']=='OUTCOME_CAP_COMPOSITION' and p['new_records']==[] for p in history['probes'])
        return len(known)
    frozen,retail,bindings,base_acceptance=freeze(history['seed'],lookup,deref)
    frozen_policies={v['id'] for v in frozen['body']['families']}
    for ix,decision in enumerate(history['decisions']):
        assert set(decision)=={'records','receipt_utf8'}
        rows=decision['records']
        assert not {r['kind'] for r in rows} & {'policy-snapshot','base-posting','target-basis','obligation','target-snapshot','base-evaluation','base-acceptance','binding-snapshot','base-identity'}, 'UNREFERENCED_NEW_SEED'
        prior_rows=list(known.values()); insert(rows)
        validate_documents(rows)
        def all_kind(k): return [r for r in rows if r['kind']==k]
        def one(k):
            rs=all_kind(k); assert len(rs)==1, 'RECORD_CARDINALITY:'+k; return rs[0]
        event=one('event'); eb=event['body']; data=eb['data']; E=event['id']
        revision=one('claim-revision'); rb=revision['body']; cl=lookup(rb['claim_id'],'claim'); cb=cl['body']
        prior=heads.get(cl['id']); correction=data['type']=='correction'
        assert correction==bool(prior), 'CLAIM_ALREADY_EXISTS'
        policy=lookup(rb['policy_snapshot'],'policy-snapshot'); pb=policy['body']; basis=lookup(rb['basis'],'target-basis'); bb=basis['body']
        assert policy['id'] in frozen_policies and rb['target_snapshot']==frozen['id'], 'FROZEN_MEMBERSHIP'
        assert rb['binding_id']==pb['binding_id'] and cb['book']==pb['book'], 'FAMILY_BINDING'
        data={**data,'book':pb['book'],'payer':pb['roles']['payer'],'code':data.get('code') if not correction else data['replacement'].get('code')}
        assert all(cb[k]==data[k] for k in ('target','agreement_id','family_id')), 'CLAIM_SLOT'
        assert all(pb[k]==data[k] for k in ('agreement_id','book','family_id')), 'POLICY_SLOT'
        assert basis['id']==retail['id'] and bb['target']==data['target'], 'RETAIL_BASIS'
        assert bindings[pb['binding_id']]['body']['roles']==pb['roles'], 'PAYER'
        assert pb['currency']==bb['amount']['currency'] and pb['scale']==bb['amount']['scale'], 'POLICY_MONEY_UNIT'
        assert bb['stage']=='uncapped', 'OUTCOME_CAP_COMPOSITION'
        assert bb['finality']=='final', 'TARGET_NOT_FINAL'
        assert bb['amount']['currency']==rb['amount']['currency'] and bb['amount']['scale']==rb['amount']['scale'], 'MONEY_UNIT'
        assert rb['event_id']==E and rb.get('code')==data['code'] and rb['state']==('reversed' if data['code'] is None else 'active'), 'REVISION_EVENT'
        assert rb['original_receipt']==cb['original_receipt'], 'ORIGINAL_RECEIPT'
        adm=one('admission'); ab=adm['body']
        assert all(int(ab[k])>=0 for k in ('credential_revision','grant_revision','target_guard_revision')), 'AUTHORITY_REVISION'
        assert rb['admission']==adm['id'] and ab['event_id']==E and ab['policy_snapshot']==policy['id'] and ab['basis']==basis['id'], 'ADMISSION_REFS'
        assert ab['permission']==data['type'] and ab['authorized_source']==data['source']==pb['correction_source' if correction else 'submission_source'], 'SOURCE_AUTHORITY'
        assert all(ab[k]==data[k] for k in ('agreement_id','payer','book','family_id')), 'ADMISSION_SCOPE'
        assert ab['target_snapshot']==frozen['id'] and ab['binding_id']==pb['binding_id'], 'ADMISSION_TARGET'
        assert ab['accepted_at']>=frozen['body']['accepted_at'], 'INVALID_ACCEPTED_ORDER'
        window(pb['corrections' if correction else 'ordinary'],data['occurred_at'],ab['received_at'],ab['accepted_at'])
        if correction:
            assert prior['id']==data['expected_revision']==rb['previous'] and data['claim_id']==cl['id'], 'STALE_CORRECTION'
            assert data['expected_revision_number']==prior['body']['number'], 'STALE_CORRECTION'
            assert (data['code'] is None and pb['allow_reversal']) or data['code'] in pb['replacement_codes'], 'CORRECTION_NOT_PERMITTED'
            assert int(rb['number'])==int(prior['body']['number'])+1, 'REVISION_SEQUENCE'
            assert rb['policy_snapshot']==prior['body']['policy_snapshot'] and rb['basis']==prior['body']['basis'], 'PIN_CHANGED'
            assert ab['accepted_at']>=lookup(prior['body']['admission'],'admission')['body']['accepted_at'], 'INVALID_ACCEPTED_ORDER'
            assert not all_kind('claim'), 'CLAIM_REINSERTED'
        else:
            assert rb['number']=='1' and 'previous' not in rb, 'INITIAL_REVISION'
            assert cb['first_event']==E and cb['facts_hash']==digest('claim-facts',facts(eb,lambda i:lookup(i,'evidence')['body']['document_id'])), 'CLAIM_FACTS'
            assert one('claim')['id']==cl['id']
        for proof,kind in [(ab['authentication'],'authentication'),(ab['grant'],'grant'),(pb['assent'],'assent')]:
            assert lookup(proof,'evidence')['body']['purpose']==kind, 'EVIDENCE_PURPOSE'
        requested_documents=resolved_evidence(data['evidence'],lookup)
        for proof in data['evidence']:
            lookup(proof,'evidence')  # Verified evidence may be reused by a correction.
        assert data['evidence'] or not pb['evidence_required'], 'OUTCOME_EVIDENCE_REQUIRED'
        authority=one('authority-decision'); au=authority['body']
        assert ab['authority_decision']==authority['id'] and au['event_id']==E, 'AUTHORITY_REFERENCE'
        assert all(au[k]==data[k] for k in ('target','agreement_id','family_id','source')), 'OUTCOME_AUTHORITY'
        assert au['active'] and au['may_read'] and au['may_correct' if correction else 'may_submit'], 'OUTCOME_AUTHORITY'
        assert all(au[k]==ab[k] for k in ('principal','grant','grant_revision','received_at','accepted_at')), 'AUTHORITY_OBSERVATIONS'
        verified_documents=resolved_evidence(au['verified_evidence'],lookup)
        assert set(requested_documents)<=set(verified_documents), 'OUTCOME_EVIDENCE_REQUIRED'
        for proof in au['verified_evidence']:lookup(proof,'evidence')
        # New immutable evidence must belong to this exact request/verification.
        # It cannot change the frozen target or carry operative decision fields.
        assert {r['id'] for r in all_kind('evidence')} <= set(data['evidence']), 'UNUSED_DECISION_EVIDENCE'
        assert all(r['id'] in au['verified_evidence'] for r in all_kind('evidence')), 'UNVERIFIED_DECISION_EVIDENCE'
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
            binding=bindings[pb['binding_id']]
            assert b['binding_id']==pb['binding_id'] and b['binding_snapshot']==binding['id'], 'ACTION_BINDING'
            assert b['component']==pb['family_id'], 'ACTION_COMPONENT'
            assert all(b[k]==binding['body'][k] for k in ('agreement_id','book','roles')), 'ACTION_BINDING_PARTIES'
            assert b['revision_id']==revision['id'] and b['claim_id']==cl['id'] and b['event_id']==E, 'ACTION_OWNER'
            assert b['book']==data['book'] and b['agreement_id']==data['agreement_id'] and b['family_id']==data['family_id'] and b['roles']==pb['roles'], 'ACTION_PARTIES'
            assert b['policy_snapshot']==policy['id'] and b['basis']==basis['id'], 'ACTION_PINS'
            assert b['amount']['currency']==rb['amount']['currency'] and b['amount']['scale']==rb['amount']['scale'], 'ACTION_UNIT'
            o=lookup(b['obligation_id'],'obligation')['body']
            assert all(o[k]==b[k] for k in ('agreement_id','book','roles')) and o['currency']==b['amount']['currency'] and o['scale']==b['amount']['scale'], 'OBLIGATION'
            effect=lookup(b['effect_id'],'effect')['body']
            assert effect['action_id']==a['id'] and effect['facts_hash']==digest('effect-facts',effect_facts(b)) and effect['revision_id']==revision['id'] and effect['claim_id']==cl['id'] and effect['slot']==b['slot'], 'EFFECT_FACTS'
            assert all(effect[k]==b[k] for k in ('binding_id','binding_snapshot','component')), 'EFFECT_BINDING'
            if b['slot']=='inverse':
                old=lookup(b['reverses'],'action')['body']; assert b['reverses'] not in reversed_actions, 'ALREADY_REVERSED'; reversed_actions.add(b['reverses'])
                assert int(b['amount']['atoms'])==-int(old['amount']['atoms']), 'EXACT_INVERSE'
                assert all(b[k]==old[k] for k in ('obligation_id','book','agreement_id','family_id','roles','policy_snapshot','basis','binding_id','binding_snapshot','component')), 'INVERSE_PROVENANCE'
            else: assert 'reverses' not in b, 'ORIGINAL_REVERSES'
        limits=one('limit-evidence'); lb=limits['body']; before=[v for v in heads.values() if all(lookup(v['body']['claim_id'],'claim')['body'][k]==cb[k] for k in ('target',)) and v['body']['binding_id']==pb['binding_id']]
        assert lb['binding_id']==pb['binding_id'] and lb['target_snapshot']==frozen['id'], 'LIMIT_SCOPE'
        assert lb['event_id']==E and all(lb[k]==cb[k] for k in ('target','agreement_id','book')), 'LIMIT_SCOPE'
        assert lb['currency']==bb['amount']['currency'] and lb['scale']==bb['amount']['scale'], 'LIMIT_UNIT'
        assert lb['current_before']==ordered([reference(v) for v in before]), 'INCOMPLETE_AGGREGATE'
        assert lb.get('replacing')==(prior['id'] if prior else None), 'LIMIT_REPLACING'
        after=[v for v in before if v['body']['claim_id']!=cl['id']]+[revision]
        for label,values in [('before',before),('after',after)]:
            amounts=[int(v['body']['amount']['atoms']) for v in values]
            assert int(lb[label+'_premium'])==bounded(sum(max(n,0) for n in amounts)) and int(lb[label+'_discount'])==bounded(sum(max(-n,0) for n in amounts)), 'LIMIT_TOTAL'
        assert lb['maximum_premium']==pb['max_premium_atoms'] and lb['maximum_discount']==pb['max_discount_atoms'], 'LIMIT_PIN'
        assert int(lb['after_discount'])<=int(lb['maximum_discount']), 'DISCOUNT_EXCEEDS_BASIS'
        assert int(lb['after_premium'])<=int(lb['maximum_premium']), 'PREMIUM_LIMIT'
        bounded(int(lb['maximum_discount'])-int(lb['after_discount'])+int(lb['after_premium']))
        # All families on this target/book pin the SAME aggregate limits.
        for v in before:
            oldp=lookup(v['body']['policy_snapshot'],'policy-snapshot')['body']
            assert all(oldp[k]==pb[k] for k in ('max_premium_atoms','max_discount_atoms')), 'LIMIT_TERMS_MISMATCH'
        explanations=sorted(all_kind('explanation'),key=lambda x:x['body']['ordinal'])
        assert len(explanations)==(2 if correction else 1), 'EXPLANATION_COUNT'
        rule=next((r for r in pb['rules'] if r['code']==data['code']),None)
        assert rule or data['code'] is None, 'POLICY_OUTCOME_CODE'
        exact=Fraction(0) if rule is None else (Fraction(int(rule['fixed_atoms'])) if rule['kind']=='fixed' else exact_percentage(int(bb['amount']['atoms']),rule['rate']))
        assert max(abs(exact.numerator).bit_length(),exact.denominator.bit_length())<=512 and bounded(round_away(exact))==amount, 'EXPLANATION_MATH'
        expected_x=[]
        if prior: expected_x.append(('EXACT_REVERSAL',Fraction(-int(prior['body']['amount']['atoms'])),rb['inverse_action_ids']))
        expected_x.append(('CLAIM_REVERSED' if data['code'] is None else ('ZERO_ROUNDED' if not amount else 'OUTCOME_APPLIED'),exact,rb['live_action_ids']))
        for ordinal,(xp,(code,q,aids)) in enumerate(zip(explanations,expected_x)):
            xb=xp['body']
            assert xb['ordinal']==ordinal and xb['code']==code and xb['basis_amount']==bb['amount'], 'EXPLANATION_CODE'
            assert xb['unrounded_atoms']=={'numerator':str(q.numerator),'denominator':str(q.denominator)} and xb['rounded_atoms']==str(round_away(q)), 'EXPLANATION_MATH'
            assert xb['event_id']==E and xb['claim_id']==cl['id'] and xb['revision_id']==revision['id'] and xb['limit_evidence']==limits['id'] and xb['policy_snapshot']==policy['id'] and xb['basis']==basis['id'], 'EXPLANATION_REFS'
            assert xb['action_ids']==aids, 'EXPLANATION_ACTIONS'
            assert resolved_evidence(xb['evidence'],lookup)==requested_documents, 'EXPLANATION_EVIDENCE'
            assert xb['authority_decision']==authority['id'] and xb['evidence']==data['evidence'], 'EXPLANATION_EVIDENCE'
        deps=[]
        for a in actions:
            deps.append(dict(dependent=a['id'],input=reference(basis),reason='frozen_basis'))
            if a['body']['slot']=='inverse': deps.append(dict(dependent=a['id'],input=reference(lookup(a['body']['reverses'],'action')),reason='exact_inverse'))
        for xp in explanations:
            deps += [dict(dependent=xp['id'],input=reference(basis),reason='frozen_basis'),dict(dependent=xp['id'],input=reference(limits),reason='aggregate_limit')]
            if prior: deps.append(dict(dependent=xp['id'],input=reference(prior),reason='prior_revision'))
        assert ordered([{k:v for k,v in d['body'].items() if k!='schema'} for d in all_kind('dependency')])==ordered(deps), 'DEPENDENCY_CLOSURE'
        replay=one('replay-input'); replayb=replay['body']
        assert replayb['target_snapshot']==frozen['id'] and replayb['authority_decision']==authority['id'], 'REPLAY_VERIFICATION'
        assert replayb['event_id']==E and replayb['received_at']==ab['received_at'] and replayb['accepted_at']==ab['accepted_at'], 'REPLAY_CONTEXT'
        assert replayb['inputs']==ordered([reference(r) for r in prior_rows]+[reference(event),reference(adm),reference(authority)]+[reference(r) for r in all_kind('evidence')]), 'REPLAY_INCOMPLETE'
        for v in replayb['inputs']: deref(v)
        net=bounded(sum(int(a['body']['amount']['atoms']) for a in actions)); intents=all_kind('intention')
        assert len(intents)==(net!=0), 'INTENTION_CARDINALITY'
        if intents:
            i=intents[0]; ib=i['body']; assert ib['amount']=={**rb['amount'],'atoms':str(net)} and ib['event_id']==E and ib['idempotency_key']==i['id'], 'INTENTION_AMOUNT'
            assert ib['action_ids']==ordered([a['id'] for a in actions]) and all(a['body']['obligation_id']==ib['obligation_id'] for a in actions), 'INTENTION_ACTIONS'
            assert ib['depends_on']==ordered(claim_intents.get(cl['id'],[])), 'EXPORT_DEPENDENCIES'
            assert ib['payload']=={'schema':'ledger-obligation-delta/2-candidate.4','obligation_id':ib['obligation_id'],'amount':ib['amount'],'actions':ordered([{'action_id':a['id'],'amount':a['body']['amount']} for a in actions])}, 'INTENTION_PAYLOAD'
            claim_intents.setdefault(cl['id'],[]).append(i['id'])
        delivery=one('delivery-key')['body']; assert delivery['event_id']==E and delivery['ingress']==eb and delivery['ingress_hash']==digest('ingress',eb) and delivery['source']==data['source'] and delivery['external_id']==data['external_id'], 'DELIVERY_BYTES'
        manifest=one('decision-manifest'); mb=manifest['body']; receipt=one('receipt'); recb=receipt['body']; chain=one('chain-revision')['body']
        assert chain['number']==str(ix+1)==mb['revision']==recb['revision'] and chain['event_id']==E and chain['decision_id']==manifest['id'], 'CHAIN_REVISION'
        assert all(b['chain_id']==data['chain_id'] for b in (chain,mb,recb)) and data['chain_id']==history['name'], 'CHAIN_ID'
        assert mb['event_id']==E and mb['accepted_at']==ab['accepted_at'] and mb['explanation_ids']==[xp['id'] for xp in explanations], 'MANIFEST_CONTEXT'
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
        probe_evidence={r['id']:r for r in p.get('evidence_records',[])}
        for r in probe_evidence.values():checked_row(r)
        validate_documents(list(probe_evidence.values()))
        def probe_lookup(i,kind=None):return probe_evidence[i] if i in probe_evidence else lookup(i,kind)
        if 'candidate' in p:
            resolved_evidence(p['candidate']['data']['evidence'],probe_lookup)
            Validator({'$ref':'#/$defs/event','$defs':SCHEMA['$defs']}).validate(p['candidate'])
        if p['kind']=='evidence_wrapper_retry':
            original=next(r for r in history['decisions'][p['step']]['records'] if r['kind']=='event')
            assert facts(p['candidate'],lambda i:probe_lookup(i,'evidence')['body']['document_id'])==facts(original['body'],lambda i:lookup(i,'evidence')['body']['document_id']), 'RETRY_DOCUMENT_FACTS'
            assert p['expected']=='DUPLICATE_CLAIM' and p['original_receipt_utf8']==history['decisions'][p['step']]['receipt_utf8'], 'RETRY_RECEIPT'
            continue
        if p['kind'] in ('identity_retry','semantic_retry','changed_facts','stale_correction'):
            original=next(r for r in history['decisions'][p['step']]['records'] if r['kind']=='event')
            cb=p['candidate']['data']
            if p['kind']=='identity_retry': assert canonical(p['candidate'])==canonical(original['body']) and p['expected']=='DUPLICATE_IDENTITY'
            if p['kind']=='semantic_retry': assert facts(p['candidate'],lambda i:lookup(i,'evidence')['body']['document_id'])==facts(original['body'],lambda i:lookup(i,'evidence')['body']['document_id']) and cb['external_id']!=original['body']['data']['external_id'] and p['expected']=='DUPLICATE_CLAIM'
            if p['kind']=='changed_facts': assert facts(p['candidate'],lambda i:lookup(i,'evidence')['body']['document_id'])!=facts(original['body'],lambda i:lookup(i,'evidence')['body']['document_id']) and p['expected']=='CLAIM_CONFLICT'
            if p['kind']=='stale_correction': assert cb['expected_revision']!=heads[cb['claim_id']]['id'] and p['expected']=='STALE_CORRECTION'
        if p['kind'].endswith('_retry'):
            checked_row(p['current_policy'])
            assert p['current_policy']['body']['policy_version']=='2'
            assert p['original_receipt_utf8']==history['decisions'][p['step']]['receipt_utf8'], 'RETRY_RECEIPT'
            assert p['current_policy_version']=='2', 'RETRY_VERSION_PROBE'
        if p['kind']=='aggregate_limit':
            assert p['current_before']==ordered([reference(v) for v in heads.values()])
            proposed=int(p['proposed_atoms']); pb=lookup(p['policy_snapshot'],'policy-snapshot')['body']
            assert next(r['fixed_atoms'] for r in pb['rules'] if r['code']=='success')==p['proposed_atoms']
            assert sum(max(0,int(v['body']['amount']['atoms'])) for v in heads.values())+max(proposed,0)>int(pb['max_premium_atoms']) and p['expected']=='PREMIUM_LIMIT'
        if p['kind'] in ('timing','correction_timing'):
            pb=lookup(p['policy_snapshot'],'policy-snapshot')['body']
            try: window(pb['corrections' if p['kind']=='correction_timing' else 'ordinary'],p['candidate']['data']['occurred_at'],p['received_at'],p['accepted_at'])
            except AssertionError as e: assert str(e)==p['expected']
            else: raise AssertionError('timing rejection expected')
        if p['kind']=='cap': assert lookup(p['basis'],'target-basis')['body']['stage']==p['stage']=='capped' and p['expected']=='OUTCOME_CAP_COMPOSITION'
    assert trusted_base is not None, 'BASE_TRUST_REQUIRED'
    assert reference(base_acceptance)==trusted_base, 'BASE_ACCEPTANCE_CHANGED'
    return len(known)

def main():
    # Approval/status is external metadata. Preserve every reviewed contract byte
    # and the historical candidate labels rather than changing hash preimages.
    import importlib.util
    spec=importlib.util.spec_from_file_location('outcome_freeze',Path(__file__).parent.parent/'check_outcome_freeze.py')
    freeze_status=importlib.util.module_from_spec(spec);spec.loader.exec_module(freeze_status)
    freeze_status.main()
    constructed=files(); golden=CANDIDATE/'goldens'
    assert {p.name for p in golden.iterdir()}==set(constructed), 'GOLDEN_INVENTORY'
    histories=[]; records=0
    for name,expected in constructed.items():
        actual=(golden/name).read_bytes(); assert actual==expected, 'BYTE_RECONSTRUCTION:'+name
        parsed=strict(actual); assert canonical(parsed)==actual, 'CANONICAL_BYTES'
        if name not in ('vectors.json','inventory.json'):
            histories.append(parsed)
            trusted=next((reference(r) for r in strict(expected)['seed'] if r['kind']=='base-acceptance'),None)
            records+=verify(parsed,trusted)
    reasons=strict((CANDIDATE/'reason-codes.json').read_bytes())
    codes=set().union(*(set(reasons[k]) for k in ('accepted','duplicate','conflict','rejected')))
    for h in histories:
        for p in h['probes']: assert p['expected'] in codes, 'UNKNOWN_REASON'
    # Duplicate keys, malformed encodings, nulls, precision and unsafe integers
    # are rejected before JSON schema or digest comparisons.
    malformed=[b'{"a":1,"a":2}',b'\xef\xbb\xbf{}',b'{"x":1.0}',b'{"x":1e0}',b'{"x":-0}',b'{"x":9007199254740992}',b'{"x":null}',b'{"x":"\\ud800"}',b'{"x":"\xff"}',b'{"x":NaN}']
    assert exact_percentage(2**29,{'numerator':'1','denominator':str(2**511)})==Fraction(1,25*2**484), 'REDUCED_PERCENT_INTERMEDIATE'
    negative=0
    for raw in malformed:
        try: strict(raw)
        except (ValueError,UnicodeError): negative+=1
        else: raise AssertionError('malformed input accepted')
    from boundaries import run_boundaries
    negative+=run_boundaries(histories,SCHEMA,checked_row,verify)
    from unicode_parity import run_unicode_parity
    run_unicode_parity(histories,checked_row,verify)
    from adversarial import run_adversarial
    negative+=run_adversarial(histories,verify)
    subprocess.run(['node',str(Path(__file__).with_name('check_hashes.mjs')),str(ROOT)],check=True)
    print(json.dumps({'status':'passed','contract_frozen':True,'profile':'2-candidate.4','histories':len(histories),'records':records,'decisions':sum(len(h['decisions']) for h in histories),'negative_checks':negative,'record_kinds':len(SCHEMA['oneOf'])}))

if __name__=='__main__': main()
