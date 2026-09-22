"""Canonical bounded read session; no actual-store durability/nonposting proof."""
import copy,hashlib
import validate as v
class Reader:
 def __init__(self,ledger,visitor=None):self.ledger=ledger;self.sessions={};self.visitor=visitor
 def cancel(self):self.sessions.clear()
 def start(self,expected):
  v.validate_shape('expected_prefix',expected);e=self.ledger.s['enrollment'];v.require(expected['store']==e['store'] and expected['scope']==e['scope'] and expected['registration']==e['registration'] and expected['target']==e['target'] and expected['enrollment']==self.ledger.enrollment_hash and expected['profile']=='central-adjudication-r3/1','EXPECTED_BINDING');v.require(0<int(expected['ordinal'])<=len(self.ledger.read_index.get(expected['host'],[])),'EXPECTED_PREFIX');self.cancel()
  return dict(expected=copy.deepcopy(expected),ordinal=0,offset=0,root=v.ZERO,previous=v.ZERO,hash=hashlib.sha256(b'ledgerlab/central-r3/segment/1\0'))
 def read(self,request):
  v.validate_shape('read_request',request)
  if 'cursor' in request:
   token=request['cursor']['continuation'];v.require(token in self.sessions and self.cursor(self.sessions[token])==request['cursor'],'UNVERIFIED_CURSOR');state=dict(self.sessions[token]);state['hash']=state['hash'].copy();v.require(state['expected']==request['expected'],'CURSOR_PREFIX')
  else:state=self.start(request['expected'])
  measured=dict(bytes=0,pages=0,segments=0);budget={k:int(a) for k,a in request['budget'].items()};limit=int(state['expected']['ordinal']);host=state['expected']['host']
  while state['ordinal']<limit:
   row=self.ledger.read_index[host][state['ordinal']];remaining=len(row['bytes'])-state['offset'];take=min(4096,remaining,budget['bytes']-measured['bytes'])
   if take<=0 or measured['pages']>=budget['pages'] or measured['segments']>=budget['segments']:break
   state['hash'].update(row['bytes'][state['offset']:state['offset']+take]);state['offset']+=take;measured['bytes']+=take;measured['pages']+=1
   if state['offset']==len(row['bytes']):
    v.require(row['previous']==state['previous'] and row['previous_root']==state['root'],'PREFIX_CHAIN');v.require(state['hash'].hexdigest()==row['segment'],'SEGMENT_HASH');
    if self.visitor:self.visitor(row)
    state['previous']=row['segment'];state['root']=row['root'];state['ordinal']+=1;state['offset']=0;state['hash']=hashlib.sha256(b'ledgerlab/central-r3/segment/1\0');measured['segments']+=1
  measured={k:str(a) for k,a in measured.items()};self.cancel()
  if state['ordinal']==limit:
   v.require(state['root']==state['expected']['root'] and state['previous']==state['expected']['segment'],'EXPECTED_PREFIX');result=dict(status='COMPLETE',expected=copy.deepcopy(state['expected']),measured=measured,selection='CURRENT_AT_READ' if self.ledger.journal(host)['root']==state['expected']['root'] else 'HISTORICAL_PREFIX',scope='CENTRAL_PREFIX' if host==self.ledger.s['enrollment']['store'] else 'GATEWAY_PREFIX',coverage=v.sorted_set([{'gateway':g,'status':'UNKNOWN_GATEWAY_COVERAGE'} for g in self.ledger.s['gateways']]))
  else:
   cursor=self.cursor(state);self.sessions[cursor['continuation']]=state;result=dict(status='INCOMPLETE',cursor=cursor,measured=measured)
  v.validate_shape('read_response',result);return result
 def cursor(self,s):
  c=dict(expected=copy.deepcopy(s['expected']),ordinal=str(s['ordinal']),byte_offset=str(s['offset']),verified_root=s['root']);c['continuation']=v.digest('authority',c);return c

def expected(ledger,host='center'):
 e=ledger.s['enrollment'];j=ledger.journal(host);return dict(store=e['store'],scope=e['scope'],target=e['target'],profile='central-adjudication-r3/1',enrollment=ledger.enrollment_hash,registration=e['registration'],host=host,ordinal=j['ordinal'],segment=j['segment'],root=j['root'])

