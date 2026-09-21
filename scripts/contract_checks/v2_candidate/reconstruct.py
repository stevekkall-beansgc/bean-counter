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
    seed=[]; decisions=[]; known={}; vectors=[]; heads={}; policies={}; targets={}; bases={}; bindings={}; obligations={}; claim_intentions={}
    def add(kind,fields,dest):
        b=body(kind,**fields); r=envelope(kind,S,b); dest.append(r); known[canonical(r['id'])]=r
        vectors.append({'kind':kind,'id':r['id'],'id_input':key_input(kind,S,b),'body_utf8':canonical(b).decode(),'content_hash':r['content_hash']})
        return r
    def evidence(purpose):
        return add('evidence',dict(purpose=purpose,media_type='text/plain;charset=utf-8',utf8='synthetic:'+spec['name']+':'+purpose),seed)
    proofs={p:evidence(p) for p in ['assent','grant','authentication','finality','outcome','correction','supplier_authorization','offer','base_bundle','base_context']}
    for book,n in [('retail',spec['base'])]+([('supplier',spec['supplier_base'])] if 'supplier_base' in spec else []):
        agreement='agreement-'+book
        e=targets.get('retail')
        if e is None:
            e=add('event',dict(data=dict(type='base',source='urn:synthetic:work',external_id=spec['name']+'-base',chain_id=spec['name'],customer='customer',work_type='tool.completed' if 'supplier_base' in spec else 'content.generated',occurred_at=T,evidence=[],operation_id='base',status='succeeded',quantity='1',unit='call')),seed)
        postings=[]
        amounts=[spec.get('base_gross',n),spec['base_discount']] if book=='retail' and 'base_discount' in spec else [n]
        for ordinal,atoms in enumerate(amounts):
            if atoms!='0': postings.append(add('base-posting',dict(event_id=e['id'],agreement_id=agreement,book=book,binding_id='binding-'+book,ordinal=ordinal,amount=money(atoms),roles=roles(book)),seed))
        binding=dict(unit='call',maximum_quantity='1',event_types=[e['body']['data']['work_type']],allowed_modifiers=[],allocation_view=False,binding_id='binding-'+book,agreement_id=agreement,book=book,roles=roles(book),assent=proofs['assent']['id'],sources=['urn:synthetic:outcome','urn:synthetic:work'],correction_sources=['urn:synthetic:correction'],booked_net=money(n))
        if book=='supplier':
            binding.update(offer=proofs['offer']['id'],maximum_exposure=money(spec.get('supplier_exposure','15000')),supplier_invocation=dict(id='invocation-supplier',binding_id='binding-supplier',operation_id='base',chain=spec['name'],customer='customer',source='urn:synthetic:work',unit='call',maximum_quantity='1',maximum_exposure=money(spec.get('supplier_exposure','15000')),held=money(spec.get('supplier_held','15000')),authorized_at=T,start_before='2026-09-21T12:01:00.000000Z',attested_start=T,outcome_deadline='2026-09-25T13:00:00.000000Z'))
        bindings[book]=add('binding-snapshot',binding,seed)
        b=add('target-basis',dict(target=e['id'],agreement_id=agreement,book=book,payer=roles(book)['payer'],amount=money(n),postings=ordered([reference(p) for p in postings]),finality='final',finality_evidence=proofs['finality']['id'],stage=spec.get('stage','uncapped')),seed)
        o=add('obligation',dict(agreement_id=agreement,book=book,currency='USD',scale=2,roles=roles(book)),seed)
        targets[book]=e; bases[book]=b; obligations[book]=o
    for step in (spec['steps'] or [dict(family='success',fixed='2500')])+spec.get('additional_policies',[]):
        f=step['family']; book=step.get('book','retail')
        if f in policies: continue
        rule={'code':'rebate','kind':'percentage','rate':ratio(step['rate'])} if 'rate' in step else {'code':'success','kind':'fixed','fixed_atoms':step['fixed']}
        fields=dict(agreement_id='agreement-'+book,currency='USD',scale=2,book=book,family_id=f,policy_version='1',rules=ordered([rule,dict(code='none',kind='fixed',fixed_atoms='0')]+step.get('extra_rules',[])),rounding='nearest_ties_away',basis_kind='frozen_target_retail_net',binding_id='binding-'+book,evidence_required=True,replacement_codes=ordered(step.get('replacements',[r['code'] for r in [rule,dict(code='none')]+step.get('extra_rules',[])])),allow_reversal=step.get('allow_reversal',True),roles=roles(book),assent=proofs['assent']['id'],submission_source='urn:synthetic:outcome',correction_source='urn:synthetic:correction',ordinary=dict(starts_at=T,occurs_before='2026-09-22T12:00:00.000000Z',received_by='2026-09-23T12:00:00.000000Z',accepted_by='2026-09-23T13:00:00.000000Z'),corrections=dict(starts_at=T,occurs_before='2026-09-24T12:00:00.000000Z',received_by='2026-09-25T12:00:00.000000Z',accepted_by='2026-09-25T13:00:00.000000Z'),max_premium_atoms=spec.get('maximum_premium','10000'),max_discount_atoms=spec.get('supplier_base',spec['base']) if book=='supplier' else spec['base'])
        if book=='supplier': fields['supplier_authorization']=proofs['supplier_authorization']['id']
        policies[f]=add('policy-snapshot',fields,seed)
    base_event=targets['retail']
    all_postings=[r for r in seed if r['kind']=='base-posting']
    # Lossless typed base Evaluation material, not another base rating.
    def decimal_atoms(n):
        # Exact base fixture prices; this authors literals, it does not rate work.
        a=abs(int(n)); whole,fraction=divmod(a,100)
        return str(whole)+(('.'+str(fraction).zfill(2).rstrip('0')) if fraction else '')
    bundle_policies=[]
    for b in bindings.values():
        rules=[]; own=[r for r in all_postings if r['body']['binding_id']==b['body']['binding_id']]
        for r in own:
            atoms=r['body']['amount']['atoms'];ordinal=r['body']['ordinal'];component='base-'+r['body']['book']+'-'+str(ordinal)
            operation=dict(kind='base',price=dict(kind='fixed',value=decimal_atoms(atoms))) if int(atoms)>=0 else dict(kind='discount',amount=dict(kind='fixed',value=decimal_atoms(atoms)),component='base-'+r['body']['book']+'-0',mode='additive')
            rules.append(dict(id=component,on=base_event['body']['data']['work_type'],component=component,when=[],operation=operation))
        if not rules:rules=[dict(id='base-zero',on=base_event['body']['data']['work_type'],component='base-zero',when=[],operation=dict(kind='base',price=dict(kind='fixed',value='0')))]
        bundle_policies.append(dict(binding=b['body'],rules=rules))
    base_actions=[]
    for r in all_postings:
        b=r['body'];component='base-'+b['book']+'-'+str(b['ordinal'])
        base_actions.append(dict(id=r['id'],effect_id='ef_'+hashlib.sha256(('synthetic-base-effect:'+spec['name']+':'+component).encode()).hexdigest(),obligation_id=obligations[b['book']]['id'],binding=bindings[b['book']]['body'],binding_id=b['binding_id'],kind='charge' if int(b['amount']['atoms'])>=0 else 'discount',component=component,amount=b['amount'],book=b['book'],roles=b['roles'],sources=[base_event['id']],links=[],inputs=[]))
    base_material=dict(event=base_event['body']['data'],bundle=dict(currency='USD',scale=2,policies=bundle_policies,order=[[pi,ri] for pi,p in enumerate(bundle_policies) for ri in range(len(p['rules']))]),context=dict(document=proofs['base_context']['id'],customer='customer',funding='byok'),claim_id='synthetic-base-completion',actions=base_actions,explanations=[dict(binding_id=r['body']['binding_id'],rule_id='base-'+r['body']['book']+'-'+str(r['body']['ordinal']),code='BASE_APPLIED' if int(r['body']['amount']['atoms'])>=0 else 'DISCOUNT_APPLIED',unrounded=dict(numerator=r['body']['amount']['atoms'],denominator='1'),rounded=r['body']['amount']['atoms'],inputs=[],action_ids=[r['id']]) for r in all_postings],deltas=[dict(obligation_id=obligations[book]['id'],amount=bindings[book]['body']['booked_net'],book=book,roles=roles(book),actions=[r['id'] for r in all_postings if r['body']['book']==book]) for book in bindings if bindings[book]['body']['booked_net']['atoms']!='0'],consumptions=[dict(invocation_id='invocation-supplier',consume=money(spec['supplier_base']),release=money('0'))] if 'supplier_base' in spec else [],invocations=[b['body']['supplier_invocation'] for b in bindings.values() if 'supplier_invocation' in b['body']],received_at=T,source_authority=dict(source='urn:synthetic:work',grant=proofs['grant']['id'],revision='1',active=True,event_types=[base_event['body']['data']['work_type']],relations=[]),costs=[])
    if 'supplier_base' in spec:
        # Nomination is retained in original material, not inferred from another operation.
        base_material['event']=dict(base_material['event'],binding_id='binding-supplier',invocation_id='invocation-supplier')
    if spec.get('stage')=='capped': base_material['bundle']['policies'][0]['rules'].append(dict(id='cap',on='outcome',operation=dict(kind='cap',ceiling=money(spec['base']))))
    evaluation=add('base-evaluation',dict(event_id=base_event['id'],evaluation_utf8=canonical(base_material).decode(),postings=ordered([reference(r) for r in all_postings]),bindings=ordered([reference(b) for b in bindings.values()]),predecessors=[],received_at=T),seed)
    frozen=add('target-snapshot',dict(target=base_event['id'],base_evaluation=reference(evaluation),retail_basis=reference(bases['retail']),families=ordered([reference(v) for v in policies.values()]),bindings=ordered([reference(b) for b in bindings.values()]),limits=ordered([dict(binding_id=b['body']['binding_id'],premium=money(spec.get('maximum_premium','10000')),discount_capacity=b['body']['booked_net']) for b in bindings.values()]),accepted_at=T,rated_final=True,verified_assents=[proofs['assent']['id']],verified_offers=[proofs['offer']['id']] if 'supplier_base' in spec else [],verified_delegations=[],finality_evidence=proofs['finality']['id']),seed)
    base_members=sorted([reference(r) for r in seed],key=row_order)
    base_receipt=dict(schema='ledger-base-receipt/2-candidate.2',target=base_event['id'],base_evaluation=reference(evaluation),target_snapshot=reference(frozen),accepted_at=T,membership_hash=digest('base-membership',base_members))
    if spec.get('stage')!='capped': add('base-acceptance',dict(target=base_event['id'],base_evaluation=reference(evaluation),target_snapshot=reference(frozen),accepted_at=T,members=base_members,original_receipt_utf8=canonical(base_receipt).decode()),seed)
    for index,step in enumerate(spec['steps']):
        rows=[]; f=step['family']; book=step.get('book','retail'); policy=policies[f]; basis=bases['retail']; target=targets[book]; obligation=obligations[book]
        agreement='agreement-'+book; correcting=step.get('correct',False); prior=heads.get(f)
        timestamp=f'2026-09-21T13:{index:02d}:00.000000Z'
        # Revision and posting amounts below are explicit history-input literals.
        data=dict(type='correction' if correcting else 'outcome',source='urn:synthetic:correction' if correcting else 'urn:synthetic:outcome',external_id=spec['name']+'-'+str(index+1),chain_id=spec['name'],occurred_at=step.get('occurred_at','2026-09-21T12:30:00.000000Z'),evidence=[proofs['outcome' if step.get('reuse_evidence') else ('correction' if correcting else 'outcome')]['id']],target=target['id'],agreement_id=agreement,family_id=f,code=step.get('code','none'))
        if correcting:
            data.pop('code')
            data.update(claim_id=prior['body']['claim_id'],expected_revision=prior['id'],expected_revision_number=prior['body']['number'],replacement=dict(kind='reverse') if step.get('reverse') else dict(kind='code',code=step['code']))
        event=add('event',dict(data=data),rows); E=event['id']; R=ident('receipt',[E]); D=ident('decision-manifest',[E])
        clid=ident('claim',[S,agreement,f,target['id']]); number=str(int(prior['body']['number'])+1) if prior else '1'; rid=ident('claim-revision',[clid,number])
        received=step.get('received_at',timestamp); accepted=step.get('accepted_at',timestamp)
        authority=add('authority-decision',dict(event_id=E,target=target['id'],agreement_id=agreement,family_id=f,source=data['source'],principal='synthetic-authorized-principal',grant=proofs['grant']['id'],grant_revision='1',active=True,may_read=True,may_submit=not correcting,may_correct=correcting,verified_evidence=data['evidence'],received_at=received,accepted_at=accepted),rows)
        admission=add('admission',dict(event_id=E,principal='synthetic-authorized-principal',credential_revision='1',authentication=proofs['authentication']['id'],grant=proofs['grant']['id'],grant_revision='1',target_guard_revision='1',target_state='final_unreversed',aggregate_guard_revision=str(index),authorized_source=data['source'],agreement_id=agreement,payer=roles(book)['payer'],book=book,family_id=f,permission=data['type'],target_snapshot=frozen['id'],authority_decision=authority['id'],binding_id='binding-'+book,received_at=received,accepted_at=accepted,decision='allow',policy_snapshot=policy['id'],basis=basis['id']),rows)
        if not prior:
            claim=add('claim',dict(target=target['id'],agreement_id=agreement,book=book,family_id=f,first_event=E,original_receipt=R,facts_hash=digest('claim-facts',facts(event['body']))),rows)
        else: claim=known[canonical(clid)]
        add('link',dict(event_id=E,target=target['id'],relation='outcome_of'),rows)
        actions=[]; replacements=[]; inverses=[]
        def action(slot,atoms,reverse=None):
            efid=ident('effect',[clid,rid,slot])
            fields=dict(binding_id='binding-'+book,event_id=E,effect_id=efid,revision_id=rid,claim_id=clid,slot=slot,amount=money(atoms),obligation_id=obligation['id'],book=book,agreement_id=agreement,family_id=f,roles=roles(book),policy_snapshot=policy['id'],basis=basis['id'])
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
        revision_fields=dict(claim_id=clid,number=number,event_id=E,state='reversed' if step.get('reverse') else 'active',binding_id='binding-'+book,target_snapshot=frozen['id'],policy_snapshot=policy['id'],basis=basis['id'],admission=admission['id'],live_action_ids=ordered(replacements),inverse_action_ids=ordered(inverses),amount=money(step['atoms']),original_receipt=claim['body']['original_receipt'])
        if not step.get('reverse'): revision_fields['code']=step['code']
        if prior: revision_fields['previous']=prior['id']
        rev=add('claim-revision',revision_fields,rows)
        before=[v for v in heads.values() if v['body']['binding_id']=='binding-'+book]
        after=[v for v in before if v['body']['claim_id']!=clid]+[rev]
        def totals(revs):
            amounts=[int(r['body']['amount']['atoms']) for r in revs]
            return str(sum(max(v,0) for v in amounts)),str(sum(max(-v,0) for v in amounts))
        bp,bd=totals(before); ap,ad=totals(after)
        lf=dict(binding_id='binding-'+book,target_snapshot=frozen['id'],event_id=E,target=target['id'],agreement_id=agreement,book=book,currency='USD',scale=2,current_before=ordered([reference(v) for v in before]),before_premium=bp,before_discount=bd,after_premium=ap,after_discount=ad,maximum_premium=policy['body']['max_premium_atoms'],maximum_discount=policy['body']['max_discount_atoms'])
        if prior: lf['replacing']=prior['id']
        limit=add('limit-evidence',lf,rows)
        explanations=[]
        explanation_values=[]
        if prior:
            inverse_atoms=str(-int(prior['body']['amount']['atoms']))
            explanation_values.append(('EXACT_REVERSAL',[inverse_atoms,'1'],inverse_atoms,inverses))
        explanation_values.append(('CLAIM_REVERSED' if step.get('reverse') else ('ZERO_ROUNDED' if step['atoms']=='0' else 'OUTCOME_APPLIED'),step['exact'],step['atoms'],replacements))
        for ordinal,(code,exact,atoms,aids) in enumerate(explanation_values):
            xp=add('explanation',dict(event_id=E,ordinal=ordinal,claim_id=clid,revision_id=rid,code=code,policy_snapshot=policy['id'],basis=basis['id'],basis_amount=basis['body']['amount'],unrounded_atoms=ratio(exact),rounded_atoms=atoms,rounding='nearest_ties_away',action_ids=ordered(aids),limit_evidence=limit['id']),rows)
            explanations.append(xp)
            add('dependency',dict(dependent=xp['id'],input=reference(basis),reason='frozen_basis'),rows)
            add('dependency',dict(dependent=xp['id'],input=reference(limit),reason='aggregate_limit'),rows)
            if prior: add('dependency',dict(dependent=xp['id'],input=reference(prior),reason='prior_revision'),rows)
        # Complete retained economic inputs: all seed records plus all previous
        # canonical records (bounded); no recursive expansion from output rows.
        replay_refs=ordered([reference(r) for r in seed]+[reference(r) for d in decisions for r in d['records']]+[reference(event),reference(admission),reference(authority)])
        replay=add('replay-input',dict(event_id=E,semantics='ledger-outcome-semantics/2-candidate.2',target_snapshot=frozen['id'],authority_decision=authority['id'],inputs=replay_refs,received_at=received,accepted_at=accepted),rows)
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
        manifest=add('decision-manifest',dict(event_id=E,chain_id=spec['name'],revision=str(index+1),accepted_at=accepted,members=sorted(members.values(),key=row_order),explanation_ids=[x['id'] for x in explanations]),rows)
        receipt=add('receipt',dict(event_id=E,decision_id=D,chain_id=spec['name'],revision=str(index+1),accepted_at=accepted,event_hash=event['content_hash'],decision_hash=manifest['content_hash'],claim_id=clid,claim_revision=rid,action_ids=ordered([a['id'] for a in actions]),intention_ids=intent_ids),rows)
        decisions.append({'records':sorted(rows,key=row_order),'receipt_utf8':canonical(receipt['body']).decode()})
        heads[f]=rev
    probes=build_probes(spec,decisions,policies,bases,proofs,heads,targets)
    result={'status':'candidate-not-frozen','name':spec['name'],'target_admission':'rejected' if spec.get('stage')=='capped' else 'accepted','seed':sorted(seed,key=row_order),'decisions':decisions,'probes':probes}
    return result,vectors

