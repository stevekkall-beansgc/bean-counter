"""Synthetic host-approved exact sources for canonical schedules, never real assent."""
import base64,copy,hashlib
import validate as v
START='2000-01-01T00:00:00.000000Z';END='2100-01-01T00:00:00.000000Z'
def prepare(builder):
 p=builder.p;p['pools']=[copy.deepcopy(x) for x in p['pools']];rows=[]
 def add(body):
  raw=v.canonical(body);h=hashlib.sha256(raw).hexdigest();rows.append(dict(body=base64.b64encode(raw).decode(),body_hash=h,bytes=str(len(raw))));return h
 def common(kind,id):return dict(kind=kind,source='synthetic-authority',id=id,revision='1',scope=p['scope'],target=p['target'])
 builder.authorization=add(dict(common('AUTHORIZATION','command-authority'),principal='host',permissions=sorted(['enroll','capacity','submit','read','decide','adjust','correct','close','replace']),starts_at=START,ends_at=END))
 def roles(value,id):
  value=copy.deepcopy(value)
  if value['payer']!=value['bearer'] or 'payer_delegation' in value:
   value.pop('payer_delegation',None);body=dict(common('DELEGATION','delegation-'+id),roles=value,agreement_ids=v.sorted_set(list({f['key'][1] for f in p['families']})),maximum_exposure=str(v.M),starts_at=START,ends_at=END,acceptor=value['payer']);body['assent']=dict(accepted_at=START,terms=v.digest('authority',body));value=dict(value,payer_delegation=add(body))
  return value
 for i,f in enumerate(p['families']):
  f['roles']=roles(f['roles'],'family-'+str(i));terms={k:x for k,x in f.items() if k!='assent'};f['assent']=add(dict(common('ASSENT','family-'+str(i)),roles=f['roles'],terms=v.digest('authority',terms)))
 for pool in p['pools']:
  for au in pool['authorizations']:
   au['roles']=roles(au['roles'],'pool-'+pool['id']+'-'+au['direction']);terms={k:x for k,x in pool.items() if k!='authorizations'}|{k:x for k,x in au.items() if k!='assent'};au['assent']=add(dict(common('ASSENT','pool-'+pool['id']+'-'+au['direction']),roles=au['roles'],terms=v.digest('authority',terms)))
  pool['authorizations']=v.sorted_set(pool['authorizations'])
 p['pools']=v.sorted_set(p['pools'])
 builder.initial.update(authority_sources=v.sorted_set(rows),authority_documents=v.sorted_set([r['body_hash'] for r in rows]),authority_observations=[],trusted_observations=[],grant_authentications=[])
