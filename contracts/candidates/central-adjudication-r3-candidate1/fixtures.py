#!/usr/bin/env python3
"""Deterministic synthetic schedules; semantic expectations live in tests.py."""
import base64,copy,hashlib,json
from pathlib import Path
import validate as v
HERE=Path(__file__).resolve().parent;REPO=HERE.parents[2]
NOW='2026-09-22T12:00:00.000000Z'
class Builder:
 def __init__(self,customer=False,families=5,gateways=4,suppliers=None,pools=None,preparation_order=None):
  minimal=json.loads((HERE/'minimal-trace.json').read_text());self.initial=copy.deepcopy(minimal['initial']);self.p=copy.deepcopy(next(c['payload'] for c in minimal['commands'] if c['kind']=='ENROLL'));self.initial.pop('optional_rounds',None);self.p['preparations']=[];self.commands=[];self.seq=0;self.checkpoints=[]
  if customer:
   rows=json.loads((REPO/'contracts/candidates/v2/goldens/supplier-separation.json').read_text())['seed'];objects=[];hashes={}
   for row in rows:
    raw=v.canonical(row);h=hashlib.sha256(raw).hexdigest();objects.append(dict(kind='ORIGINAL_BASE',full_key=h,body=base64.b64encode(raw).decode(),body_hash=h,bytes=str(len(raw))));hashes[row['kind']]=h
   self.initial.update(original_objects=v.sorted_set(objects),original_base_receipt=hashes['base-acceptance'],original_base_manifest=hashes['target-snapshot'])
   target=next(r['body']['target'] for r in rows if r['kind']=='base-acceptance');self.p.update(scope=rows[0]['scope'],target=target,base_atoms='10000',supplier_booked='3000',base_receipt=hashes['base-acceptance'],base_manifest=hashes['target-snapshot'])
  template=self.p['families'][0];scope=self.p['scope'];target=self.p['target'];names=['resolution','upsell','late-plus','late-minus','late-zero']+[f'family{i}' for i in range(5,families)]
  self.p['families']=[]
  for i,name in enumerate(names[:families]):
   f=copy.deepcopy(template);f.update(key=[scope,'agreement',name,target],ordinary_atoms='1200' if i==0 else '500',prerequisites=[0] if i==1 else []);self.p['families'].append(f)
  self.p['gateways']=[dict(scope=scope,tag=str(i+1)*32,gateway='g'+str(i),route='full-case-sha256-mod/1') for i in range(gateways)]
  roles=template['roles'];negative=copy.deepcopy(roles);negative.update(provider='customer',cost_originator='customer',bearer='vendor',payer='vendor',beneficiary='vendor',recipient='customer')
  self.p['pools']=pools if pools is not None else [dict(id='adjustments',funding='250',positive='100',negative='150',gross='250',authorizations=v.sorted_set([dict(direction=d,roles=negative if d=='NEGATIVE' else roles,assent='a'*64) for d in ['POSITIVE','NEGATIVE','ZERO']]))]
  if customer and pools is None:self.p['pools']=v.sorted_set([dict(id=id,funding=str(amount),positive=str(amount if direction=='POSITIVE' else 0),negative=str(amount if direction=='NEGATIVE' else 0),gross=str(amount),authorizations=[dict(direction=direction,roles=negative if direction=='NEGATIVE' else roles,assent='a'*64)]) for id,direction,amount in [('positive','POSITIVE',100),('negative','NEGATIVE',150),('zero','ZERO',0)]])
  self.p['suppliers']=suppliers or [];self.initial['initial_resources']={h:{d:str(v.M) for d in v.DIMS} for h in ['center']+[g['gateway'] for g in self.p['gateways']]};self.initial['writer_capabilities']={g['gateway']:dict(epoch='1',journal_head=v.ZERO,fence='b'*64) for g in self.p['gateways']};self.initial['initial_counters']={g:{'writer_epoch':{'q':c['epoch'],'R':'0'}} for g,c in self.initial['writer_capabilities'].items()};self.enroll(preparation_order)
 def enroll(self,order=None):
  from authority_fixtures import prepare
  prepare(self);self.l=v.Ledger(self.initial)
  for o in self.initial['original_objects']:o['origin']=dict(store=self.p['store'],scope=self.p['scope'],registration=self.p['registration'],host=self.p['store'],ordinal='1')
  self.l.initial['original_objects']=copy.deepcopy(self.initial['original_objects'])
  self.p['preparations']=[];intent=v.digest('enrollment',{k:a for k,a in self.p.items() if k!='preparations'})
  for n in ([next(x for x in self.p['gateways'] if x['gateway']==g) for g in order] if order else self.p['gateways']):self.add('PREPARE_ENROLL',dict(store=self.p['store'],scope=self.p['scope'],registration=self.p['registration'],gateway=n['gateway'],namespace=n,intent=intent))
  self.p['preparations']=v.sorted_set([self.proof('ENROLL_PREPARATION',n['gateway']) for n in self.p['gateways']]);self.add('ENROLL',self.p)
 def trace(self):
  for name in ['authority_documents','authority_observations','grant_authentications','trusted_observations']:self.initial[name]=v.sorted_set(self.initial[name])
  return dict(format='r3-trace/1',initial=self.initial,commands=self.commands)
 def add(self,kind,payload,expect='COMMITTED',permission=None,key=None,observed_at=None):
  payload=copy.deepcopy(payload)
  if kind in {'DECIDE','CORRECT'} and payload.get('assent')=='a'*64:
   if kind=='DECIDE' and payload['path']=='ADJUSTMENT':
    direction='POSITIVE' if int(payload['signed_atoms'])>0 else 'NEGATIVE' if int(payload['signed_atoms'])<0 else 'ZERO';pool=next(q for q in self.p['pools'] if q['id']==payload['pool']);payload['assent']=next(a['assent'] for a in pool['authorizations'] if a['direction']==direction)
   else:payload['assent']=next(f['assent'] for f in self.p['families'] if f['key']==payload['case'][0])
  self.seq+=1;key=key or [self.p['scope'],'control',str(self.seq)];c=dict(kind=kind,key=key,payload=copy.deepcopy(payload),authority=dict(principal='host',permission=permission or {'ENROLL':'enroll','RECEIVE':'submit','SUPPLEMENT':'submit','DECIDE':'adjust' if payload.get('path')=='ADJUSTMENT' else 'decide','CORRECT':'correct','BEGIN':'close','CLOSE':'close','ABORT':'close','REPLACE_WRITER':'replace'}.get(kind,'capacity'),document=self.authorization,revision='1',observed_at=observed_at or NOW,command=v.ZERO,head=v.ZERO));c['authority']['head']=self.l.journal(self.l.host(c))['root'];c['authority']['command']=v.command_hash(c);
  for initial in [self.initial,self.l.initial]:
   observation=v.digest('authority',c['authority'])
   if observation not in initial['authority_observations']:initial['authority_observations'].append(observation)
  r=self.l.execute(c)
  assert r['status']==expect,(kind,r);self.commands.append(c);return r
 def proof(self,kind,fullkey):
  for seg in reversed(self.l.s['segments']):
   producer={'ENROLL_PREPARATION':'PREPARE_ENROLL','ROUND_PREPARATION':'PREPARE_ROUND','ENROLLMENT':'ENROLL','GRANT':'LOCAL_GRANT','CLAIM':'ISSUE','RECEIPT':'RECEIVE','ALIAS':'RECEIVE','RETURNED_UNUSED':'RETURN_UNUSED','RECONCILIATION':'RECONCILE','RETIREMENT':'RETIRE_GRANT','BEGIN':'BEGIN','SEAL':'SEALED','INSTALLATION':'INSTALL'}.get(kind)
   if producer is not None and seg['command']['kind']!=producer:continue
   if kind=='TERMINAL' and seg['command']['kind'] not in {'CLOSE','ABORT'}:continue
   for o in seg['objects']:
    if o['kind']==kind and o['full_key']==fullkey:
     p=dict(store=self.p['store'],scope=self.p['scope'],registration=self.p['registration'],host=seg['host'],ordinal=seg['ordinal'],segment=v.digest('segment',seg),root=seg['result']['root'],fact_kind=kind,full_key=fullkey,body_hash=o['body_hash'],bytes=o['bytes']);p['trusted_observation_ref']=v.digest('authority',p)
     for initial in [self.initial,self.l.initial]:
      if p['trusted_observation_ref'] not in initial['trusted_observations']:initial['trusted_observations'].append(p['trusted_observation_ref'])
     return p
  raise AssertionError(('no proof',kind,fullkey))
 def owner(self,case):return self.p['gateways'][int(v.digest('route',case),16)%len(self.p['gateways'])]['gateway']
 def grant(self,g,suffix=None):
  n=len(self.l.s['grants'])+1;gid='gr1.'+self.l.s['gateways'][g]['namespace']['tag']+'.'+(suffix or str(n));w=v.WORK['bundles']['local_grant'];r=w['retained'];resource=dict(zip(v.DIMS,[r['segment_bytes'],r['new_trusted_bytes'],r['records'],r['index_path_pages'],r['index_value_pages'],w['peak_workspace']]));gr=dict(id=gid,store='center',registration=self.p['registration'],gateway=g,namespace=next(x for x in self.p['gateways'] if x['gateway']==g),template='complete-ingress/1',resources={d:str(a) for d,a in resource.items()},counters={d:str(a) for d,a in w['counters'].items()},journal_head=self.l.journal(g)['root']);gr['authentication']=v.digest('grant',gr)
  self.initial['grant_authentications'].append(gr['authentication']);self.l.initial['grant_authentications'].append(gr['authentication']);self.add('LOCAL_GRANT',dict(grant=gr,proof=self.proof('ENROLLMENT',self.p['registration'])));self.add('REGISTER_GRANT',dict(grant=gr,proof=self.proof('GRANT',gid)));return gid
 def claim(self,gid,category='ORDINARY',expect='COMMITTED'):
  g=self.l.s['grants'][gid]['body']['gateway'];tid='token'+str(len(self.l.s['tokens'])+1);allocation=str(self.l.s['gateways'][g]['allocation']+1);token=dict(id=tid,grant=gid,gateway=g,allocation=allocation,category=category,claim=v.digest('claim',[gid,tid,g,allocation,category]));self.add('ISSUE',dict(grant=gid,token=token),expect=expect);return tid
 def issue(self,g,category='ORDINARY'):return self.claim(self.grant(g),category)
 def receive(self,tid,case,delivery=None):
  t=self.l.s['tokens'][tid]['body'];g=t['gateway'];self.add('ACTIVATE',dict(token=tid,gateway=g,proof=self.proof('CLAIM',tid)));delivery=delivery or [self.p['scope'],'source','gw1.'+self.l.s['gateways'][g]['namespace']['tag']+'.'+tid];return self.add('RECEIVE',dict(token=tid,gateway=g,epoch='1',delivery=delivery,submission=dict(case=case,occurred_at=NOW,evidence=[],sender_backfill=False),received_at=NOW),key=delivery)
 def intake(self,family,business):
  case=[self.p['families'][family]['key'],'source',business];tid=self.issue(self.owner(case));self.receive(tid,case);self.import_token(tid);return case,tid
 def import_token(self,tid):
  t=self.l.s['tokens'][tid];kind='ALIAS' if t['state']=='ALIAS' else 'RECEIPT';self.add('IMPORT',dict(token=tid,proof=self.proof(kind,tid)))
 def reconcile(self,tid):
  t=self.l.s['tokens'][tid];kind=t['state'] if t['state']in {'ALIAS','RETURNED_UNUSED'} else 'RECEIPT';self.add('RECONCILE',dict(token=tid,proof=self.proof(kind,tid)));self.add('LOCAL_TERMINAL',dict(grant=t['body']['grant'],gateway=t['body']['gateway'],proof=self.proof('RECONCILIATION',tid)))
 def unused(self,tid):
  t=self.l.s['tokens'][tid]['body'];self.add('RETURN_UNUSED',dict(token=tid,gateway=t['gateway'],claim=t['claim'],proof=self.proof('CLAIM',tid)));self.reconcile(tid)
 def advance(self):
  for g,gw in self.l.s['gateways'].items():
   while gw['allocation_prefix']<gw['allocation']:
    n=gw['allocation_prefix']+1;t=next(t for t in self.l.s['tokens'].values() if t['body']['gateway']==g and int(t['body']['allocation'])==n)
    if not t['reconciled']:break
    self.add('ADVANCE',dict(gateway=g,through=str(n)));gw=self.l.s['gateways'][g]
   while gw['receipt_prefix']<gw['receipt']:
    n=gw['receipt_prefix']+1;t=self.l.s['tokens'][gw['receipts'][n]]
    if not t['imported']:break
    self.add('ADVANCE_RECEIPT',dict(gateway=g,through=str(n)));gw=self.l.s['gateways'][g]
 def decide(self,case,a,verdict='ALLOW',path='ORDINARY'):
  terms=self.l.s['families'][v.key(case[0])]['terms'];roles=terms['roles']
  pool='none'
  if path=='ADJUSTMENT':
   direction='POSITIVE' if a>0 else 'NEGATIVE' if a<0 else 'ZERO';matching=[(q,x) for q in self.p['pools'] for x in q['authorizations'] if x['direction']==direction];selected,au=matching[0];roles=au['roles'];pool=selected['id']
  return self.add('DECIDE',dict(case=case,verdict=verdict,path=path,signed_atoms=str(a),pool=pool,roles=roles,assent='a'*64,reason='explicit retained decision'))
 def begin(self,families,mode='FINISH_ONLY',gateways=None):
  n=str(self.l.s['last_round']+1);preparations=[];selected=gateways or sorted(self.l.s['gateways'])
  if mode=='CANCELLABLE':
   for g in selected:
    self.add('PREPARE_ROUND',dict(round=n,predecessor=str(self.l.s['gateways'][g]['installed']),gateway=g,mode=mode,enrollment=v.digest('enrollment',self.l.s['enrollment']),proof=self.proof('ENROLLMENT',self.p['registration'])));preparations.append(self.proof('ROUND_PREPARATION',v.digest('namespace',[g,n])))
  self.add('BEGIN',dict(preparations=v.sorted_set(preparations),round=n,predecessor=str(self.l.s['last_round']),mode=mode,families=v.sorted_set([self.p['families'][i]['key'] for i in families]),gateways=sorted(selected)));return n
 def seal(self,n):
  r=self.l.s['rounds'][int(n)]
  for g in r['gateways']:
   self.add('SEAL_BEGIN',dict(round=n,gateway=g,predecessor=str(r['gateway_predecessors'][g]),proof=self.proof('BEGIN',n)));self.add('SEALED',dict(round=n,gateway=g));self.add('DRAIN',dict(round=n,gateway=g,proof=self.proof('SEAL',n)))
  self.add('READY',dict(round=n))
 def install(self,n,outcome):
  for g in self.l.s['rounds'][int(n)]['gateways']:
   self.add('INSTALL',dict(round=n,gateway=g,outcome=outcome,proof=self.proof('TERMINAL',n),begin=self.proof('BEGIN',n)));self.add('ACK_INSTALL',dict(round=n,gateway=g,proof=self.proof('INSTALLATION',n)))
 def close(self,families):
  self.advance();n=self.begin(families);self.seal(n);self.add('CLOSE',dict(round=n,closed_at=NOW));self.install(n,'COMMITTED');return n
 def checkpoint(self,name):self.checkpoints.append(dict(name=name,through=len(self.commands),customer_atoms=str(self.l.s['customer']),supplier_booked=self.p['supplier_booked']))
