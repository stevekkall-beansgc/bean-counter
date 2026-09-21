"""Fully rehashed adversarial retained histories; no checksum-only rejection proof."""
import copy
import json
import subprocess
from pathlib import Path
from profile import canonical, envelope, reference, strict
from integrity import integrity,rebuild_hash_graph
from reconstruct import ROOT
from retained import document_fields


def run_adversarial(histories,verify):
    from audit import checked_row
    byname={h['name']:h for h in histories};cases=[]
    def record(h,kind,index=None):
        rows=h['seed'] if index is None else h['decisions'][index]['records']
        return next(r for r in rows if r['kind']==kind)
    def add(name,original,mutate,expected):
        h=copy.deepcopy(byname[original]);h['probes']=[]
        anchor=reference(record(h,'base-acceptance'))
        mutate(h);rebuild_hash_graph(h)
        integrity(h,checked_row)  # all rewritten digests and references VALID first
        try:verify(h,anchor)
        except AssertionError as e:
            assert str(e)==expected, (name,str(e),expected)
        else:raise AssertionError('fully rehashed attack accepted: '+name)
        cases.append(dict(name=name,history=h,expected=expected))
    add('altered-retail-basis','fixed-success-fee',lambda h:record(h,'target-basis')['body']['amount'].update(atoms='11000'),'FROZEN_BASIS')
    def remove_family(h):
        removed=next(r for r in h['seed'] if r['kind']=='policy-snapshot' and r['body']['family_id']=='future-bonus')
        h['seed'].remove(removed)
        target=record(h,'target-snapshot')['body'];target['families']=[v for v in target['families'] if v['id']!=removed['id']]
        policy=strict(target['policy_utf8'].encode());policy['families']=[f for f in policy['families'] if f['family']!='future-bonus']
        target['policy_utf8']=canonical(policy).decode()
        proof=next(r for r in h['seed'] if r['kind']=='evidence' and r['body']['document_id']==policy['document'])
        proof['body']['utf8']=canonical({k:v for k,v in policy.items() if k!='document'}).decode()
    add('rewrite-complete-frozen-membership','predeclared-unclaimed-family',remove_family,'BASE_ACCEPTANCE_CHANGED')
    add('claim-revision-skipped','correction-replacement',lambda h:record(h,'claim-revision',1)['body'].update(number='9'),'REVISION_SEQUENCE')
    def held_capacity(h):
        b=next(r for r in h['seed'] if r['kind']=='binding-snapshot' and r['body']['book']=='supplier')
        b['body']['supplier_invocation']['held']['atoms']='3000'
        base=record(h,'base-evaluation')['body'];m=strict(base['evaluation_utf8'].encode())
        for i in m['invocations']:i['held']['atoms']='3000'
        base['evaluation_utf8']=canonical(m).decode()
    add('supplier-contingent-capacity','supplier-separation',held_capacity,'EXPOSURE_EXCEEDED')
    def discount_capacity(h):
        records=h['decisions'][1]['records']
        for r in records:
            if r['kind'] in ('claim-revision','action'):r['body']['amount']['atoms']='-4000'
            if r['kind']=='limit-evidence':r['body']['after_discount']='4000'
            if r['kind']=='explanation':r['body'].update(unrounded_atoms={'numerator':'-4000','denominator':'1'},rounded_atoms='-4000')
    add('supplier-discount-capacity-not-retail-denominator','supplier-separation',discount_capacity,'DISCOUNT_EXCEEDS_BASIS')
    def stale(h):
        first=record(h,'claim-revision',0)
        record(h,'event',2)['body']['data'].update(expected_revision=first['id'],expected_revision_number='1')
    add('stale-correction-after-rehash','correction-replacement',stale,'STALE_CORRECTION')
    def deadline(h,field,value):
        for r in h['decisions'][-1]['records']:
            if field in r['body']:r['body'][field]=value
    add('ordinary-receipt-boundary-plus-microsecond','inclusive-ordinary-deadlines',lambda h:deadline(h,'received_at','2026-09-23T12:00:00.000001Z'),'OUTCOME_DEADLINE')
    add('ordinary-acceptance-boundary-plus-microsecond','inclusive-ordinary-deadlines',lambda h:deadline(h,'accepted_at','2026-09-23T13:00:00.000001Z'),'OUTCOME_DEADLINE')
    add('correction-receipt-boundary-plus-microsecond','inclusive-correction-deadlines',lambda h:deadline(h,'received_at','2026-09-25T12:00:00.000001Z'),'OUTCOME_DEADLINE')
    add('correction-acceptance-boundary-plus-microsecond','inclusive-correction-deadlines',lambda h:deadline(h,'accepted_at','2026-09-25T13:00:00.000001Z'),'OUTCOME_DEADLINE')
    add('ordinary-exclusive-occurrence-end','inclusive-ordinary-deadlines',lambda h:record(h,'event',0)['body']['data'].update(occurred_at='2026-09-22T12:00:00.000000Z'),'OUTCOME_WINDOW')
    add('correction-exclusive-occurrence-end','inclusive-correction-deadlines',lambda h:record(h,'event',1)['body']['data'].update(occurred_at='2026-09-24T12:00:00.000000Z'),'OUTCOME_WINDOW')
    def duplicate_version(h):
        old=record(h,'policy-snapshot');b=copy.deepcopy(old['body']);b['policy_version']='2'
        duplicate=envelope('policy-snapshot',old['scope'],b);h['seed'].append(duplicate)
        record(h,'target-snapshot')['body']['families'].append(reference(duplicate))
    add('policy-version-cannot-add-eligibility','fixed-success-fee',duplicate_version,'POLICY_AMBIGUOUS_MATCH')
    # All attacks below pass structural and independent byte/hash validation.
    # Ordinary and correction evidence is introduced without touching the root.
    for ix, label in ((0,'claim'),(1,'correction')):
        add(label+'-new-evidence-unverified','decision-time-evidence',
            lambda h,ix=ix:record(h,'authority-decision',ix)['body'].update(verified_evidence=[]),
            'OUTCOME_EVIDENCE_REQUIRED')
        add(label+'-explanation-evidence-substitution','decision-time-evidence',
            lambda h,ix=ix:record(h,'explanation',ix)['body'].update(evidence=[]),
            'EXPLANATION_EVIDENCE')
    add('evidence-verification-belongs-to-another-decision','decision-time-evidence',
        lambda h:record(h,'authority-decision',1)['body'].update(event_id=record(h,'event')['id']),
        'AUTHORITY_REFERENCE')
    def unused_evidence(h):
        old=record(h,'evidence');b=copy.deepcopy(old['body']);b.update(document_fields(b['document_type'],{'synthetic':'unused'}))
        h['decisions'][0]['records'].append(envelope('evidence',old['scope'],b))
    add('unreferenced-decision-evidence','decision-time-evidence',unused_evidence,'UNUSED_DECISION_EVIDENCE')
    def inject_terms(h):
        old=record(h,'policy-snapshot');b=copy.deepcopy(old['body']);b['family_id']='evidence-invented-family'
        h['decisions'][0]['records'].append(envelope('policy-snapshot',old['scope'],b))
    add('new-evidence-cannot-add-family','decision-time-evidence',inject_terms,'UNREFERENCED_NEW_SEED')
    def evidence_money(h):
        for r in h['decisions'][0]['records']:
            if r['kind']=='evidence':r['body']['utf8']=canonical({'synthetic':'nonoperative','proposed_atoms':'9000'}).decode()
            if r['kind'] in ('action','claim-revision'):r['body']['amount']['atoms']='9000'
            if r['kind']=='limit-evidence':r['body']['after_premium']='9000'
    add('new-evidence-cannot-set-money','decision-time-evidence',evidence_money,'EXPLANATION_MATH')
    def wrong_policy_observation(h):
        target=record(h,'target-snapshot')['body']
        target['verified_policy_document']=next(r['body']['document_id'] for r in h['seed'] if r['kind']=='evidence' and r['body']['purpose']=='grant')
    add('verified-policy-document-mismatch','fixed-success-fee',wrong_policy_observation,'TERMS_NOT_VERIFIED')
    def wrong_policy_source(h):
        target=record(h,'target-snapshot')['body'];p=strict(target['policy_utf8'].encode())
        p['document']=next(r['body']['document_id'] for r in h['seed'] if r['kind']=='evidence' and r['body']['purpose']=='grant')
        target['policy_utf8']=canonical(p).decode()
    add('original-policy-document-mismatch','fixed-success-fee',wrong_policy_source,'TERMS_NOT_VERIFIED')
    add('policy-document-observed-hash-mismatch','fixed-success-fee',
        lambda h:record(h,'target-snapshot')['body'].update(policy_document_hash='sha256:'+'0'*64),
        'POLICY_DOCUMENT_EVIDENCE')
    def changed_policy_document(h):
        proof=next(r for r in h['seed'] if r['kind']=='evidence' and r['body']['purpose']=='policy')
        value=strict(proof['body']['utf8'].encode());value['version']='changed-document';proof['body']['utf8']=canonical(value).decode()
    add('policy-document-terms-mismatch','fixed-success-fee',changed_policy_document,'POLICY_DOCUMENT_TERMS')
    def strip_extensions(h):
        b=record(h,'base-evaluation')['body'];m=strict(b['evaluation_utf8'].encode());m['event']['extensions']={};b['evaluation_utf8']=canonical(m).decode()
    add('lossless-event-extensions-discarded','lossless-event-and-outcome-terms',strip_extensions,'BASE_EVENT_BYTES')
    def strip_outcome(h):
        b=record(h,'binding-snapshot')['body'];source=strict(b['binding_utf8'].encode());del source['outcome'];b['binding_utf8']=canonical(source).decode()
    add('lossless-binding-outcome-discarded','lossless-event-and-outcome-terms',strip_outcome,'BASE_BINDING_MATERIAL')
    add('lossless-binding-outcome-projection-changed','lossless-event-and-outcome-terms',
        lambda h:record(h,'binding-snapshot')['body']['outcome'].update(claim_namespace='changed'),
        'BASE_BINDING_PROJECTION')
    def substitute_action(h,ix,slot,foreign):
        a=next(r for r in h['decisions'][ix]['records'] if r['kind']=='action' and r['body']['slot']==slot)
        effect=next(r for r in h['decisions'][ix]['records'] if r['kind']=='effect' and r['body']['action_id']==a['id'])
        for r in (a,effect):
            r['body']['binding_id']='binding-supplier' if foreign else 'binding-never-authorized'
            if foreign:r['body']['binding_snapshot']=next(r['id'] for r in h['seed'] if r['kind']=='binding-snapshot' and r['body']['book']=='supplier')
    for ix,slot,label in ((0,'replacement','original'),(2,'replacement','replacement'),(2,'inverse','inverse')):
        for foreign in (False,True):
            add(label+('-cross-family-binding' if foreign else '-unauthorized-binding'),'cross-binding-corrections',
                lambda h,ix=ix,slot=slot,foreign=foreign:substitute_action(h,ix,slot,foreign),'ACTION_BINDING')
    add('action-unpermitted-component','fixed-success-fee',
        lambda h:record(h,'action',0)['body'].update(component='different-family'),'ACTION_COMPONENT')
    add('effect-binding-substitution','fixed-success-fee',
        lambda h:record(h,'effect',0)['body'].update(binding_id='binding-never-authorized'),'EFFECT_BINDING')
    def original_binding(h):
        b=record(h,'base-evaluation')['body'];m=strict(b['evaluation_utf8'].encode())
        m['actions'][0]['binding']['agreement']='unaccepted-agreement';b['evaluation_utf8']=canonical(m).decode()
    add('original-base-action-binding-substitution','fixed-success-fee',original_binding,'BASE_ACTION_BINDING')
    # Book cannot split the economic claim key even if its posting data differ.
    from profile import key
    claim=record(byname['fixed-success-fee'],'claim',0)
    changed=copy.deepcopy(claim['body']);changed['book']='supplier'
    assert key('claim',claim['scope'],changed)==claim['id']
    path=ROOT/'work/validation/v2-rehashed-attacks.json';path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes(canonical([c['history'] for c in cases]))
    subprocess.run(['node',str(Path(__file__).with_name('check_hashes.mjs')),str(ROOT),'--histories',str(path)],check=True)
    (ROOT/'work/validation/v2-adversarial-results.json').write_text(json.dumps([{'case':c['name'],'integrity':'passed','semantic_rejection':c['expected']} for c in cases],indent=2)+'\n')
    return len(cases)+1
