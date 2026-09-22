#!/usr/bin/env python3
"""Author conformance, independent constants; actual store/crash proofs pending."""
import copy,json,sys,base64,hashlib
from fractions import Fraction
from pathlib import Path
import validate as v
from fixtures import Builder,customer_story,NOW,HERE
RESULTS=[]
def test(name,f):
 f();RESULTS.append(name);print(name,file=sys.stderr,flush=True)
def reject(f,code=None):
 try:f()
 except (ValueError,AssertionError,KeyError,UnicodeError) as e:
  if code is not None:assert code in str(e),(code,str(e))
  return
 raise AssertionError('accepted invalid input')
def equal(a,b):assert a==b,(a,b)
def no_authoritative(s):return {k:z for k,z in s.items() if k not in {'duplicates','refused'}}
def retry(b,c):
 c=copy.deepcopy(c);c['authority']['permission']='read';c['authority']['head']=v.ZERO;c['authority']['command']=v.command_hash(c)
 for initial in [b.initial,b.l.initial]:
  observation=v.digest('authority',c['authority'])
  if observation not in initial['authority_observations']:initial['authority_observations'].append(observation)
 before=copy.deepcopy(no_authoritative(b.l.s));r=b.l.execute(c);equal(r['status'],'DUPLICATE');equal(no_authoritative(b.l.s),before);b.commands.append(c)
def saturation(b):
 for a in b.l.s['resources'].values():a['provisioned']={d:a['used'][d]+a['held'][d] for d in v.DIMS}
def customer():
 b=customer_story();expected=['10000','10000','11200','11200','11700','11700','11700','11700','11700','11800','11650','11650','11450','11450','11450'];equal([c['customer_atoms'] for c in b.checkpoints],expected);equal(b.l.s['enrollment']['supplier_booked'],'3000');equal(b.l.s['gross'],250);equal(len(b.l.s['entitlements']),5);equal(sum(int(e['body']['signed_atoms']) for e in b.l.s['actions']),1450);equal(10000+1500+500+100-150-500+300,11750)
 for c in list(b.commands):
  retry(b,c);retry(b,c)
def mixed(n,allunused=False):
 b=Builder(families=1,gateways=1);ts=[b.issue('g0') for i in range(n)]
 if not allunused:
  case=[b.p['families'][0]['key'],'source','one'];b.receive(ts[1],case);b.receive(ts[0],case)
  b.add('IMPORT',dict(token=ts[0],proof=b.proof('ALIAS',ts[0])),expect='REFUSED');b.import_token(ts[1]);b.import_token(ts[0]);b.reconcile(ts[1]);b.reconcile(ts[0])
 for tid in ts if allunused else ts[2:]:b.unused(tid)
 saturation(b);b.close([0]);s=b.l.summary(len(b.commands));equal(s['tokens'],n);equal(s['allocation_prefix']['g0'],str(n));equal(s['receipt_prefix']['g0'],'0' if allunused else '1');equal(s['receipts'],0 if allunused else 1);equal(s['aliases'],0 if allunused else 1);equal(len(b.l.s['deliveries']),0 if allunused else 2);equal(s['round'],'1');return b

def pending(n):
 b=Builder(families=1,gateways=1)
 for i in range(n):case,tid=b.intake(0,'case'+str(i));b.reconcile(tid)
 saturation(b);b.close([0]);equal(sum(c['state']=='ADJUSTMENT_PENDING' for c in b.l.s['cases'].values()),n);equal(len({c['transfer'] for c in b.l.s['cases'].values()}),1);equal(len(b.l.s['certificates']),1);equal(len(b.l.s['certificates'][0]['families']),1);return b

def highwater():
 b=Builder(families=1,gateways=1);n=b.begin([0]);late=b.issue('g0','ADJUSTMENT');case=[b.p['families'][0]['key'],'source','above'];b.receive(late,case);b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='0',proof=b.proof('BEGIN',n)));b.add('SEALED',dict(round=n,gateway='g0'));b.add('DRAIN',dict(round=n,gateway='g0',proof=b.proof('SEAL',n)),expect='REFUSED');equal(b.l.s['rounds'][1]['cutoffs']['g0'],0);equal(b.l.s['rounds'][1]['sealed']['g0']['high'],1);b.import_token(late);b.reconcile(late);b.advance();saturation(b);b.add('DRAIN',dict(round=n,gateway='g0',proof=b.proof('SEAL',n)));b.add('READY',dict(round=n));b.add('CLOSE',dict(round=n,closed_at=NOW));b.install(n,'COMMITTED');equal(b.l.s['cases'][v.key(case)]['state'],'ADJUSTMENT_PENDING');return b