def build_probes(spec,decisions,policies,bases,proofs,heads,targets):
    import copy
    probes=[]
    for source in spec.get('probes',[]):
        p={**source,'new_records':[]}
        kind=p['kind']
        if kind in ('identity_retry','semantic_retry','changed_facts','stale_correction'):
            original=next(r for r in decisions[p['step']]['records'] if r['kind']=='event')
            p['candidate']=copy.deepcopy(original['body'])
            if kind!='identity_retry': p['candidate']['data']['external_id']+='-retry'
            if kind=='changed_facts':p['candidate']['data']['code']='none'
            if kind.endswith('_retry'):
                p['original_receipt_utf8']=decisions[p['step']]['receipt_utf8']
                old=policies[p['candidate']['data']['family_id']]
                updated=copy.deepcopy(old['body']);updated['policy_version']='2'
                for rule in updated['rules']:
                    if rule['kind']=='fixed' and rule['fixed_atoms']!='0':rule['fixed_atoms']=str(2*int(rule['fixed_atoms']))
                p['current_policy']=envelope('policy-snapshot',S,updated)
        else:
            f=p.get('family','success'); p['policy_snapshot']=policies[f]['id'];p['basis']=bases['retail']['id']
            data=dict(type='outcome',source='urn:synthetic:outcome',external_id=spec['name']+'-rejected-'+str(len(probes)),chain_id=spec['name'],occurred_at=p.get('occurred_at','2026-09-21T12:30:00.000000Z'),evidence=[proofs['outcome']['id']],target=targets['retail']['id'],agreement_id='agreement-retail',family_id=f,code='success')
            if kind=='correction_timing':
                data.pop('code');data.update(type='correction',source='urn:synthetic:correction',claim_id=heads[f]['body']['claim_id'],expected_revision=heads[f]['id'],expected_revision_number=heads[f]['body']['number'],replacement=dict(kind='reverse'),evidence=[proofs['correction']['id']])
            p['candidate']=body('event',data=data)
            if kind=='aggregate_limit':p['current_before']=ordered([reference(v) for v in heads.values()])
        probes.append(p)
    return probes

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
        'scripts/check-contracts.sh','scripts/check-candidate-contracts.sh','ROADMAP.md'])
    paths.update(p for p in Path(__file__).parent.iterdir() if p.suffix in ('.py','.mjs'))
    value={'status':'candidate-not-frozen','profile':'2-candidate.2',
        'independent_review':'pending','base_commit':'b35258425970052ed71481eca1f33ef857c61be1','semantic_commit':'1e0ba3f886788c08f427d3aae1d916b341187e76',
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