def customer_story():
 b=Builder(customer=True);b.checkpoint('S00');resolution,rt=b.intake(0,'resolution');b.checkpoint('S01');b.decide(resolution,1200);b.checkpoint('S02');denied,dt=b.intake(1,'denied-upsell');b.checkpoint('S03');b.decide(denied,0,'DENY');b.checkpoint('S04');upsell,ut=b.intake(1,'qualified-upsell');b.checkpoint('S05');b.decide(upsell,500);b.checkpoint('S06');late=[]
 for i in [2,3,4]:late.append(b.intake(i,'late-'+str(i)));b.checkpoint('S'+str(i+5).zfill(2))
 for tid in list(b.l.s['tokens']):b.reconcile(tid)
 b.close([0,1,2,3,4]);b.checkpoint('S10');b.decide(late[0][0],100,path='ADJUSTMENT');b.checkpoint('S11');b.decide(late[1][0],-150,path='ADJUSTMENT');b.checkpoint('S12');b.decide(late[2][0],0,path='ADJUSTMENT');b.checkpoint('S13');b.add('CORRECT',dict(case=upsell,expected_revision='1',replacement='300',roles=b.p['families'][1]['roles'],assent='a'*64));b.checkpoint('S14');return b
if __name__=='__main__':
 import sys
 b=customer_story();outputs={'customer-trace.json':b.trace(),'customer-checkpoints.json':b.checkpoints}
 for name,value in outputs.items():
  raw=json.dumps(value,indent=2)+'\n'
  if '--write' in sys.argv:(HERE/name).write_text(raw)
  else:assert (HERE/name).read_text()==raw,'CUSTOMER_DERIVATION_MISMATCH'
 print(json.dumps(b.l.summary(len(b.commands)),indent=2))