def cancellation(stage):
 b=Builder(families=1,gateways=1);tid=b.issue('g0');n=b.begin([0],'CANCELLABLE')
 if stage!='before_begin':b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='0',proof=b.proof('BEGIN',n)))
 if stage in {'sealed','ready'}:
  b.unused(tid);b.advance();b.add('SEALED',dict(round=n,gateway='g0'))
 if stage=='ready':b.add('DRAIN',dict(round=n,gateway='g0',proof=b.proof('SEAL',n)));b.add('READY',dict(round=n))
 b.add('ABORT',dict(round=n));b.install(n,'ABORTED');equal(b.l.s['gateways']['g0']['clock_floor'],'0001-01-01T00:00:00.000000Z')
 if stage not in {'sealed','ready'}:b.unused(tid)
 b.advance();saturation(b);n2=b.begin([0]);old=next(c for c in b.commands if c['kind']=='ABORT');retry(b,old);equal(b.l.s['active'],2);b.add('ABORT',dict(round=n2),expect='REFUSED');b.seal(n2);b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='0',proof=b.proof('BEGIN',n)),expect='REFUSED');b.add('INSTALL',dict(round=n,gateway='g0',outcome='ABORTED',proof=b.proof('TERMINAL',n),begin=b.proof('BEGIN',n)),expect='REFUSED');b.add('CLOSE',dict(round=n2,closed_at=NOW));b.install(n2,'COMMITTED');return b

def resource_boundary(d,offset):
 initial=json.loads((HERE/'minimal-trace.json').read_text())['initial'];initial.pop('optional_rounds',None);l=v.Ledger(initial);needed=v.vec('IMPORT')[d];l.s['resources']['center']['provisioned'][d]=needed+offset
 if offset<0:reject(lambda:l.reserve('center','probe',['IMPORT']),'RESOURCE_'+d)
 else:
  l.reserve('center','probe',['IMPORT']);l.spend('probe','IMPORT');l.terminal_slack('probe');equal(l.s['resources']['center']['used'][d],0 if d=='workspace_bytes' else needed)
def counter_boundary(name,offset):
 initial=json.loads((HERE/'minimal-trace.json').read_text())['initial'];initial.pop('optional_rounds',None);l=v.Ledger(initial);kind=next(k for k,r in v.WORK['transitions'].items() if r['counter_increments'][name]);cost=v.WORK['transitions'][kind]['counter_increments'][name];l.s['counters']['center'][name]['q']=v.M-cost+offset
 if offset>0:reject(lambda:l.reserve('center','probe',[kind]),'COUNTER_'+name)
 else:l.reserve('center','probe',[kind]);l.spend('probe',kind);l.terminal_slack('probe');equal(l.s['counters']['center'][name],{'q':v.M+offset,'R':0})
def supplier(last,batch=False,zero=False):
 b=Builder(families=2,gateways=1,suppliers=[dict(id='pool',maximum='500',consumed='500' if zero else '300',held='0' if zero else '200',released='0')])
 p=copy.deepcopy(b.p)
 for f in p['families']:f.update(book='SUPPLIER',supplier_pool='pool',ordinary_atoms='30')
 b=reenroll(b,p)
 if last and not zero:
  case,tid=b.intake(0,'supplier-consume30');b.decide(case,30);b.reconcile(tid);equal(b.l.s['suppliers']['pool']['consumed'],'330');equal(b.l.s['suppliers']['pool']['held'],'170')
 b.close([0,1] if batch else [0]);equal(b.l.s['suppliers']['pool']['held'],'0' if batch or zero else '170' if last else '200')
 if not batch:b.close([1])
 equal(b.l.s['suppliers']['pool']['held'],'0');equal(b.l.s['suppliers']['pool']['released'],'0' if zero else '170' if last else '200');assert b.l.s['certificates'][-1]['supplier_after'];return b
def reenroll(b,p):
 b.p=p;b.commands=[];b.seq=0;b.initial['authority_observations']=[];b.initial['trusted_observations']=[];b.initial['grant_authentications']=[];b.l=v.Ledger(b.initial);b.enroll();return b

def funded_replay(b,name,wide_center=False):
 trace=b.trace();trace['initial']['initial_resources']={h:{d:str(n) for d,n in values.items()} for h,values in b.l.peaks.items()}
 if wide_center:trace['initial']['initial_resources']['center']={d:str(v.M) for d in v.DIMS}
 rebuilt=v.replay(trace);equal(rebuilt['summary']['root'],b.l.summary(len(b.commands))['root'])
 out=HERE/'vectors';raw=json.dumps(trace,separators=(',',':'))+'\n'
 if '--write-vectors' in sys.argv:out.mkdir(exist_ok=True);(out/(name+'.json')).write_text(raw)
 else:equal((out/(name+'.json')).read_text(),raw)
 return rebuilt

def namespace_routes():
 b=Builder(families=1,gateways=4);owners={}
 for i in range(100):
  case=[b.p['families'][0]['key'],'source',str(i)];owners.setdefault(b.owner(case),case)
 equal(len(owners),4)
 for owner,case in owners.items():
  for arrival in b.l.s['gateways']:
   tid=b.issue(arrival);b.add('ACTIVATE',dict(token=tid,gateway=arrival,proof=b.proof('CLAIM',tid)));delivery=[b.p['scope'],'source','gw1.'+b.l.s['gateways'][arrival]['namespace']['tag']+'.route'+tid];payload=dict(token=tid,gateway=arrival,epoch='1',delivery=delivery,submission=dict(case=case,occurred_at=NOW,evidence=[],sender_backfill=False),received_at=NOW)
   b.add('RECEIVE',payload,key=delivery,expect='COMMITTED' if owner==arrival else 'REFUSED')
 b=Builder(families=1,gateways=1)
 for i,suffix in enumerate(['x'*90,'x'*91,'x'*92,'é'*45+'x','é'*46,'']):
  case=[b.p['families'][0]['key'],'source','suffix'+str(i)];tid=b.issue('g0');b.add('ACTIVATE',dict(token=tid,gateway='g0',proof=b.proof('CLAIM',tid)));delivery=[b.p['scope'],'source','gw1.'+'1'*32+'.'+suffix];p=dict(token=tid,gateway='g0',epoch='1',delivery=delivery,submission=dict(case=case,occurred_at=NOW,evidence=[],sender_backfill=False),received_at=NOW)
  if len(delivery[2].encode())>128:reject(lambda:b.add('RECEIVE',p,key=delivery))
  else:b.add('RECEIVE',p,key=delivery,expect='REFUSED' if not suffix else 'COMMITTED')

