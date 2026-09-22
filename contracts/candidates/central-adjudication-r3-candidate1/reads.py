"""Canonical bounded read session; no actual-store durability/nonposting proof."""
import copy,hashlib
import validate as v
class Reader:
 def __init__(self,ledger):self.ledger=ledger;self.sessions={}
 def start(self,expected):
  v.validate_shape('expected_prefix',expected);e=self.ledger.s['enrollment'];v.require(expected['store']==e['store'] and expected['scope']==e['scope'] and expected['registration']==e['registration'],'EXPECTED_BINDING');stream=[s for s in self.ledger.s['segments'] if s['host']==expected['host'] and int(s['ordinal'])<=int(expected['ordinal'])];v.require(len(stream)==int(expected['ordinal']) and bool(stream),'EXPECTED_PREFIX');last=stream[-1];v.require(last['result']['root']==expected['root'] and v.digest('segment',last)==expected['segment'],'EXPECTED_PREFIX');return dict(expected=copy.deepcopy(expected),stream=stream,ordinal=0,offset=0,root=v.ZERO,previous=v.ZERO)
 def read(self,request):
  v.validate_shape('read_request',request)
  if 'cursor' in request:
   token=request['cursor']['continuation'];v.require(token in self.sessions and self.cursor(self.sessions[token])==request['cursor'],'UNVERIFIED_CURSOR');state=dict(self.sessions[token]);v.require(state['expected']==request['expected'],'CURSOR_PREFIX')
  else:state=self.start(request['expected'])
  measured=dict(bytes=0,pages=0,segments=0);budget={k:int(a) for k,a in request['budget'].items()}
  while state['ordinal']<len(state['stream']):
   raw=v.canonical(state['stream'][state['ordinal']]);remaining=len(raw)-state['offset'];take=min(4096,remaining,budget['bytes']-measured['bytes'])
   if take<=0 or measured['pages']>=budget['pages'] or measured['segments']>=budget['segments']:break
   state['offset']+=take;measured['bytes']+=take;measured['pages']+=1
   if state['offset']==len(raw):
    segment=state['stream'][state['ordinal']];v.require(segment['previous']==state['previous'] and segment['previous_root']==state['root'],'PREFIX_CHAIN');v.validate_shape('segment',segment);state['previous']=v.digest('segment',segment);state['root']=segment['result']['root'];state['ordinal']+=1;state['offset']=0;measured['segments']+=1
  measured={k:str(a) for k,a in measured.items()}
  if state['ordinal']==len(state['stream']):result=dict(status='COMPLETE',expected=copy.deepcopy(state['expected']),measured=measured)
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
  if entry['status']!='UNKNOWN_GATEWAY_COVERAGE':v.require(entry['observation'] in ledger.initial['trusted_observations'],'COVERAGE_AUTHORITY')
 if prefix['host']!=e['store'] or prefix['root']!=ledger.journal(e['store'])['root']:return dict(status='UNSUPPORTED',reason='comparison requires this pinned central snapshot')
 replacement=int(request['policy']['resolution_atoms']);delta=0;premium=0;resolution=e['families'][0]['key']
 for effect in ledger.s['actions']:
  a=effect['body'];amount=int(a['signed_atoms'])
  if a['book']=='RETAIL' and a['kind']=='ORDINARY':
   alt=replacement if a['case'][0]==resolution else amount;premium+=max(0,alt)
   if premium>int(e['premium_cap']):return dict(status='POLICY_FAILURE',at_case=a['case'],reason='original premium cap exceeded at this actual acceptance')
   delta+=alt-amount
 result=dict(status='COMPARABLE',expected=prefix,actual=str(ledger.s['customer']),alternative=str(ledger.s['customer']+delta),difference=str(delta),supplier_booked=e['supplier_booked'],coverage=copy.deepcopy(request['coverage']));v.validate_shape('comparison_response',result);return result
