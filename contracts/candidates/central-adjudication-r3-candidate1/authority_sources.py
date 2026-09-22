"""Exact synthetic source retention within the existing trusted-host boundary."""
import base64,copy,hashlib
class SourceSet:
 def __init__(self,initial,v):
  self.v=v;self.records={};self.identities={};rows=initial['authority_sources'];v.require(type(rows)is list and rows==v.sorted_set(rows),'AUTH_SOURCE_SET')
  for row in rows:
   v.validate_shape('authority_source',row);raw=base64.b64decode(row['body'],validate=True);body=v.strict(raw);v.validate_shape('authority_source_body',body);v.require(raw==v.canonical(body) and len(raw)==int(row['bytes']) and hashlib.sha256(raw).hexdigest()==row['body_hash'],'AUTH_SOURCE_BYTES');identity=v.key([body['source'],body['id'],body['revision']]);v.require(identity not in self.identities,'AUTH_SOURCE_IDENTITY');v.require(row['body_hash'] not in self.records,'AUTH_SOURCE_DUPLICATE');self.identities[identity]=row['body_hash'];self.records[row['body_hash']]=(copy.deepcopy(row),body)
  v.require(set(self.records)==set(initial['authority_documents']),'AUTH_SOURCE_MEMBERSHIP')
 def get(self,ledger,reference,kind,scope,target=None):
  v=self.v;v.require(reference in self.records and reference in ledger.initial['authority_documents'],'AUTH_SOURCE_MISSING');row,body=self.records[reference];v.require(body['kind']==kind,'AUTH_SOURCE_KIND');v.require(body['scope']==scope and (target is None or body['target']==target),'AUTH_SOURCE_SCOPE');ledger._authority_used[reference]=(row,body);return body
 def command(self,ledger,c):
  v=self.v;a=c['authority'];p=c['payload'];enrollment=ledger.s['enrollment'];target=p['target'] if c['kind']=='ENROLL' else enrollment['target'] if enrollment else None
  body=self.get(ledger,a['document'],'AUTHORIZATION',c['key'][0],target);v.require(body['revision']==a['revision'] and body['principal']==a['principal'] and a['permission'] in body['permissions'],'AUTH_SOURCE_BINDING');v.require(body['starts_at']<=a['observed_at']<body['ends_at'],'AUTH_SOURCE_WINDOW')
 def delegation(self,ledger,roles,context,agreements,exposure,now,finish=None):
  v=self.v
  if roles['bearer']==roles['payer'] and 'payer_delegation' not in roles:return
  v.require('payer_delegation' in roles,'PAYER_DELEGATION');body=self.get(ledger,roles['payer_delegation'],'DELEGATION',context['scope'],context['target']);plain={k:x for k,x in roles.items() if k!='payer_delegation'}
  v.require(body['roles']==plain and set(agreements)<=set(body['agreement_ids']) and int(body['maximum_exposure'])>=exposure,'DELEGATION_BINDING');v.require(body['acceptor']==roles['payer'] and body['starts_at']<=now<body['ends_at'] and (finish is None or finish<body['ends_at']),'DELEGATION_WINDOW');v.require(body['assent']['accepted_at']<=now and body['assent']['terms']==v.digest('authority',{k:x for k,x in body.items() if k!='assent'}),'DELEGATION_ASSENT')
 def assent(self,ledger,reference,roles,terms,context):
  v=self.v;body=self.get(ledger,reference,'ASSENT',context['scope'],context['target']);v.require(body['roles']==roles and body['terms']==v.digest('authority',terms),'ASSENT_BINDING')
 def economics(self,ledger,c):
  v=self.v;p=c['payload'];now=c['authority']['observed_at'];kind=c['kind'];enrollment=p if kind=='ENROLL' else ledger.s['enrollment']
  if kind=='ENROLL':
   exposure={}
   for f in p['families']:
    self.assent(ledger,f['assent'],f['roles'],{k:x for k,x in f.items() if k!='assent'},p);bound=max(abs(int(x)) for x in [f['ordinary_atoms']]+f['correction_atoms']);self.delegation(ledger,f['roles'],p,[f['key'][1]],bound,now,f['correction_by'])
    if 'payer_delegation' in f['roles']:ref=f['roles']['payer_delegation'];exposure[ref]=exposure.get(ref,0)+bound
   for pool in p['pools']:
    for au in pool['authorizations']:
     terms={k:x for k,x in pool.items() if k!='authorizations'}|au;terms.pop('assent');self.assent(ledger,au['assent'],au['roles'],terms,p);bound=min(int(pool[x]) for x in ['funding','gross','positive' if au['direction']=='POSITIVE' else 'negative']) if au['direction']!='ZERO' else 0;self.delegation(ledger,au['roles'],p,[f['key'][1] for f in p['families']],bound,now)
     if 'payer_delegation' in au['roles']:ref=au['roles']['payer_delegation'];exposure[ref]=exposure.get(ref,0)+bound
   for ref,bound in exposure.items():v.require(int(self.records[ref][1]['maximum_exposure'])>=bound,'DELEGATION_EXPOSURE')
  elif kind in {'DECIDE','CORRECT'}:
   f=ledger.s['families'].get(v.key(p['case'][0]));
   if f is None:return
   f=f['terms']
   if kind=='DECIDE' and p['path']=='ADJUSTMENT':
    pool=ledger.s['pools'].get(p['pool'])
    if pool is None:return
    pool=pool['terms'];a=int(p['signed_atoms']);direction='POSITIVE' if a>0 else 'NEGATIVE' if a<0 else 'ZERO';terms={k:x for k,x in pool.items() if k!='authorizations'}|dict(direction=direction,roles=p['roles']);self.assent(ledger,p['assent'],p['roles'],terms,enrollment);exposure=abs(a)
   else:self.assent(ledger,p['assent'],p['roles'],{k:x for k,x in f.items() if k!='assent'},enrollment);exposure=abs(int(p['replacement'] if kind=='CORRECT' else p['signed_atoms']))
   self.delegation(ledger,p['roles'],enrollment,[f['key'][1]],exposure,now)
 def inventory(self,ledger,c,host,ordinal):
  v=self.v;used=ledger._authority_used;limit=83 if c['kind']=='ENROLL' else 3 if c['kind'] in {'DECIDE','CORRECT'} else 1;v.require(len(used)<=limit,'AUTH_SOURCE_COUNT');v.require(sum(int(row['bytes']) for row,_ in used.values())<=(524288 if c['kind']=='ENROLL' else limit*16384),'AUTH_SOURCE_BUDGET');known=ledger.s['authority_retained'].get(host,{});objects=[];dependencies=[];pending={};p=c['payload'];binding=p if c['kind']=='PREPARE_ENROLL' else ledger.s['enrollment']
  for reference,(row,body) in used.items():
   identity=v.key([body['source'],body['id'],body['revision']])
   if identity in known:
    old=known[identity];v.require(old['hash']==reference and old['bytes']==row['bytes'],'AUTH_RETAINED_CONFLICT');dependencies.append(old['segment']);continue
   objects.append(dict(origin=dict(store=binding['store'],scope=binding['scope'],registration=binding['registration'],host=host,ordinal=ordinal),kind='AUTHORITY',full_key=[body['source'],body['id'],body['revision']],**copy.deepcopy(row)));pending[identity]=dict(hash=reference,bytes=row['bytes'])
  return objects,dependencies,pending