def reads_and_comparison():
 from reads import Reader,expected,compare
 b=customer_story();e=expected(b.l);reader=Reader(b.l);request=dict(expected=e,budget=dict(bytes='8192',pages='2',segments='2'));before=copy.deepcopy(no_authoritative(b.l.s));count=0
 while True:
  r=reader.read(request);count+=1
  if r['status']=='COMPLETE':break
  request['cursor']=r['cursor'];assert count<1000
 equal(no_authoritative(b.l.s),before);equal(compare(b.l,dict(expected=e,coverage=[dict(gateway=g,status='UNKNOWN_GATEWAY_COVERAGE') for g in sorted(b.l.s['gateways'])],policy=dict(resolution_atoms='1500'),budget=dict(bytes=str(v.M),pages=str(v.M),segments=str(v.M))))['alternative'],'11750')
 equal(compare(b.l,dict(expected=e,coverage=[dict(gateway=g,status='UNKNOWN_GATEWAY_COVERAGE') for g in sorted(b.l.s['gateways'])],policy=dict(resolution_atoms='6000'),budget=dict(bytes=str(v.M),pages=str(v.M),segments=str(v.M))))['status'],'POLICY_FAILURE')
 equal(compare(b.l,dict(expected=e,coverage=[dict(gateway=g,status='UNKNOWN_GATEWAY_COVERAGE') for g in sorted(b.l.s['gateways'])],policy=dict(resolution_atoms='1500'),budget=dict(bytes='1',pages='1',segments='1')))['status'],'INCOMPLETE')
 forged=copy.deepcopy(request);forged['cursor']['byte_offset']='999';reject(lambda:reader.read(forged),'UNVERIFIED_CURSOR')
 receipt=next(iter(b.l.s['deliveries'].values()))['receipt'];response=dict(receipt=receipt,knowledge='UNKNOWN',current_lifecycle='UNKNOWN',central_admission='UNKNOWN',coverage=[]);v.validate_shape('retry_response',response);bad=copy.deepcopy(response);bad['current_lifecycle']='FINAL_ALLOW';reject(lambda:v.validate_shape('retry_response',bad))

def retirement():
 b=Builder(families=1,gateways=1);tid=b.issue('g0');gid=b.l.s['tokens'][tid]['body']['grant'];b.add('RETIRE_GRANT',dict(grant=gid),expect='REFUSED');b.unused(tid);b.add('ACTIVATE',dict(token=tid,gateway='g0',proof=b.proof('CLAIM',tid)),expect='REFUSED');equal(b.l.s['tokens'][tid]['state'],'RETURNED_UNUSED')
 # Separate fresh unclaimed grant branch: exact existing local/registration commands.
 original=copy.deepcopy(b.trace());b=Builder(families=1,gateways=1);commands=[c for c in original['commands'] if c['kind'] in {'LOCAL_GRANT','REGISTER_GRANT'}]
 for c in commands:
  c=copy.deepcopy(c);c['authority']['head']=b.l.journal(b.l.host(c))['root'];c['authority']['command']=v.command_hash(c)
  for name in ['grant_authentications','trusted_observations']:b.initial[name]=original['initial'][name];b.l.initial[name]=original['initial'][name]
  b.add(c['kind'],c['payload'])
 b.add('RETIRE_GRANT',dict(grant=gid));b.claim(gid,expect='REFUSED');b.add('LOCAL_TERMINAL',dict(grant=gid,gateway='g0',proof=b.proof('RETIREMENT',gid)));equal(b.l.s['grants'][gid]['local_terminal'],True);funded_replay(b,'grant-retirement')

def rollback():
 b=Builder(families=1,gateways=1);case,tid=b.intake(0,'pending');b.reconcile(tid);b.close([0]);p=dict(case=case,verdict='ALLOW',path='ADJUSTMENT',signed_atoms='100',pool='adjustments',roles=b.p['families'][0]['roles'],assent='a'*64,reason='attempt')
 saturation(b);before=copy.deepcopy(no_authoritative(b.l.s));b.add('DECIDE',p,expect='REFUSED');equal(no_authoritative(b.l.s),before)
 # Structurally valid wrong authority head raises after touched-map isolation.
 c=copy.deepcopy(b.commands[-1]);c['authority']['head']='f'*64;observation=v.digest('authority',c['authority']);b.l.initial['authority_observations'].append(observation);reject(lambda:b.l.execute(c),'AUTH_HEAD');equal(no_authoritative(b.l.s),before)
 for command in list(b.commands):
  original=copy.deepcopy(command);original['key']=[b.p['scope'],'mutant',str(len(b.l.initial['authority_observations']))];original['authority']['head']='e'*64;original['authority']['command']=v.command_hash(original)
  if original['kind']=='RECEIVE':original['payload']['delivery']=original['key'];original['authority']['command']=v.command_hash(original)
  b.l.initial['authority_observations'].append(v.digest('authority',original['authority']));state=copy.deepcopy(no_authoritative(b.l.s));
  try:equal(b.l.execute(original)['status'],'REFUSED')
  except v.Invalid:pass
  equal(no_authoritative(b.l.s),state)

