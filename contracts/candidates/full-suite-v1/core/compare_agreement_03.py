#!/usr/bin/env python3
"""Read-only full author/independent AGR03 comparison. Sealed oracle paths explicit."""
import argparse,hashlib,json
from pathlib import Path
ap=argparse.ArgumentParser();ap.add_argument('--primary',type=Path,required=True);ap.add_argument('--expiry',type=Path,required=True);args=ap.parse_args()
P=Path(__file__).resolve().parent
SEALED={'primary':'d5aa41165ae16d6f98d8a483247b1ee178ed1808fc3fef4ea591515c03de48a4','expiry':'0af178a661fe98f1520b33c5f4e5d3169618bd57f006e62deaf8b8798d936907'}
def canon(v):
 if isinstance(v,dict):return '{'+','.join(canon(k)+':'+canon(v[k]) for k in sorted(v,key=lambda z:z.encode('utf-16-be')))+'}'
 if isinstance(v,list):return '['+','.join(canon(z) for z in v)+']'
 return json.dumps(v,ensure_ascii=False,separators=(',',':'),allow_nan=False)
for scenario in ['primary','expiry']:
 oracle_path=getattr(args,scenario);raw=oracle_path.read_bytes();assert hashlib.sha256(raw).hexdigest()==SEALED[scenario], 'sealed oracle SHA mismatch'
 independent=json.loads(raw);author=json.loads((P/f'agreement-03-{scenario}-author.json').read_text())
 assert author['registry_digest']==independent['registry_digest']
 own={v['ref']['id']:v for v in author['records']};peer={v['ref']['id']:v for v in independent['vectors']}
 assert len(own)==len(author['records'])==len(peer)==len(independent['vectors'])
 assert own.keys()==peer.keys()
 for identity,v in own.items():
  w=peer[identity]
  for field in ['body','ref','canonical_hex']:assert v[field]==w[field],(scenario,identity,field)
  assert canon(v['body'])==w['canonical_utf8']
 assert len(author['operations'])==len(independent['operations'])==len(independent['profile_b_deltas'])
 for v,w,meter in zip(author['operations'],independent['operations'],independent['profile_b_deltas']):
  name=v['name'];assert name==w['name']==meter['name']
  for field in ['locators','descriptor_resolution','request_evidence']:assert v[field]==w[field],(scenario,name,field)
  assert v['normalized_request']==w['request_evidence']['normalized_request']
  assert v['operation']['body']['key']==w['key']
  for field in ['manifest','receipt']:
   assert v[field]['body']==w[field] and v[field]['ref']==w[field+'_ref'],(scenario,name,field)
   assert canon(v[field]['body'])==w['canonical_'+field+'_utf8']
  assert len(v['manifest']['body']['members'])==w['member_count']
  assert v['meter']=={'units':meter['operation_delta'],'new_document_count':meter['new_document_count'],'new_document_bytes':meter['new_document_bytes'],'wrapper_bytes':meter['wrapper_key_bytes'],'wb_delta':meter['payload_delta']},(scenario,name,'meter')
 print(scenario, len(own),'bodies and',len(author['operations']),'complete chains: canonical bytes/Refs/requests/descriptors/membership/receipts/ProfileB MATCH')
print('Read-only independent comparison passed. No runtime authority/persistence or full-Wave PASS follows.')
