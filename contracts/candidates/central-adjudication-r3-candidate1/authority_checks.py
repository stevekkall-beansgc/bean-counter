#!/usr/bin/env python3
"""Focused exact-source attacks and independent customer-oracle chronology checks."""
import base64,copy,hashlib,json,sys
from pathlib import Path
import validate as v
from fixtures import Builder,HERE,customer_story
from reads import reconstruct_stored,expected,compare
RESULTS=[]
def test(name,f):f();RESULTS.append(name)
def reject(f,code):
 try:f()
 except ValueError as error:assert str(error)==code,(code,str(error));return
 raise AssertionError('accepted '+code)
def row(body):
 raw=v.canonical(body);return dict(body=base64.b64encode(raw).decode(),body_hash=hashlib.sha256(raw).hexdigest(),bytes=str(len(raw)))
def replace(trace,old,body):
 r=row(body);trace['initial']['authority_sources']=[r if x['body_hash']==old else x for x in trace['initial']['authority_sources']];trace['initial']['authority_documents']=[r['body_hash'] if x==old else x for x in trace['initial']['authority_documents']];return r['body_hash']
def finish(trace):
 for name in ['authority_sources','authority_documents','authority_observations','trusted_observations','grant_authentications']:trace['initial'][name]=v.sorted_set(trace['initial'][name])
 return trace
def refresh(trace):
 for c in trace['commands']:c['authority']['command']=v.command_hash(c);trace['initial']['authority_observations'].append(v.digest('authority',c['authority']))
 trace['initial']['authority_observations']=v.sorted_set(list(set(trace['initial']['authority_observations'])));return finish(trace)
def negative_vectors():
 base=v.strict((HERE/'minimal-trace.json').read_bytes());vectors={}
 def put(name,trace,code):vectors[name]=(finish(trace),code);reject(lambda:v.replay(trace),code)
 t=copy.deepcopy(base);t['initial']['authority_sources'].pop();put('authority-missing-source',t,'AUTH_SOURCE_MEMBERSHIP')
 t=copy.deepcopy(base);r=next(x for x in t['initial']['authority_sources'] if x['body'].endswith('='));alphabet='ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';n=len(r['body'].rstrip('='))-1;r['body']=r['body'][:n]+alphabet[alphabet.index(r['body'][n])^1]+r['body'][n+1:];put('authority-base64-padbits',t,'BASE64')
 old=base['commands'][0]['authority']['document'];original=next(x for x in base['initial']['authority_sources'] if x['body_hash']==old);body=v.strict(base64.b64decode(original['body']))
 t=copy.deepcopy(base);bad=dict(body,principal='other');r=row(bad);t['initial']['authority_sources'].append(r);t['initial']['authority_documents'].append(r['body_hash']);put('authority-identity-body-conflict',t,'AUTH_SOURCE_IDENTITY')
 for name,field,value,code in [('scope','scope',['wrong','scope'],'AUTH_SOURCE_SCOPE'),('principal','principal','other','AUTH_SOURCE_BINDING'),('revision','revision','2','AUTH_SOURCE_BINDING'),('permission','permissions',['read'],'AUTH_SOURCE_BINDING'),('expired','ends_at','2001-01-01T00:00:00.000000Z','AUTH_SOURCE_WINDOW')]:
  t=copy.deepcopy(base);h=replace(t,old,dict(body,**{field:value}))
  for c in t['commands']:c['authority']['document']=h
  put('authority-'+name,refresh(t),code)
 for name in ['roles','terms','target']:
  t=copy.deepcopy(base);c=next(x for x in t['commands'] if x['kind']=='ENROLL');f=c['payload']['families'][0];ref=f['assent'];doc=v.strict(base64.b64decode(next(x['body'] for x in t['initial']['authority_sources'] if x['body_hash']==ref)))
  if name=='roles':doc['roles']['recipient']='wrong'
  elif name=='terms':doc['terms']='e'*64
  else:doc['target']='wrong'
  f['assent']=replace(t,ref,doc);put('assent-'+name,refresh(t),'AUTH_SOURCE_SCOPE' if name=='target' else 'ASSENT_BINDING')
 return vectors

def retained_controls():
 b=Builder(families=1,gateways=1);tid=b.issue('g0');b.unused(tid);b.close([0]);trace=b.trace();pins=[expected(b.l,h) for h in b.l.s['journals']];segments=copy.deepcopy(b.l.s['segments']);reconstruct_stored(trace,segments,pins)
 assert all(len([o for seg in segments if seg['host']==h for o in seg['objects'] if o['kind']=='AUTHORITY' and o['full_key']==json.loads(identity)])==1 for h,entries in b.l.s['authority_retained'].items() for identity in entries)
 bad=copy.deepcopy(segments);seg=next(s for s in bad if any(o['kind']=='AUTHORITY' for o in s['objects']));seg['objects']=[o for o in seg['objects'] if o['kind']!='AUTHORITY'];reject(lambda:reconstruct_stored(trace,bad,pins),'STORED_SEMANTIC_MISMATCH')
 bad=copy.deepcopy(segments);seg=next(s for s in bad if s['command']['kind']=='ISSUE');seg['dependencies']=[];reject(lambda:reconstruct_stored(trace,bad,pins),'STORED_SEMANTIC_MISMATCH')
 ledger=v.Ledger(trace['initial']);first=trace['commands'][0];ledger.execute(first);ledger.initial['authority_documents']=[];retry=copy.deepcopy(first);retry['authority']['permission']='read';ledger.initial['authority_observations'].append(v.digest('authority',retry['authority']));reject(lambda:ledger.execute(retry),'AUTH_SOURCE_MISSING')
 return b