def uncovered_issuance():
 b=Builder(families=1,gateways=1);eligible=b.grant('g0')
 for i in range(257):
  tid='unbacked'+str(i);gid='absent'+str(i);token=dict(id=tid,grant=gid,gateway='g0',allocation='1',category='ORDINARY',claim=v.digest('claim',[gid,tid,'g0','1','ORDINARY']));before=copy.deepcopy(no_authoritative(b.l.s));b.add('ISSUE',dict(grant=gid,token=token),expect='REFUSED');equal(no_authoritative(b.l.s),before)
 tid=b.claim(eligible);original=copy.deepcopy(b.commands[-1])
 for i in range(257):b.claim(eligible,expect='REFUSED');retry(b,original)
 equal(len(b.l.s['tokens']),1);b.unused(tid);b.close([0]);funded_replay(b,'uncovered-issuance',wide_center=True)

def repeated_cancel():
 b=Builder(families=1,gateways=1)
 for _ in range(2):n=b.begin([0],'CANCELLABLE');b.add('ABORT',dict(round=n));b.install(n,'ABORTED')
 saturation(b);before=copy.deepcopy(no_authoritative(b.l.s));b.add('PREPARE_ROUND',dict(round='3',predecessor='2',gateway='g0',mode='CANCELLABLE',enrollment=v.digest('enrollment',b.l.s['enrollment']),proof=b.proof('ENROLLMENT',b.p['registration'])),expect='REFUSED');equal(no_authoritative(b.l.s),before);b.close([0]);funded_replay(b,'optional-round-exhaustion')

def writer_epoch():
 b=Builder(families=1,gateways=1);tid=b.issue('g0');b.add('ACTIVATE',dict(token=tid,gateway='g0',proof=b.proof('CLAIM',tid)));b.add('REPLACE_WRITER',dict(gateway='g0',old_epoch='1',new_epoch='2',journal_head=b.l.journal('g0')['root'],fence='b'*64));case=[b.p['families'][0]['key'],'source','writer'];delivery=[b.p['scope'],'source','gw1.'+'1'*32+'.writer'];p=dict(token=tid,gateway='g0',epoch='1',delivery=delivery,submission=dict(case=case,occurred_at=NOW,evidence=[],sender_backfill=False),received_at=NOW);b.add('RECEIVE',p,key=delivery,expect='REFUSED');p['epoch']='2';b.add('RECEIVE',p,key=delivery);b.import_token(tid);b.reconcile(tid);b.close([0]);funded_replay(b,'writer-epoch')

def measured_envelopes():
 b=customer_story()
 for segment in b.l.s['segments']:
  row=v.WORK['transitions'][segment['command']['kind']];trusted=len(v.canonical(segment['command']))+len(v.canonical(segment['result']))+sum(int(o['bytes']) for o in {o['body_hash']:o for o in segment['objects']}.values());assert len(v.canonical(segment))<=row['segment_bytes']<=8388608;assert trusted<=row['new_trusted_bytes']<=2097152;assert len(v.canonical(segment['command']))<=row['command_bytes']<=262144
 from index_model import self_test
 self_test()

def truncation():
 from reads import Reader,expected
 b=customer_story();e=expected(b.l);historical=dict(e);first=next(s for s in b.l.s['segments'] if s['host']=='center');historical.update(ordinal=first['ordinal'],segment=v.digest('segment',first),root=first['result']['root']);reader=Reader(b.l);equal(reader.read(dict(expected=historical,budget=dict(bytes=str(v.M),pages=str(v.M),segments=str(v.M))))['status'],'COMPLETE');b.l.read_index['center'].pop();reject(lambda:Reader(b.l).start(e),'EXPECTED_PREFIX')

def topology():
 b=Builder(families=32,gateways=4,suppliers=[dict(id='supplier'+str(i),maximum='500',consumed='330',held='170',released='0') for i in range(8)]);p=copy.deepcopy(b.p);p['pools']=v.sorted_set([dict(p['pools'][0],id='pool'+str(i)) for i in range(3)])
 for i in range(24,32):p['families'][i].update(book='SUPPLIER',supplier_pool='supplier'+str(i-24),ordinary_atoms='30')
 b=reenroll(b,p);b.close(list(range(32)));equal([s['released'] for s in b.l.s['suppliers'].values()],['170']*8);funded_replay(b,'max-topology')

