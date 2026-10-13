#!/usr/bin/env python3
"""Read-only full author/independent selected-chain comparison; no expected writes."""
import hashlib,json,pathlib
P=pathlib.Path(__file__).resolve().parent
expected=P/'transport-02-independent/expected.json'
assert hashlib.sha256(expected.read_bytes()).hexdigest()=='fa2ebceb2150961275e740e3c9d360b1944975d794bbbda3c8e5dc79bbcc672b'
r=json.loads(expected.read_text());a=json.loads((P/'publication-transport-02-author.json').read_text())
def cj(x):
 if isinstance(x,dict):return '{'+','.join(cj(k)+':'+cj(x[k]) for k in sorted(x,key=lambda k:k.encode('utf-16-be')))+'}'
 if isinstance(x,list):return '['+','.join(map(cj,x))+']'
 return json.dumps(x,ensure_ascii=False,separators=(',',':'))
ar={v['id']:v for v in a['records']};rr={v['id']:v for v in r['envelopes']};assert ar==rr
for ident in ar:assert cj(ar[ident]['body']).encode()==cj(rr[ident]['body']).encode()
assert len(a['operations'])==len(r['operations'])==27
for x,y in zip(a['operations'],r['operations']):
 assert x['operation_id']==y['name'] and x['request']==y['normalized_request']
 for field in ['locators','descriptor_resolution','request_evidence','operation_ref','manifest_ref','receipt_ref','result_refs']:assert x[field]==y[field],(x['operation_id'],field)
 assert ar[x['manifest_ref']['id']]['body']['members']==y['members']
 b=x['profile_b'];d=y['delta'];assert (b['units'],len(b['new_document_refs']),b['new_document_bytes'],b['wrapper_and_key_bytes'],b['payload_bytes'])==(d['units'],d['new_documents'],d['document_bytes'],d['wrapper_and_key_bytes'],d['WB_delta'])
assert [x['response'] for x in a['canonical_reads']]==r['canonical_reads']
assert a['original_retry']['status']==r['original_retry']['status_result']
assert a['original_retry']['units']==r['original_retry']['new_operations']==0
assert a['original_retry']['payload_bytes']==r['original_retry']['WB_delta']==0
assert a['totals']['payload_bytes']==r['summary']['WB_total']==4812654
print('PASS exact135 envelopes/canonical bodies,27 request/descriptor/evidence/manifest/receipt chains and B deltas,2 reads and original retry/status; selected synthetic encoding only')
