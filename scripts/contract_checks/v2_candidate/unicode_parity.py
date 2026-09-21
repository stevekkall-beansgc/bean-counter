"""Exhaustive Unicode classification and a coherently rehashed accepted history.

This audits the approved source rule; it does not normalize or tighten it.
Diagnostic outputs live only under ignored work/. Goldens are never rewritten.
"""
import json
import subprocess
from pathlib import Path
from integrity import integrity, rebuild_hash_graph
from profile import canonical, reference, strict
from reconstruct import ROOT
from scalars import check_scalar


def run_unicode_parity(histories, checked_row, verify):
    # Independent closed ranges for the approved Unicode Cc / White_Space
    # classifications. Python's four extra isspace characters are already Cc.
    controls=set(range(0x20))|set(range(0x7f,0xa0))
    whitespace=set(range(0x09,0x0e))|{0x20,0x85,0xa0,0x1680,0x2028,0x2029,0x202f,0x205f,0x3000}|set(range(0x2000,0x200b))
    expected={'text':sorted(controls),'source':sorted(controls|whitespace)}
    rejected={'text':[],'source':[]};count=0
    for cp in range(0x110000):
        if 0xd800<=cp<=0xdfff:continue  # Surrogates are not Unicode scalar values.
        count+=1;character=chr(cp)
        for kind,value in [('text','a'+character),('source','urn:synthetic:'+character+'outcome')]:
            try:check_scalar(kind,value)
            except ValueError:rejected[kind].append(cp)
    assert count==1112064 and rejected==expected, 'PYTHON_UNICODE_CLASSIFICATION'
    path=ROOT/'work/validation/v2-unicode-parity.json'
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_bytes(canonical(dict(scalar_values=count,rejected=expected)))
    subprocess.run(['node',str(Path(__file__).with_name('unicode_parity.mjs')),str(ROOT),str(path)],check=True)

    # Change the complete source-bearing history, including recursively embedded
    # canonical JSON. All identities/hashes/manifests/receipts are then rebuilt.
    old='urn:synthetic:outcome';new='urn:synthetic:\ufeffoutcome';replacements=0
    def rewrite(value):
        nonlocal replacements
        if isinstance(value,dict):return {k:rewrite(v) for k,v in value.items()}
        if isinstance(value,list):return [rewrite(v) for v in value]
        if isinstance(value,str):
            if value.startswith(('{','[')):
                return canonical(rewrite(strict(value.encode()))).decode()
            replacements+=value.count(old)
            return value.replace(old,new)
        return value
    history=rewrite(next(h for h in histories if h['name']=='fixed-success-fee'))
    history['probes']=[]
    rebuild_hash_graph(history)
    integrity(history,checked_row)
    # This is a newly derived valid fixture, not an attempted replacement for a
    # pre-existing accepted root. Anchor it to its own newly constructed base.
    anchor=reference(next(r for r in history['seed'] if r['kind']=='base-acceptance'))
    records=verify(history,anchor)
    assert replacements>10 and records==41 and len(history['decisions'])==1
    history_path=ROOT/'work/validation/v2-rehashed-accepted-unicode.json'
    history_path.write_bytes(canonical([history]))
    subprocess.run(['node',str(Path(__file__).with_name('check_hashes.mjs')),str(ROOT),'--histories',str(history_path)],check=True)
    print(json.dumps(dict(status='passed',unicode_scalar_values=count,checks_per_runtime=count*2,
                          text_rejected=len(expected['text']),source_rejected=len(expected['source']),
                          accepted_rehashed_histories=1,accepted_rehashed_records=records,
                          accepted_rehashed_decisions=1)))