def gross_boundaries():
 for cap in [249,250,251]:
  b=Builder(families=2,gateways=1);p=copy.deepcopy(b.p);p['pools'][0]['gross']=str(cap);p['pools'][0]['funding']='1000';b=reenroll(b,p);a,ta=b.intake(0,'plus');z,tz=b.intake(1,'minus');b.reconcile(ta);b.reconcile(tz);b.close([0,1]);b.decide(a,100,path='ADJUSTMENT')
  if cap>=250:b.decide(z,-150,path='ADJUSTMENT');equal(b.l.s['gross'],250)
  else:
   negative=next(au['roles'] for au in p['pools'][0]['authorizations'] if au['direction']=='NEGATIVE');before=copy.deepcopy(no_authoritative(b.l.s));b.add('DECIDE',dict(case=z,verdict='ALLOW',path='ADJUSTMENT',signed_atoms='-150',pool='adjustments',roles=negative,assent='a'*64,reason='negative'),expect='REFUSED');equal(no_authoritative(b.l.s),before)
  funded_replay(b,'gross'+str(cap))

def directional_funding():
 for name,funding,positive,negative,order in [('shared200-plus-first',200,100,150,[100,-150]),('shared200-minus-first',200,100,150,[-150,100]),('positive99',1000,99,150,[100]),('negative149',1000,100,149,[-150])]:
  b=Builder(families=2,gateways=1);p=copy.deepcopy(b.p);p['pools'][0].update(funding=str(funding),positive=str(positive),negative=str(negative));b=reenroll(b,p);cases=[]
  for i in range(2):case,tid=b.intake(i,'direction'+str(i));b.reconcile(tid);cases.append(case)
  b.close([0,1]);used=0
  for i,amount in enumerate(order):
   roles=next(au['roles'] for au in p['pools'][0]['authorizations'] if au['direction']==('POSITIVE' if amount>0 else 'NEGATIVE'));payload=dict(case=cases[i],verdict='ALLOW',path='ADJUSTMENT',signed_atoms=str(amount),pool='adjustments',roles=roles,assent='a'*64,reason='directional authority');allowed=used+abs(amount)<=funding and max(amount,0)<=positive and max(-amount,0)<=negative;b.add('DECIDE',payload,expect='COMMITTED' if allowed else 'REFUSED')
   if allowed:used+=abs(amount)
  equal(b.l.s['gross'],used);funded_replay(b,name)

def clock_floor():
 b=Builder(families=1,gateways=1);b.close([0]);tid=b.issue('g0','ADJUSTMENT');b.add('ACTIVATE',dict(token=tid,gateway='g0',proof=b.proof('CLAIM',tid)));case=[b.p['families'][0]['key'],'source','clock'];delivery=[b.p['scope'],'source','gw1.'+'1'*32+'.clock'];earlier='2026-09-22T11:59:59.000000Z';p=dict(token=tid,gateway='g0',epoch='1',delivery=delivery,submission=dict(case=case,occurred_at=earlier,evidence=[],sender_backfill=False),received_at=earlier);r=b.add('RECEIVE',p,key=delivery,observed_at=earlier,expect='REFUSED');equal(r['code'],'CLOCK_BEHIND');p['received_at']=NOW;b.add('RECEIVE',p,key=delivery);b.import_token(tid);b.reconcile(tid);b.advance();funded_replay(b,'clock-floor')

def encoding():
 for raw in [b'\xef\xbb\xbf{}',b'{"a":1,"a":2}',b'{"a":-0}',b'{"a":1.0}',b'{"a":1e0}',b'{"a":9007199254740992}',b'{"a":"\xff"}',b'{"a":"\\ud800"}']:reject(lambda raw=raw:v.strict(raw))
 equal(v.canonical({'\ue000':1,'😀':2}),'{"😀":2,"\ue000":1}'.encode());equal(v.canonical({'x':'\b\t\n\f\r\u0001"\\'}),b'{"x":"\\b\\t\\n\\f\\r\\u0001\\"\\\\"}')
 for n in [0,1,4096]:raw=b'x'*n;e=dict(body=base64.b64encode(raw).decode(),sha256=hashlib.sha256(raw).hexdigest());v.validate_shape('evidence',e)
 reject(lambda:v.validate_shape('evidence',dict(body=base64.b64encode(b'x'*4097).decode(),sha256='a'*64)))
 for n in [v.M-1,v.M]:v.validate_shape('count',str(n))
 reject(lambda:v.validate_shape('count',str(v.M+1)))

def base_mutations():
 from base_bridge import verify
 b=Builder(customer=True);objects=copy.deepcopy(b.initial['original_objects']);o=next(o for o in objects if json.loads(base64.b64decode(o['body']))['kind']=='base-posting');r=json.loads(base64.b64decode(o['body']));r['body']['roles']['payer']='different';raw=v.canonical(r);o.update(body=base64.b64encode(raw).decode(),body_hash=hashlib.sha256(raw).hexdigest(),bytes=str(len(raw)));reject(lambda:verify(objects,b.p,v.canonical,v.require))

def delayed_seal():
 b=Builder(families=1,gateways=1);gid=b.grant('g0');n=b.begin([0]);b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='0',proof=b.proof('BEGIN',n)));b.add('SEALED',dict(round=n,gateway='g0'));local=copy.deepcopy(b.l.s['resources']['g0']);b.claim(gid,expect='REFUSED');tid=b.claim(gid,'ADJUSTMENT');equal(b.l.s['resources']['g0'],local);b.add('ACTIVATE',dict(token=tid,gateway='g0',proof=b.proof('CLAIM',tid)),expect='REFUSED');b.unused(tid);b.advance();b.add('DRAIN',dict(round=n,gateway='g0',proof=b.proof('SEAL',n)));b.add('READY',dict(round=n));b.add('CLOSE',dict(round=n,closed_at=NOW));b.install(n,'COMMITTED');equal(b.l.s['certificates'][-1]['cutoffs'][0]['cutoff'],'0');equal(b.l.s['certificates'][-1]['cutoffs'][0]['receipt_high'],'0');return b