def delegation_controls():
 b=Builder(families=1,gateways=1);b.p['families'][0]['roles']['payer']='delegated-payer';b.commands=[];b.seq=0;b.enroll();case,tid=b.intake(0,'delegated');b.decide(case,1200);b.reconcile(tid);b.close([0]);b.add('CORRECT',dict(case=case,expected_revision='1',replacement='300',roles=b.p['families'][0]['roles'],assent='a'*64));trace=b.trace();v.replay(trace)
 if '--write' in sys.argv:(HERE/'vectors'/'authority-delegation.json').write_text(json.dumps(trace,separators=(',',':'))+'\n')
 else:assert (HERE/'vectors'/'authority-delegation.json').read_text()==json.dumps(trace,separators=(',',':'))+'\n'
 c=next(x for x in trace['commands'] if x['kind']=='ENROLL');f=c['payload']['families'][0];reference=f['roles']['payer_delegation'];doc=v.strict(base64.b64decode(next(x['body'] for x in trace['initial']['authority_sources'] if x['body_hash']==reference)))
 for field,value,code in [('maximum_exposure','1','DELEGATION_BINDING'),('agreement_ids',['other'],'DELEGATION_BINDING'),('ends_at','2001-01-01T00:00:00.000000Z','DELEGATION_WINDOW'),('acceptor','other','DELEGATION_WINDOW')]:
  t=copy.deepcopy(trace);d=copy.deepcopy(doc);d[field]=value;d['assent']['terms']=v.digest('authority',{k:x for k,x in d.items() if k!='assent'});ref=replace(t,reference,d);enroll=next(x for x in t['commands'] if x['kind']=='ENROLL');family=enroll['payload']['families'][0];family['roles']['payer_delegation']=ref;old=family['assent'];assent=v.strict(base64.b64decode(next(x['body'] for x in t['initial']['authority_sources'] if x['body_hash']==old)));assent['roles']=family['roles'];assent['terms']=v.digest('authority',{k:x for k,x in family.items() if k!='assent'});family['assent']=replace(t,old,assent);refresh(t);reject(lambda:v.replay(t),code)
 return b

def customer_oracle():
 oracle=v.strict((HERE/'customer-oracle.json').read_bytes());b=customer_story();checkpoints=v.strict((HERE/'customer-checkpoints.json').read_bytes());assert checkpoints==b.checkpoints;ledger=v.Ledger(b.trace()['initial']);by_through={x['through']:x for x in checkpoints};rows={x['id']:x for x in oracle['story']['prefixes']};comparison={x['prefix']:x for x in oracle['comparison']['prefix_totals']};denied=None;consumer=None
 for n,c in enumerate(b.commands,1):
  ledger.execute(c)
  if n not in by_through:continue
  point=by_through[n];pin=rows[point['name']];snapshot=ledger.snapshot();assert str(ledger.s['customer'])==pin['customer_signed_obligation_total_atoms'];assert len(snapshot['entitlements'])==pin['consumed_entitlement_count'];assert str(ledger.s['gross'])==pin['adjustment_gross_atoms'];assert {id:p['used'] for id,p in snapshot['pools'].items()}==pin['adjustment_pool_used_atoms'];assert ledger.s['enrollment']['supplier_booked']=='3000'
  request=dict(expected=expected(ledger),policy={'resolution_atoms':'1500'},coverage=[dict(gateway=g,status='UNKNOWN_GATEWAY_COVERAGE') for g in sorted(ledger.s['gateways'])],budget=dict(bytes=str(v.M),pages=str(v.M),segments=str(v.M)));result=compare(ledger,request);assert result['actual']==comparison[point['name']]['original_customer_total_atoms'] and result['alternative']==comparison[point['name']]['alternative_customer_total_atoms']
  if point['name']=='S04':assert len(snapshot['entitlements'])==1 and c['payload']['verdict']=='DENY';denied=v.key(c['payload']['case']);assert snapshot['cases'][denied]['state']=='FINAL_DENY'
  if point['name']=='S06':consumer=copy.deepcopy(snapshot['entitlements']);assert denied!=v.key(c['payload']['case'])
  if point['name']=='S10':assert all(f['closed'] for f in ledger.s['families'].values());assert sum(x['state']=='ADJUSTMENT_PENDING' for x in ledger.s['cases'].values())==3
  if point['name']=='S14':assert c['kind']=='CORRECT' and all(f['closed'] for f in ledger.s['families'].values());assert all(snapshot['entitlements'][k]==value for k,value in consumer.items());assert [x['body']['signed_atoms'] for x in snapshot['actions'][-2:]]==['-500','300'];assert snapshot['cases'][denied]['state']=='FINAL_DENY'
 assert len(checkpoints)==15

def main():
 vectors=negative_vectors()
 for name,(trace,code) in vectors.items():
  path=HERE/'negative-vectors'/(name+'.json');raw=json.dumps(trace,separators=(',',':'))+'\n'
  if '--write' in sys.argv:path.write_text(raw)
  else:assert path.read_text()==raw
  RESULTS.append(name)
 expected_codes=json.loads((HERE/'negative-expectations.json').read_text());expected_codes.update({name+'.json':code for name,(_,code) in vectors.items()});raw=json.dumps(expected_codes,indent=2)+'\n'
 if '--write' in sys.argv:(HERE/'negative-expectations.json').write_text(raw)
 else:assert (HERE/'negative-expectations.json').read_text()==raw
 test('retained-authority-and-current-host-both-required',retained_controls);test('delegation-positive-and-bound-attacks',delegation_controls);test('independent-oracle-all15-prefixes-and-comparisons',customer_oracle)
 print(json.dumps(dict(passed=len(RESULTS),failed=0,ignored=0,tests=RESULTS),indent=2))
if __name__=='__main__':main()
