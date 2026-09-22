"""Canonical bounded read session; no actual-store durability/nonposting proof."""
import copy,hashlib
import validate as v
class Reader:
 def __init__(self,ledger):self.ledger=ledger;self.sessions={}
 def cancel(self):self.sessions.clear()
 def start(self,expected):
  v.validate_shape('expected_prefix',expected);e=self.ledger.s['enrollment'];v.require(expected['store']==e['store'] and expected['scope']==e['scope'] and expected['registration']==e['registration'],'EXPECTED_BINDING');v.require(0<int(expected['ordinal'])<=len(self.ledger.read_index.get(expected['host'],[])),'EXPECTED_PREFIX');self.cancel()
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
    v.require(row['previous']==state['previous'] and row['previous_root']==state['root'],'PREFIX_CHAIN');v.require(state['hash'].hexdigest()==row['segment'],'SEGMENT_HASH');state['previous']=row['segment'];state['root']=row['root'];state['ordinal']+=1;state['offset']=0;state['hash']=hashlib.sha256(b'ledgerlab/central-r3/segment/1\0');measured['segments']+=1
  measured={k:str(a) for k,a in measured.items()};self.cancel()
  if state['ordinal']==limit:
   v.require(state['root']==state['expected']['root'] and state['previous']==state['expected']['segment'],'EXPECTED_PREFIX');result=dict(status='COMPLETE',expected=copy.deepcopy(state['expected']),measured=measured,selection='CURRENT_AT_READ' if self.ledger.journal(host)['root']==state['expected']['root'] else 'HISTORICAL_PREFIX',scope='CENTRAL_PREFIX' if host==self.ledger.s['enrollment']['store'] else 'GATEWAY_PREFIX',coverage=v.sorted_set([{'gateway':g,'status':'UNKNOWN_GATEWAY_COVERAGE'} for g in self.ledger.s['gateways']]))
  else:
   cursor=self.cursor(state);self.sessions[cursor['continuation']]=state;result=dict(status='INCOMPLETE',cursor=cursor,measured=measured)
  v.validate_shape('read_response',result);return result
 def cursor(self,s):
  c=dict(expected=copy.deepcopy(s['expected']),ordinal=str(s['ordinal']),byte_offset=str(s['offset']),verified_root=s['root']);c['continuation']=v.digest('authority',c);return c

def expected(ledger,host='center'):
 e=ledger.s['enrollment'];j=ledger.journal(host);return dict(store=e['store'],scope=e['scope'],registration=e['registration'],host=host,ordinal=j['ordinal'],segment=j['segment'],root=j['root'])

def compare(ledger,request):
 v.validate_shape('comparison_request',request);prefix=request['expected'];r=Reader(ledger).read(dict(expected=prefix,budget=request['budget']))
 if r['status']!='COMPLETE':return dict(status='INCOMPLETE',expected=prefix,reason='verification budget exhausted; no successful total')
 e=ledger.s['enrollment']
 v.require({x['gateway'] for x in request['coverage']}==set(ledger.s['gateways']),'COVERAGE_GATEWAYS')
 for entry in request['coverage']:
  if entry['status']!='UNKNOWN_GATEWAY_COVERAGE':
   v.require(entry['observation'] in ledger.initial['trusted_observations'],'COVERAGE_AUTHORITY')
   if entry['status']=='COMPLETE_GATEWAY_CUTOFF':v.require(any(entry==coverage for cert in ledger.s['certificates'] for coverage in cert['cutoffs']),'COVERAGE_EXACT_PREFIX')
   else:v.require(any(s['command']['payload'].get('proof',{}).get('trusted_observation_ref')==entry['observation'] and s['command']['payload'].get('proof',{}).get('fact_kind')=='SEAL' and s['command']['payload']['proof']['host']==entry['gateway'] for s in ledger.s['segments']),'COVERAGE_SEAL_OBSERVATION')
 if prefix['host']!=e['store'] or prefix['root']!=ledger.journal(e['store'])['root']:return dict(status='UNSUPPORTED',reason='comparison requires this pinned central snapshot')
 replacement=int(request['policy']['resolution_atoms']);delta=0;premium=0;resolution=e['families'][0]['key']
 for effect in ledger.s['actions']:
  a=effect['body'];amount=int(a['signed_atoms'])
  if a['book']=='RETAIL' and a['kind']=='ORDINARY':
   alt=replacement if a['case'][0]==resolution else amount;premium+=max(0,alt)
   if premium>int(e['premium_cap']):return dict(status='POLICY_FAILURE',at_case=a['case'],reason='original premium cap exceeded at this actual acceptance')
   delta+=alt-amount
 result=dict(status='COMPARABLE',expected=prefix,actual=str(ledger.s['customer']),alternative=str(ledger.s['customer']+delta),difference=str(delta),supplier_booked=e['supplier_booked'],coverage=copy.deepcopy(request['coverage']));v.validate_shape('comparison_response',result);return result

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