def host_identity():
 b=Builder(families=1,gateways=2);shared=[b.p['scope'],'same-control','same'];b.add('EXTEND_RESOURCES',dict(host='g0',resources={d:'0' for d in v.DIMS}),key=shared);b.add('EXTEND_RESOURCES',dict(host='g1',resources={d:'0' for d in v.DIMS}),key=shared);a=b.grant('g0','same');z=b.grant('g1','same');assert a!=z
 cases=[[b.p['families'][0]['key'],'source','host'+str(i)] for i in range(100)];case=next(c for c in cases if b.owner(c)=='g0');tid=b.claim(a);b.receive(tid,case);original=copy.deepcopy(b.commands[-1]);other=b.claim(z);b.add('ACTIVATE',dict(token=other,gateway='g1',proof=b.proof('CLAIM',other)));p=copy.deepcopy(original['payload']);p.update(gateway='g1',token=other);before=copy.deepcopy(no_authoritative(b.l.s));b.add('RECEIVE',p,key=original['key'],permission='read',expect='REFUSED');equal(no_authoritative(b.l.s),before)
 # Occupied full delivery key conflicts before a possible semantic case alias.
 p=copy.deepcopy(original['payload']);p['submission']['sender_backfill']=True;b.add('RECEIVE',p,key=original['key'],permission='read',expect='REFUSED');equal(no_authoritative(b.l.s),before)
 b.import_token(tid);b.reconcile(tid);b.unused(other);b.close([0]);return b

def provenance_attacks():
 b=Builder(families=1,gateways=1);case,tid=b.intake(0,'known');b.reconcile(tid);proof=b.proof('RECEIPT',tid)
 for label in ['wrong-key','wrong-source']:
  trace=copy.deepcopy(b.trace());q=copy.deepcopy(proof)
  if label=='wrong-key':q['full_key']='other-token'
  else:
   copied=next(seg for seg in b.l.s['segments'] if seg['host']=='center' and any(o['kind']=='RECEIPT' for o in seg['objects']));q.update(host='center',ordinal=copied['ordinal'],segment=v.digest('segment',copied),root=copied['result']['root'])
  q['trusted_observation_ref']=v.digest('authority',{a:x for a,x in q.items() if a!='trusted_observation_ref'});trace['initial']['trusted_observations']=v.sorted_set(list(set(trace['initial']['trusted_observations']+[q['trusted_observation_ref']])))
  c=copy.deepcopy(next(c for c in trace['commands'] if c['kind']=='IMPORT'));c['key']=[b.p['scope'],'attack',label];c['payload']['proof']=q;c['authority']['head']=b.l.journal('center')['root'];c['authority']['command']=v.command_hash(c);trace['initial']['authority_observations']=v.sorted_set(list(set(trace['initial']['authority_observations']+[v.digest('authority',c['authority'])])));trace['commands'].append(c);reject(lambda:v.replay(trace),'PROOF_KIND_KEY' if label=='wrong-key' else 'PROOF_MEMBERSHIP')
  out=HERE/'negative-vectors';raw=json.dumps(trace,separators=(',',':'))+'\n'
  if '--write-vectors' in sys.argv:out.mkdir(exist_ok=True);(out/(label+'.json')).write_text(raw)
  else:equal((out/(label+'.json')).read_text(),raw)
 # Fresh ZERO journals cannot invent prior ordinal/counter progress.
 initial=copy.deepcopy(b.initial);initial['initial_counters']['center']={'segment':{'q':'1','R':'0'}};reject(lambda:v.Ledger(initial),'INITIAL_COUNTER_ANCHOR')


def seal_scan():
 from seal_reader import SealReader
 b=Builder(families=1,gateways=1)
 for i in range(3):b.unused(b.issue('g0'))
 b.advance();n=b.begin([0]);b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='0',proof=b.proof('BEGIN',n)));before=copy.deepcopy(no_authoritative(b.l.s));reader=SealReader(b.l,'g0',n);first=reader.read(1,1);equal(first['status'],'INCOMPLETE');assert 'receipt_root' not in first;reader.abort();reject(lambda:reader.read(100,2),'SCAN_ABORTED');reader=SealReader(b.l,'g0',n);steps=0
 while True:
  result=reader.read(7,2);steps+=1
  if result['status']=='COMPLETE':break
  assert steps<1000
 equal(no_authoritative(b.l.s),before);full=SealReader(b.l,'g0',n).read(v.M,v.M);equal(result['disposition_root'],full['disposition_root']);equal(result['receipt_root'],full['receipt_root']);b.add('SEALED',dict(round=n,gateway='g0'));effect=b.l.s['segments'][-1]['result']['effects'][0]['body'];equal(result['disposition_root'],effect['disposition_root']);equal(result['receipt_root'],effect['receipt_root']);b.add('DRAIN',dict(round=n,gateway='g0',proof=b.proof('SEAL',n)));b.add('READY',dict(round=n));b.add('CLOSE',dict(round=n,closed_at=NOW));b.install(n,'COMMITTED');funded_replay(b,'seal-scan');return dict(first=first,final=full,pages_calls=str(steps))


