"""Resumable logical SEAL fold; actual paged storage/heap bounds remain pending."""
import copy,hashlib,json
import validate as v
from reads import expected
class SealReader:
 def __init__(self,ledger,gateway,round_number):
  self.ledger=ledger;self.gateway=gateway;self.round=int(round_number);self.prefix=expected(ledger,gateway);self.offset=0;self.phase=0;self.index=0;self.closed=False
  r=ledger.s['rounds'][self.round];gw=ledger.s['gateways'][gateway];v.require(gw['state']=='SEALING' and gw['round']==self.round,'SCAN_ROUND');owner=r['owner']+':'+gateway;v.require(ledger.s['allocations'][owner]['held']['workspace_bytes']>=32768,'SCAN_WORKSPACE')
  self.counts=[r['cutoffs'][gateway],gw['receipt']]
  self.hashes=[hashlib.sha256(('ledgerlab/central-r3/'+d+'/1\0').encode()) for d in ['result','receipt']]
 def cursor(self):return dict(expected=copy.deepcopy(self.prefix),round=str(self.round),phase=['DISPOSITIONS','RECEIPTS','COMPLETE'][self.phase],entry=str(self.index),byte_offset=str(self.offset))
 def read(self,bytes_budget,pages_budget):
  v.require(not self.closed,'SCAN_ABORTED');v.require(self.prefix==expected(self.ledger,self.gateway),'SCAN_PREFIX_CHANGED');used=pages=0
  while self.phase<2 and used<bytes_budget and pages<pages_budget:
   count=self.counts[self.phase]
   if self.index==0:raw=b'['
   elif self.index<=count:
    index=self.ledger.seal_index.get(self.gateway,{}).get(['dispositions','receipts'][self.phase],{});v.require(self.index in index,'SCAN_HOLE');raw=index[self.index]
   else:raw=b']'
   take=min(4032,len(raw)-self.offset,bytes_budget-used);self.hashes[self.phase].update(raw[self.offset:self.offset+take]);self.offset+=take;used+=take;pages+=1
   if self.offset==len(raw):
    self.index+=1;self.offset=0
    if self.index==count+2:self.phase+=1;self.index=0
  result=dict(status='COMPLETE' if self.phase==2 else 'INCOMPLETE',cursor=self.cursor(),measured=dict(bytes=str(used),pages=str(pages)))
  if self.phase==2:result.update(disposition_root=self.hashes[0].hexdigest(),receipt_root=self.hashes[1].hexdigest())
  v.validate_shape('seal_read_response',result);return result
 def abort(self):self.closed=True