class ComparisonReader:
 def __init__(self,ledger):self.ledger=ledger;self.reader=None;self.binding=None
 def visit(self,row):
  if not self.failure:
   for effect in row['effects']:
    if effect['kind']!='ACTION':continue
    a=effect['body'];amount=int(a['signed_atoms'])
    if a['book']=='RETAIL' and a['kind']=='ORDINARY':
     alt=int(self.binding['policy']['resolution_atoms']) if a['case'][0]==self.ledger.s['enrollment']['families'][0]['key'] else amount;self.premium+=max(0,alt)
     if self.premium>int(self.ledger.s['enrollment']['premium_cap']):self.failure=copy.deepcopy(a['case']);break
     self.delta+=alt-amount
  for i,entry in enumerate(self.binding['coverage']):
   if entry['status']=='COMPLETE_GATEWAY_CUTOFF' and any(effect['kind']=='CLOSURE' and entry in effect['body']['cutoffs'] for effect in row['effects']):self.covered[i]=True
   elif entry['status']=='UNRECONCILED':
    proof=row['proof']
    if proof and proof['fact_kind']=='SEAL' and proof['host']==entry['gateway'] and proof['trusted_observation_ref']==entry['observation']:self.covered[i]=True
 def read(self,request):
  v.validate_shape('comparison_request',request);binding={k:copy.deepcopy(request[k]) for k in ['expected','policy','coverage']};e=self.ledger.s['enrollment'];v.require({x['gateway'] for x in request['coverage']}==set(self.ledger.s['gateways']),'COVERAGE_GATEWAYS')
  if 'cursor' not in request:self.binding=binding;self.delta=0;self.premium=0;self.failure=None;self.covered=[x['status']=='UNKNOWN_GATEWAY_COVERAGE' for x in request['coverage']];self.reader=Reader(self.ledger,self.visit)
  else:v.require(self.reader is not None and self.binding==binding,'COMPARISON_CURSOR_BINDING')
  read_request=dict(expected=request['expected'],budget=request['budget'])
  if 'cursor' in request:read_request['cursor']=request['cursor']
  r=self.reader.read(read_request);prefix=request['expected'];measured=r['measured']
  if r['status']!='COMPLETE':result=dict(status='INCOMPLETE',expected=prefix,cursor=r['cursor'],measured=measured,reason='verification and policy fold incomplete; no successful total')
  elif prefix['host']!=e['store'] or prefix['root']!=self.ledger.journal(e['store'])['root']:result=dict(status='UNSUPPORTED',measured=measured,reason='comparison requires this pinned central snapshot')
  else:
   v.require(all(self.covered),'COVERAGE_EXACT_PREFIX')
   if self.failure:result=dict(status='POLICY_FAILURE',at_case=self.failure,measured=measured,reason='original premium cap exceeded at this actual acceptance')
   else:result=dict(status='COMPARABLE',expected=prefix,actual=str(self.ledger.s['customer']),alternative=str(self.ledger.s['customer']+self.delta),difference=str(self.delta),supplier_booked=e['supplier_booked'],coverage=copy.deepcopy(request['coverage']),measured=measured)
  v.validate_shape('comparison_response',result);return result
 def cancel(self):
  if self.reader:self.reader.cancel()
  self.reader=None;self.binding=None

def compare(ledger,request):return ComparisonReader(ledger).read(request)

def reconstruct_stored(trace,segments,expected_prefixes):
 """Fresh semantic reconstruction; only its result is eligible for Reader reuse."""
 ledger=v.Ledger(trace['initial']);position=0
 for command in trace['commands']:
  result=ledger.execute(command)
  if result['status']=='COMMITTED':
   v.require(position<len(segments),'STORED_SUFFIX_MISSING');v.validate_shape('segment',segments[position]);v.require(v.canonical(segments[position])==v.canonical(ledger.s['segments'][-1]),'STORED_SEMANTIC_MISMATCH');position+=1
 v.require(position==len(segments),'STORED_EXTRA_SEGMENT')
 for pin in expected_prefixes:v.require(expected(ledger,pin['host'])==pin,'EXPECTED_PREFIX')
 return ledger