def read_integrity():
 from reads import Reader,expected,reconstruct_stored
 b=Builder(families=1,gateways=1);case,tid=b.intake(0,'integrity');b.reconcile(tid);b.close([0]);pins=[expected(b.l,h) for h in b.l.s['journals']];reconstruct_stored(b.trace(),b.l.s['segments'],pins)
 for field in ['result','effects','dependencies']:
  segments=copy.deepcopy(b.l.s['segments']);segment=segments[-1]
  if field=='result':segment['result']['root']='f'*64
  elif field=='effects':segment['result']['effects']=copy.deepcopy(next(z['result']['effects'] for z in segments if z['command']['kind']=='RECEIVE'))
  else:segment['dependencies']=[]
  reject(lambda:reconstruct_stored(b.trace(),segments,pins),'STORED_SEMANTIC_MISMATCH')
 reject(lambda:reconstruct_stored(b.trace(),b.l.s['segments'][:-1],pins),'STORED_SUFFIX_MISSING')
 e=expected(b.l);reader=Reader(b.l);request=dict(expected=e,budget={'bytes':'1','pages':'1','segments':'1'});first=reader.read(request);old=copy.deepcopy(first['cursor']);request['cursor']=old;second=reader.read(request);equal(len(reader.sessions),1);reject(lambda:reader.read(dict(request,cursor=old)),'UNVERIFIED_CURSOR');reader.cancel();equal(len(reader.sessions),0);reject(lambda:reader.read(dict(request,cursor=second['cursor'])),'UNVERIFIED_CURSOR')
 row=b.l.read_index['center'][0];row['bytes']=row['bytes'][:-1]+b' ';reject(lambda:Reader(b.l).read(dict(expected=e,budget={'bytes':str(v.M),'pages':str(v.M),'segments':str(v.M)})),'SEGMENT_HASH')


def terminal_races():
 for winner in ['CLOSE','ABORT']:
  b=Builder(families=1,gateways=1);n=b.begin([0],'CANCELLABLE');b.seal(n);b.add(winner,dict(round=n,**({'closed_at':NOW} if winner=='CLOSE' else {})));loser='ABORT' if winner=='CLOSE' else 'CLOSE';before=copy.deepcopy(no_authoritative(b.l.s));b.add(loser,dict(round=n,**({'closed_at':NOW} if loser=='CLOSE' else {})),expect='REFUSED');equal(no_authoritative(b.l.s),before);b.install(n,'COMMITTED' if winner=='CLOSE' else 'ABORTED');funded_replay(b,'terminal-race-'+winner.lower())


def subset_and_delayed():
 b=Builder(families=3,gateways=2)
 for family,gateway,mode in [(0,'g0','FINISH_ONLY'),(1,'g1','CANCELLABLE'),(2,'g0','FINISH_ONLY')]:
  n=b.begin([family],mode,[gateway]);b.seal(n);b.add('CLOSE',dict(round=n,closed_at=NOW));b.install(n,'COMMITTED')
 equal(b.l.s['gateways']['g0']['acknowledged'],3);equal(b.l.s['gateways']['g1']['acknowledged'],2);equal([c['cutoffs'][0]['gateway'] for c in b.l.s['certificates']],['g0','g1','g0']);funded_replay(b,'alternating-subset-rounds')
 b=Builder(families=1,gateways=1);n=b.begin([0],'CANCELLABLE');b.add('ABORT',dict(round=n));b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='0',proof=b.proof('BEGIN',n)));b.install(n,'ABORTED');b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='0',proof=b.proof('BEGIN',n)),expect='REFUSED');b.close([0]);funded_replay(b,'unseen-abort-delayed-begin')

def preparation_order():
 b=Builder(families=1,gateways=4,preparation_order=['g3','g1','g0','g2']);found={}
 for i in range(100):
  case=[b.p['families'][0]['key'],'source','permuted'+str(i)];found.setdefault(b.owner(case),case)
 for g,case in found.items():tid=b.issue(g);b.receive(tid,case);b.import_token(tid);b.reconcile(tid)
 equal(set(found),{'g0','g1','g2','g3'});b.close([0]);funded_replay(b,'permuted-preparation-order')

def comparison_resume():
 from reads import ComparisonReader,Reader,expected
 b=customer_story();request=dict(expected=expected(b.l),policy={'resolution_atoms':'1500'},coverage=[dict(gateway=g,status='UNKNOWN_GATEWAY_COVERAGE') for g in sorted(b.l.s['gateways'])],budget={'bytes':'4096','pages':'1','segments':'1'});reader=ComparisonReader(b.l);calls=0;before=copy.deepcopy(no_authoritative(b.l.s))
 while True:
  r=reader.read(request);calls+=1
  if r['status']!='INCOMPLETE':break
  assert 'alternative' not in r;request['cursor']=r['cursor'];assert calls<1000
 equal(r['alternative'],'11750');equal(no_authoritative(b.l.s),before)
 for field in ['target','enrollment','profile']:
  pin=expected(b.l);pin[field]='f'*64 if field=='enrollment' else 'wrong';reject(lambda:Reader(b.l).start(pin))


