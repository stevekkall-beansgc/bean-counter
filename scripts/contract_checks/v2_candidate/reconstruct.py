"""Construct candidate golden bytes from explicit literals; never evaluates policies.

This is an offline contract authorship/reconstruction tool, not a product oracle.
Audit mode compares immutable files. --write is explicit and writes candidate files only.
"""
import argparse
import hashlib
import json
from pathlib import Path
from profile import (body, canonical, content_hash, digest, effect_facts, envelope,
                     facts, ident, key, key_input, ordered, reference, row_order, strict)

ROOT=Path(__file__).resolve().parents[3]
CANDIDATE=ROOT/'contracts/candidates/v2'
T='2026-09-21T12:00:00.000000Z'
S=['synthetic','sandbox']

def money(atoms): return {'currency':'USD','scale':2,'atoms':str(atoms)}
def ratio(pair): return dict(zip(['numerator','denominator'],pair))
def roles(book):
    payer='customer' if book=='retail' else 'host'
    provider='host' if book=='retail' else 'supplier'
    return dict(provider=provider,cost_originator=provider,bearer=payer,payer=payer,beneficiary=payer,recipient=provider)

def build(spec):
    seed=[]; decisions=[]; known={}; vectors=[]; heads={}; policies={}; targets={}; bases={}; obligations={}; claim_intentions={}
    def add(kind,fields,dest):
        b=body(kind,**fields); r=envelope(kind,S,b); dest.append(r); known[canonical(r['id'])]=r
        vectors.append({'kind':kind,'id':r['id'],'id_input':key_input(kind,S,b),'body_utf8':canonical(b).decode(),'content_hash':r['content_hash']})
        return r
    def evidence(purpose):
        return add('evidence',dict(purpose=purpose,media_type='text/plain;charset=utf-8',utf8='synthetic:'+spec['name']+':'+purpose),seed)
    proofs={p:evidence(p) for p in ['assent','grant','authentication','finality','outcome','correction','supplier_authorization']}
    for book,n in [('retail',spec['base'])]+([('supplier',spec['supplier_base'])] if 'supplier_base' in spec else []):
        agreement='agreement-'+book
        e=targets.get('retail')
        if e is None:
            e=add('event',dict(data=dict(type='base',source='urn:synthetic:work',external_id=spec['name']+'-base',chain_id=spec['name'],customer='customer',occurred_at=T,evidence=[],operation_id='base',status='succeeded',quantity='1',unit='call')),seed)
        p=add('base-posting',dict(event_id=e['id'],agreement_id=agreement,book=book,ordinal=0,amount=money(n),roles=roles(book)),seed)
        b=add('target-basis',dict(target=e['id'],agreement_id=agreement,book=book,payer=roles(book)['payer'],amount=money(n),postings=[reference(p)],finality='final',finality_evidence=proofs['finality']['id'],stage=spec.get('stage','uncapped')),seed)
        o=add('obligation',dict(agreement_id=agreement,book=book,currency='USD',scale=2,roles=roles(book)),seed)
        targets[book]=e; bases[book]=b; obligations[book]=o
    for step in (spec['steps'] or [dict(family='success',fixed='2500')])+spec.get('additional_policies',[]):
        f=step['family']; book=step.get('book','retail')
        if f in policies: continue
        rule={'code':'rebate','kind':'percentage','rate':ratio(step['rate'])} if 'rate' in step else {'code':'success','kind':'fixed','fixed_atoms':step['fixed']}
        fields=dict(agreement_id='agreement-'+book,currency='USD',scale=2,book=book,family_id=f,policy_version='1',rules=ordered([rule,dict(code='none',kind='fixed',fixed_atoms='0')]+step.get('extra_rules',[])),rounding='nearest_ties_away',basis_kind='original_booked_net',roles=roles(book),assent=proofs['assent']['id'],submission_source='urn:synthetic:outcome',correction_source='urn:synthetic:correction',window_start=T,window_end='2026-09-22T12:00:00.000000Z',report_before='2026-09-23T12:00:00.000000Z',correct_before='2026-09-24T12:00:00.000000Z',max_premium_atoms=spec.get('maximum_premium','10000'),max_discount_atoms=spec.get('supplier_base',spec['base']) if book=='supplier' else spec['base'])
        if book=='supplier': fields['supplier_authorization']=proofs['supplier_authorization']['id']
        policies[f]=add('policy-snapshot',fields,seed)
    for index,step in enumerate(spec['steps']):
        rows=[]; f=step['family']; book=step.get('book','retail'); policy=policies[f]; basis=bases[book]; target=targets[book]; obligation=obligations[book]
        agreement='agreement-'+book; correcting=step.get('correct',False); prior=heads.get(f)
        timestamp=f'2026-09-21T13:{index:02d}:00.000000Z'
        # Revision and posting amounts below are explicit history-input literals.
        data=dict(type='correction' if correcting else 'outcome',source='urn:synthetic:correction' if correcting else 'urn:synthetic:outcome',external_id=spec['name']+'-'+str(index+1),chain_id=spec['name'],payer=roles(book)['payer'],occurred_at='2026-09-21T12:30:00.000000Z',evidence=[proofs['correction' if correcting else 'outcome']['id']],target=target['id'],agreement_id=agreement,book=book,family_id=f,code=step['code'])
        if correcting:
            data.update(claim_id=prior['body']['claim_id'],expected_revision=prior['id'],corrected_at=timestamp)
        event=add('event',dict(data=data),rows); E=event['id']; R=ident('receipt',[E]); D=ident('decision-manifest',[E])
        clid=ident('claim',[S,target['id'],agreement,book,f]); number=str(int(prior['body']['number'])+1) if prior else '1'; rid=ident('claim-revision',[clid,number])
        admission=add('admission',dict(event_id=E,principal='synthetic-authorized-principal',credential_revision='1',authentication=proofs['authentication']['id'],grant=proofs['grant']['id'],grant_revision='1',target_guard_revision='1',target_state='final_unreversed',aggregate_guard_revision=str(index),authorized_source=data['source'],agreement_id=agreement,payer=data['payer'],book=book,family_id=f,permission=data['type'],received_at=timestamp,accepted_at=timestamp,decision='allow',policy_snapshot=policy['id'],basis=basis['id']),rows)
        if not prior:
            claim=add('claim',dict(target=target['id'],agreement_id=agreement,book=book,family_id=f,first_event=E,original_receipt=R,facts_hash=digest('claim-facts',facts(event['body']))),rows)
        else: claim=known[canonical(clid)]
        add('link',dict(event_id=E,target=target['id'],relation='outcome_of'),rows)
        actions=[]; replacements=[]; inverses=[]
        def action(slot,atoms,reverse=None):
            efid=ident('effect',[clid,rid,slot])
            fields=dict(event_id=E,effect_id=efid,revision_id=rid,claim_id=clid,slot=slot,amount=money(atoms),obligation_id=obligation['id'],book=book,agreement_id=agreement,family_id=f,roles=roles(book),policy_snapshot=policy['id'],basis=basis['id'])
            if reverse: fields['reverses']=reverse['id']
            a=add('action',fields,rows); actions.append(a)
            add('effect',dict(claim_id=clid,revision_id=rid,slot=slot,action_id=a['id'],facts_hash=digest('effect-facts',effect_facts(a['body']))),rows)
            add('dependency',dict(dependent=a['id'],input=reference(basis),reason='frozen_basis'),rows)
            if reverse: add('dependency',dict(dependent=a['id'],input=reference(reverse),reason='exact_inverse'),rows)
            return a['id']
        if prior:
            for aid in prior['body']['live_action_ids']:
                original=known[canonical(aid)]
                inverses.append(action('inverse',str(-int(original['body']['amount']['atoms'])),original))
        if step['atoms']!='0': replacements.append(action('replacement',step['atoms']))
        revision_fields=dict(claim_id=clid,number=number,event_id=E,code=step['code'],policy_snapshot=policy['id'],basis=basis['id'],admission=admission['id'],live_action_ids=ordered(replacements),inverse_action_ids=ordered(inverses),amount=money(step['atoms']),original_receipt=claim['body']['original_receipt'])
        if prior: revision_fields['previous']=prior['id']
        rev=add('claim-revision',revision_fields,rows)
        before=[v for v in heads.values() if known[canonical(v['body']['basis'])]['body']['book']==book]
        after=[v for v in before if v['body']['claim_id']!=clid]+[rev]
        def totals(revs):
            amounts=[int(r['body']['amount']['atoms']) for r in revs]
            return str(sum(max(v,0) for v in amounts)),str(sum(max(-v,0) for v in amounts))
        bp,bd=totals(before); ap,ad=totals(after)
        lf=dict(event_id=E,target=target['id'],agreement_id=agreement,book=book,currency='USD',scale=2,current_before=ordered([reference(v) for v in before]),before_premium=bp,before_discount=bd,after_premium=ap,after_discount=ad,maximum_premium=policy['body']['max_premium_atoms'],maximum_discount=policy['body']['max_discount_atoms'])
        if prior: lf['replacing']=prior['id']
        limit=add('limit-evidence',lf,rows)
        xp=add('explanation',dict(event_id=E,ordinal=0,claim_id=clid,revision_id=rid,code=('CORRECTION_ZERO' if step['atoms']=='0' else 'CORRECTION_REPLACED') if correcting else ('ZERO_ADJUSTMENT' if step['atoms']=='0' else ('PERCENTAGE_APPLIED' if 'rate' in step else 'FIXED_APPLIED')),policy_snapshot=policy['id'],basis=basis['id'],unrounded_atoms=ratio(step['exact']),rounded_atoms=step['atoms'],rounding='nearest_ties_away',action_ids=ordered([a['id'] for a in actions]),limit_evidence=limit['id']),rows)
        add('dependency',dict(dependent=xp['id'],input=reference(basis),reason='frozen_basis'),rows)
        add('dependency',dict(dependent=xp['id'],input=reference(limit),reason='aggregate_limit'),rows)
        if prior: add('dependency',dict(dependent=xp['id'],input=reference(prior),reason='prior_revision'),rows)
        # Complete retained economic inputs: all seed records plus all previous
        # canonical records (bounded); no recursive expansion from output rows.
        replay_refs=ordered([reference(r) for r in seed]+[reference(r) for d in decisions for r in d['records']]+[reference(event),reference(admission)])
        replay=add('replay-input',dict(event_id=E,semantics='ledger-outcome-semantics/2-candidate.1',inputs=replay_refs,received_at=timestamp,accepted_at=timestamp),rows)
        net=sum(int(a['body']['amount']['atoms']) for a in actions); intent_ids=[]
        if net:
            aids=ordered([a['id'] for a in actions]); iid=ident('intention',[S,'fake',obligation['id'],aids]); deps=[]
            deps=claim_intentions.get(f,[])
            intent=add('intention',dict(event_id=E,destination='fake',obligation_id=obligation['id'],action_ids=aids,amount=money(net),depends_on=ordered(list(set(deps))),idempotency_key=iid,payload=body('obligation-delta',obligation_id=obligation['id'],amount=money(net),actions=ordered([dict(action_id=a['id'],amount=a['body']['amount']) for a in actions]))),rows)
            intent_ids=[intent['id']]
            claim_intentions.setdefault(f,[]).append(intent['id'])
        add('delivery-key',dict(source=data['source'],external_id=data['external_id'],event_id=E,ingress=event['body'],ingress_hash=digest('ingress',event['body']),original_receipt=R),rows)
        add('chain-revision',dict(chain_id=spec['name'],number=str(index+1),event_id=E,decision_id=D),rows)
        # All retained replay inputs, all new rows except manifest/receipt; unique
        # member identity, sorted by kind and canonical ID bytes.
        members={canonical([r['kind'],r['id']]):reference(r) for r in rows}
        members.update({canonical([r['kind'],r['id']]):r for r in replay_refs})
        manifest=add('decision-manifest',dict(event_id=E,chain_id=spec['name'],revision=str(index+1),accepted_at=timestamp,members=sorted(members.values(),key=row_order),explanation_ids=[xp['id']]),rows)
        receipt=add('receipt',dict(event_id=E,decision_id=D,chain_id=spec['name'],revision=str(index+1),accepted_at=timestamp,event_hash=event['content_hash'],decision_hash=manifest['content_hash'],claim_id=clid,claim_revision=rid,action_ids=ordered([a['id'] for a in actions]),intention_ids=intent_ids),rows)
        decisions.append({'records':sorted(rows,key=row_order),'receipt_utf8':canonical(receipt['body']).decode()})
        heads[f]=rev
    probes=[]
    for probe in spec.get('probes',[]):
        p={**probe,'new_records':[]}
        if probe['kind'] in ('timing','correction_timing','cap'):
            # A complete rejected outcome candidate plus retained gate context.
            p['policy_snapshot']=policies['success']['id']
            p['basis']=bases['retail']['id']
            p['candidate']=body('event',data=dict(type='outcome',source='urn:synthetic:outcome',external_id=spec['name']+'-rejected-'+str(len(probes)+1),chain_id=spec['name'],payer='customer',occurred_at=probe.get('occurred_at','2026-09-21T12:30:00.000000Z'),evidence=[proofs['outcome']['id']],target=targets['retail']['id'],agreement_id='agreement-retail',book='retail',family_id='success',code='success'))
        if probe['kind']=='aggregate_limit':
            p['policy_snapshot']=policies[probe['family']]['id']; p['basis']=bases['retail']['id']
            p['current_before']=ordered([reference(v) for v in heads.values()])
            p['candidate']=body('event',data=dict(type='outcome',source='urn:synthetic:outcome',external_id=spec['name']+'-rejected',chain_id=spec['name'],payer='customer',occurred_at='2026-09-21T12:30:00.000000Z',evidence=[proofs['outcome']['id']],target=targets['retail']['id'],agreement_id='agreement-retail',book='retail',family_id=probe['family'],code='success'))
        if probe['kind']=='correction_timing':
            p['candidate']['data'].update(type='correction',source='urn:synthetic:correction',code='none',claim_id=heads['success']['body']['claim_id'],expected_revision=heads['success']['id'],corrected_at=probe['corrected_at'],evidence=[proofs['correction']['id']])
            p['received_at']=probe['corrected_at']
        if probe['kind'] in ('identity_retry','semantic_retry','changed_facts','stale_correction'):
            original=next(r for r in decisions[probe['step']]['records'] if r['kind']=='event')
            p['candidate']={'schema':original['body']['schema'],'data':dict(original['body']['data'])}
            if probe['kind']!='identity_retry': p['candidate']['data']['external_id']+='-retry'
            if probe['kind']=='changed_facts': p['candidate']['data']['code']='none'
        if probe['kind'] in ('identity_retry','semantic_retry'):
            p['original_receipt_utf8']=decisions[probe['step']]['receipt_utf8']
            original_policy=policies[p['candidate']['data']['family_id']]
            updated={**original_policy['body'],'policy_version':'2','rules':[dict(r) for r in original_policy['body']['rules']]}
            for rule in updated['rules']:
                if rule['kind']=='fixed' and rule['fixed_atoms']!='0': rule['fixed_atoms']=str(2*int(rule['fixed_atoms']))
            p['current_policy']=envelope('policy-snapshot',S,updated)

        probes.append(p)
    result={'status':'candidate-not-frozen','name':spec['name'],'seed':sorted(seed,key=row_order),'decisions':decisions,'probes':probes}
    return result,vectors

