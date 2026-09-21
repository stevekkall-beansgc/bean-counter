"""Fully rehashed adversarial retained histories; no checksum-only rejection proof."""
import copy
import json
import subprocess
from pathlib import Path
from profile import canonical, envelope, reference, strict
from integrity import integrity,rebuild_hash_graph
from reconstruct import ROOT


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
    add('rewrite-complete-frozen-membership','predeclared-unclaimed-family',remove_family,'BASE_ACCEPTANCE_CHANGED')
    add('claim-revision-skipped','correction-replacement',lambda h:record(h,'claim-revision',1)['body'].update(number='9'),'REVISION_SEQUENCE')
    def held_capacity(h):
        b=next(r for r in h['seed'] if r['kind']=='binding-snapshot' and r['body']['book']=='supplier')
        b['body']['supplier_invocation']['held']['atoms']='3000'
        base=record(h,'base-evaluation')['body'];m=strict(base['evaluation_utf8'].encode())
        for p in m['bundle']['policies']:
            if p['binding']['book']=='supplier':p['binding']['supplier_invocation']['held']['atoms']='3000'
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