def supplemental_control():
 b=Builder(families=1,gateways=1);case,tid=b.intake(0,'supplement');body=base64.b64encode(b'additional evidence').decode();b.add('SUPPLEMENT',dict(case=case,evidence=[dict(body=body,sha256=hashlib.sha256(b'additional evidence').hexdigest())]));b.decide(case,0,'DENY');b.reconcile(tid);b.close([0]);funded_replay(b,'supplement-control')


def saved_close_unimported():
 b=Builder(families=2,gateways=1,suppliers=[dict(id='pool',maximum='500',consumed='330',held='170',released='0')]);p=copy.deepcopy(b.p)
 for f in p['families']:f.update(book='SUPPLIER',supplier_pool='pool')
 b=reenroll(b,p);b.close([0]);old=copy.deepcopy(next(c for c in b.commands if c['kind']=='CLOSE'));case=[b.p['families'][1]['key'],'source','unimported'];tid=b.issue('g0');b.receive(tid,case);before=copy.deepcopy(no_authoritative(b.l.s));retry(b,old);equal(no_authoritative(b.l.s),before);equal(b.l.s['suppliers']['pool']['held'],'170');n=b.begin([1]);b.add('SEAL_BEGIN',dict(round=n,gateway='g0',predecessor='1',proof=b.proof('BEGIN',n)));b.add('SEALED',dict(round=n,gateway='g0'));b.add('DRAIN',dict(round=n,gateway='g0',proof=b.proof('SEAL',n)),expect='REFUSED');b.import_token(tid);b.reconcile(tid);b.advance();b.add('DRAIN',dict(round=n,gateway='g0',proof=b.proof('SEAL',n)));b.add('READY',dict(round=n));b.add('CLOSE',dict(round=n,closed_at=NOW));b.install(n,'COMMITTED');equal(b.l.s['suppliers']['pool']['released'],'170');funded_replay(b,'REG02-saved-close-unimported')


def main():
 test('REG02-saved-close-with-unimported-receipt',saved_close_unimported);test('supplement-control',supplemental_control);test('subset-rounds-and-unseen-abort',subset_and_delayed);test('routing-independent-of-preparation-arrival',preparation_order);test('comparison-resume-and-prefix-binding',comparison_resume);test('terminal-close-abort-winners',terminal_races);test('stored-semantic-and-bounded-reader-integrity',read_integrity);test('seal-scan-yield-abort-resume',seal_scan);test('provenance-and-genesis-attacks',provenance_attacks);test('delayed-seal-observation',lambda:funded_replay(delayed_seal(),'delayed-seal-observation'));test('independent-host-identity',lambda:funded_replay(host_identity(),'independent-host-identity'));test('strict-encoding-evidence-decimal-boundaries',encoding);test('customer-S00-S14-and-exact-retries',customer);test('base-retained-mutation',base_mutations);test('routing16-and-namespace-boundaries',namespace_routes);test('read-cursors-unknown-comparison',reads_and_comparison);test('unclaimed-retirement-and-no-revival',retirement);test('late-refusal-exception-full-rollback',rollback);test('C1-uncovered-issuance',uncovered_issuance);test('optional-round-exhaustion',repeated_cancel);test('writer-epoch-guards',writer_epoch);test('measured-envelopes-and-maximal-key-paths',measured_envelopes);test('expected-prefix-truncation',truncation);test('max-topology32-4-8-3',topology);test('independent-gross249-250-251',gross_boundaries);test('directional-caps-and-shared-funding-races',directional_funding);test('genuine-clock-lower-bound',clock_floor)
 for d in v.DIMS:
  for n in [-1,0,1]:test('resource-'+d+'-'+str(n),lambda d=d,n=n:resource_boundary(d,n))
 for name in v.SCHEMA['x-counters']:
  for n in [-1,0,1]:test('counter-'+name+'-'+str(n),lambda name=name,n=n:counter_boundary(name,n))
 for stage in ['before_begin','partial','sealed','ready']:test('abort-'+stage,lambda stage=stage:funded_replay(cancellation(stage),'abort-'+stage))
 test('actual-high-water-above-allocation-cutoff',lambda:funded_replay(highwater(),'high-water'))
 test('mixed257-original-allocation2-before-alias1',lambda:funded_replay(mixed(257),'mixed257'));test('all-undelivered257',lambda:funded_replay(mixed(257,True),'all-unused257'))
 for n in [255,256,257,1025]:test('pending-'+str(n),lambda n=n:funded_replay(pending(n),'pending'+str(n)))
 test('REG06-last-release170',lambda:funded_replay(supplier(True),'REG06-release170'));test('REG09-partial-held200',lambda:funded_replay(supplier(False),'REG09-held200'));test('supplier-batch170',lambda:supplier(True,True));test('supplier-zero-held-transition',lambda:supplier(True,False,True))
 print(json.dumps(dict(status='author-canonical-checks',passed=len(RESULTS),failed=0,ignored=0,tests=RESULTS,actual_store_proofs='PENDING'),indent=2))
if __name__=='__main__':main()