def files():
    inputs=strict((CANDIDATE/'history-inputs.json').read_bytes())
    result={}; all_vectors=[]
    for spec in inputs['histories']:
        value,vectors=build(spec)
        result[spec['name']+'.json']=canonical(value)
        all_vectors.extend({'history':spec['name'],**v} for v in vectors)
    result['vectors.json']=canonical({'status':'candidate-not-frozen','vectors':all_vectors})
    result['inventory.json']=canonical({'status':'candidate-not-frozen','files':{k:{'bytes':len(v),'sha256':hashlib.sha256(v).hexdigest()} for k,v in result.items()}})
    return result

def write_review_inventory():
    paths={p for p in CANDIDATE.rglob('*') if p.is_file() and p.name!='review-manifest.json'}
    paths.update(ROOT/p for p in ['docs/design/CANONICAL-RECORDS-V2-CANDIDATE.md',
        'docs/adr/candidates/outcome-records-v2.md','PHASE-2-CANONICAL-CANDIDATE.md',
        'scripts/check-contracts.sh','scripts/check-candidate-contracts.sh'])
    paths.update(p for p in Path(__file__).parent.iterdir() if p.suffix in ('.py','.mjs'))
    value={'status':'candidate-not-frozen','profile':'2-candidate.1',
        'independent_review':'pending','base_commit':'b35258425970052ed71481eca1f33ef857c61be1',
        'files':{str(p.relative_to(ROOT)):{'bytes':p.stat().st_size,
            'sha256':hashlib.sha256(p.read_bytes()).hexdigest()} for p in sorted(paths)}}
    (CANDIDATE/'review-manifest.json').write_text(json.dumps(value,indent=2)+'\n')

if __name__=='__main__':
    parser=argparse.ArgumentParser()
    parser.add_argument('--write',action='store_true')
    parser.add_argument('--write-review-inventory',action='store_true')
    args=parser.parse_args()
    for name,raw in files().items():
        path=CANDIDATE/'goldens'/name
        if args.write: path.write_bytes(raw)
        else: assert path.read_bytes()==raw, 'candidate reconstruction mismatch: '+name
    if args.write_review_inventory: write_review_inventory()
    print('candidate Python reconstruction: passed' if not args.write else 'candidate files authored; not frozen')
