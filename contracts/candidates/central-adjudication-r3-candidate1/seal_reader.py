"""Resumable logical SEAL fold; actual paged storage/heap bounds remain pending."""
import copy,hashlib,json
import validate as v
from reads import expected
class SealReader:
 def __init__(self,ledger,gateway,round_number):
  self.ledger=ledger;self.gateway=gateway;self.round=int(round_number);self.prefix=expected(ledger,gateway);self.offset=0;self.phase=0;self.index=0;self.closed=False
  r=ledger.s['rounds'][self.round];gw=ledger.s['gateways'][gateway];v.require(gw['state']=='SEALING' and gw['round']==self.round,'SCAN_ROUND');owner=r['owner']+':'+gateway;v.require(ledger.s['allocations'][owner]['held']['workspace_bytes']>=32768,'SCAN_WORKSPACE')
  facts=[json.loads(x) for x in ledger.s['object_inventory'].get(gateway,{})];claims={x[2] for x in facts if x[1]=='CLAIM'};terminal={x[2] for x in facts if x[0]['host']==gateway and x[1] in {'RECEIPT','ALIAS','RETURNED_UNUSED'}}
  tokens=sorted((t for tid,t in ledger.s['tokens'].items() if tid in claims and tid in terminal and t['body']['gateway']==gateway and int(t['body']['allocation'])<=r['cutoffs'][gateway]),key=lambda t:int(t['body']['allocation']))
  v.require(len(tokens)==r['cutoffs'][gateway] and all(int(t['body']['allocation'])==i+1 for i,t in enumerate(tokens)),'SCAN_HOLE')
  # This model already holds reconstructed history; adapters supply these as ordered
  # paged iterators. No exported cursor contains a pending/token list.
  self.values=[[[t['body']['allocation'],t['body']['id'],t['state']] for t in tokens],[[str(pos),ledger.s['tokens'][tid]['receipt']] for pos,tid in sorted(gw['receipts'].items())]]
  self.hashes=[hashlib.sha256(('ledgerlab/central-r3/'+d+'/1\0').encode()) for d in ['result','receipt']]
 def cursor(self):return dict(expected=copy.deepcopy(self.prefix),round=str(self.round),phase=['DISPOSITIONS','RECEIPTS','COMPLETE'][self.phase],entry=str(self.index),byte_offset=str(self.offset))
 def read(self,bytes_budget,pages_budget):
  v.require(not self.closed,'SCAN_ABORTED');v.require(self.prefix==expected(self.ledger,self.gateway),'SCAN_PREFIX_CHANGED');used=pages=0
  while self.phase<2 and used<bytes_budget and pages<pages_budget:
   values=self.values[self.phase]
   if self.index==0:raw=b'['
   elif self.index<=len(values):raw=(b',' if self.index>1 else b'')+v.canonical(values[self.index-1])
   else:raw=b']'
   take=min(4032,len(raw)-self.offset,bytes_budget-used);self.hashes[self.phase].update(raw[self.offset:self.offset+take]);self.offset+=take;used+=take;pages+=1
   if self.offset==len(raw):
    self.index+=1;self.offset=0
    if self.index==len(values)+2:self.phase+=1;self.index=0
  result=dict(status='COMPLETE' if self.phase==2 else 'INCOMPLETE',cursor=self.cursor(),measured=dict(bytes=str(used),pages=str(pages)))
  if self.phase==2:result.update(disposition_root=self.hashes[0].hexdigest(),receipt_root=self.hashes[1].hexdigest())
  v.validate_shape('seal_read_response',result);return result
 def abort(self):self.closed=True;self.values=[]
