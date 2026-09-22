#!/usr/bin/env python3
"""R3 canonical protocol reference with read-only original-profile audit. No store proof."""
import base64,copy,datetime,hashlib,json,re,sys
from pathlib import Path
HERE=Path(__file__).resolve().parent
SCHEMA=json.loads((HERE/'protocol/schema.json').read_text())
WORK=json.loads((HERE/'protocol/resources.json').read_text())
M=10**30-1; ZERO='0'*64
LOCAL={'PREPARE_ENROLL','PREPARE_ROUND','LOCAL_GRANT','ACTIVATE','RECEIVE','RETURN_UNUSED','LOCAL_TERMINAL','SEAL_BEGIN','SEALED','INSTALL','REPLACE_WRITER'}
DIMS=('canonical_bytes','trusted_bytes','records','index_pages','index_values','workspace_bytes')
class Invalid(ValueError):pass
class Refused(ValueError):pass
def require(v,code):
 if not v:raise Invalid(code)
def guard(v,code):
 if not v:raise Refused(code)
def pairs(p):
 d={}
 for k,v in p:
  require(k not in d,'DUPLICATE_KEY');d[k]=v
 return d
def integer(s):
 require(s!='-0','NEGATIVE_ZERO');n=int(s);require(abs(n)<=9007199254740991,'UNSAFE_INTEGER');return n
def strict(raw):
 if isinstance(raw,bytes):raw=raw.decode('utf8','strict')
 require(not raw.startswith('\ufeff'),'BOM')
 def bad(s):raise Invalid('NUMBER_TOKEN')
 x=json.loads(raw,object_pairs_hook=pairs,parse_int=integer,parse_float=bad,parse_constant=bad)
 def walk(v,depth=0):
  require(depth<=32,'DEPTH')
  if isinstance(v,str):v.encode('utf8','strict')
  elif isinstance(v,dict):
   for k,e in v.items():walk(k,depth+1);walk(e,depth+1)
  elif isinstance(v,list):
   for e in v:walk(e,depth+1)
 walk(x);return x
def canonical(v):
 if v is None:return b'null'
 if v is True:return b'true'
 if v is False:return b'false'
 if isinstance(v,int):require(abs(v)<=9007199254740991,'UNSAFE_INTEGER');return str(v).encode()
 if isinstance(v,str):return json.dumps(v,ensure_ascii=False,separators=(',',':')).encode('utf8')
 if isinstance(v,list):return b'['+b','.join(map(canonical,v))+b']'
 require(type(v) is dict,'JSON_TYPE')
 return b'{'+b','.join(canonical(k)+b':'+canonical(v[k]) for k in sorted(v,key=lambda k:k.encode('utf-16-be')))+b'}'
def digest(domain,v):
 require(domain in {'command','submission','grant','claim','receipt','action','closure','segment','replay','route','namespace','authority','evidence','result','enrollment'},'HASH_DOMAIN')
 return hashlib.sha256(('ledgerlab/central-r3/'+domain+'/1').encode()+b'\0'+canonical(v)).hexdigest()
def shape(s,v):
 if '$ref'in s:return shape(SCHEMA['$defs'][s['$ref'].split('/')[-1]],v)
 if 'const'in s:require(v==s['const'] and type(v)==type(s['const']),'CONST');return
 if 'enum'in s:require(v in s['enum'],'ENUM');return
 if 'oneOf'in s:
  count=0
  for q in s['oneOf']:
   try:shape(q,v);count+=1
   except (Invalid,ValueError,TypeError):pass
  require(count==1,'VARIANT');return
 t=s['type']
 if t=='object':
  require(type(v)is dict,'OBJECT');require(set(s['required'])<=set(v)<=set(s['properties']),'FIELDS')
  for k,a in v.items():shape(s['properties'][k],a)
 elif t=='array':
  require(type(v)is list and s['minItems']<=len(v)<=s['maxItems'],'ARRAY_BOUND')
  if s.get('uniqueItems'):
   require(len({canonical(a) for a in v})==len(v),'DUPLICATE_ITEM')
   if not s.get('x-ordered'):require(v==sorted(v,key=canonical),'UNSORTED_SET')
  for i,a in enumerate(v):shape(s['prefixItems'][i] if 'prefixItems'in s else s['items'],a)
 elif t=='integer':require(type(v)is int and s['minimum']<=v<=s['maximum'],'INTEGER')
 elif t=='boolean':require(type(v)is bool,'BOOLEAN')
 elif t=='string':
  require(type(v)is str,'STRING');b=v.encode('utf8');require(len(v)>=s.get('minLength',0) and len(v)<=s.get('maxLength',10**9),'TEXT_BOUND');require(len(b)<=s.get('x-max-utf8',10**9),'UTF8_BOUND')
  if 'pattern'in s:require(re.fullmatch(s['pattern'],v) is not None,'PATTERN')
  if s.get('format')=='date-time':datetime.datetime.strptime(v,'%Y-%m-%dT%H:%M:%S.%fZ')
  if 'x-base64-bytes'in s:
   a=base64.b64decode(v,validate=True);require(base64.b64encode(a).decode()==v and len(a)<=s['x-base64-bytes'],'BASE64')
 else:raise Invalid('SCHEMA_TYPE')
def validate_shape(name,value):
 shape(SCHEMA['$defs'][name],value)
 if name=='retry_response':
  if value['knowledge']=='UNKNOWN':require('prefix' not in value and value['current_lifecycle']=='UNKNOWN' and value['central_admission']=='UNKNOWN','KNOWLEDGE_UNKNOWN')
  elif value['knowledge']=='CACHED_VERIFIED_PREFIX':require('prefix' in value and value['current_lifecycle']=='UNKNOWN' and value['central_admission']=='UNKNOWN','KNOWLEDGE_CACHED')
  else:require('prefix' in value,'KNOWLEDGE_PREFIX')
def key(v):return canonical(v).decode()
def sorted_set(v):return sorted(v,key=canonical)
def command_hash(c):return digest('command',[c['kind'],c['key'],c['payload']['submission'] if c['kind']=='RECEIVE' else c['payload']])
def vec(kind):
 r=WORK['transitions'][kind];return dict(zip(DIMS,[r['segment_bytes'],r['new_trusted_bytes'],r['records'],r['index_path_pages'],r['index_value_pages'],r['logical_workspace_bytes']]))
class Ledger:
 def __init__(self,initial):
  self._charges=[];self.peaks={};self.read_index={};self.initial=copy.deepcopy(initial);self.validate_initial(initial);self.s={'enrollment':None,'preparations':{},'round_preparations':{},'object_inventory':{},'grants':{},'tokens':{},'gateways':{},'cases':{},'deliveries':{},'controls':{},'entitlements':{},'suppliers':{},'pools':{},'families':{},'rounds':{},'active':None,'last_round':0,'customer':0,'gross':0,'positive':0,'negative':0,'actions':[],'certificates':[],'journals':{},'segments':[],'duplicates':0,'refused':0,'resources':{},'counters':{},'allocations':{}}
  for host,provisioned in initial.get('initial_resources',{}).items():
   shape(SCHEMA['$defs']['resource'],provisioned)
   self.s['resources'][host]={'provisioned':{k:int(provisioned[k]) for k in DIMS},'used':dict.fromkeys(DIMS,0),'held':dict.fromkeys(DIMS,0)}
   self.s['counters'][host]={k:{'q':int(initial.get('initial_counters',{}).get(host,{}).get(k,{}).get('q',0)),'R':int(initial.get('initial_counters',{}).get(host,{}).get(k,{}).get('R',0))} for k in SCHEMA['x-counters']}
 def validate_initial(self,x):
  required={'authority_documents','authority_observations','grant_authentications','trusted_observations','original_objects','original_base_receipt','original_base_manifest','initial_resources','initial_counters','writer_capabilities'}
  require(type(x)is dict and set(x)==required,'INITIAL_FIELDS')
  for name in ['authority_documents','authority_observations','grant_authentications','trusted_observations']:
   require(type(x[name])is list and len(x[name])==len(set(x[name])) and x[name]==sorted_set(x[name]),'INITIAL_SET')
   for value in x[name]:validate_shape('digest',value)
  for name in ['original_base_receipt','original_base_manifest']:validate_shape('digest',x[name])
  require(type(x['original_objects'])is list and 1<=len(x['original_objects'])<=128,'ORIGINAL_OBJECT_COUNT')
  require(type(x['initial_resources'])is dict and 2<=len(x['initial_resources'])<=5,'RESOURCE_HOSTS')
  for host,r in x['initial_resources'].items():validate_shape('id',host);validate_shape('resource',r)
  require(type(x['writer_capabilities'])is dict and 1<=len(x['writer_capabilities'])<=4,'WRITER_COUNT')
  for host,c in x['writer_capabilities'].items():
   validate_shape('id',host);require(type(c)is dict and set(c)=={'epoch','journal_head','fence'},'WRITER_FIELDS');validate_shape('count',c['epoch']);validate_shape('digest',c['fence']);require(c['journal_head']==ZERO and c['epoch']=='1' and host in x['initial_resources'],'WRITER_ANCHOR')
  require(type(x['initial_counters'])is dict and set(x['initial_counters'])<=set(x['initial_resources']),'COUNTER_HOST')
  for host,counters in x['initial_counters'].items():
   require(type(counters)is dict and set(counters)<=set(SCHEMA['x-counters']),'COUNTER_FIELDS')
   for name,c in counters.items():
    require(type(c)is dict and set(c)=={'q','R'},'COUNTER_PAIR');validate_shape('count',c['q']);validate_shape('count',c['R']);require(int(c['R'])==0,'INITIAL_UNOWNED_RESERVE');require(int(c['q'])==(int(x['writer_capabilities'][host]['epoch']) if name=='writer_epoch' and host in x['writer_capabilities'] else 0),'INITIAL_COUNTER_ANCHOR')
  for host,cap in x['writer_capabilities'].items():require(x['initial_counters'].get(host,{}).get('writer_epoch')=={'q':cap['epoch'],'R':'0'},'INITIAL_WRITER_COUNTER')
 def host(self,c):
  p=c['payload']
  return p['host'] if c['kind']=='EXTEND_RESOURCES' else p['grant']['gateway'] if c['kind']=='LOCAL_GRANT' else p['gateway'] if c['kind']in LOCAL else p['store'] if c['kind']=='ENROLL' else self.s['enrollment']['store']
 def journal(self,h):return self.s['journals'].setdefault(h,{'ordinal':'0','segment':ZERO,'root':ZERO})
 def reserve(self,h,owner,kinds):
  require(owner not in self.s['allocations'],'ALLOCATION_ID')
  a=self.s['resources'].get(h);guard(a is not None,'RESOURCE_HOST')
  costs={d:(max((vec(k)[d] for k in kinds),default=0) if d=='workspace_bytes' else sum(vec(k)[d] for k in kinds)) for d in DIMS}
  for d,n in costs.items():guard(a['used'][d]+a['held'][d]+n<=a['provisioned'][d],'RESOURCE_'+d)
  credits={d:sum(WORK['transitions'][k]['counter_increments'][d] for k in kinds) for d in SCHEMA['x-counters']}
  for d,n in credits.items():c=self.s['counters'][h][d];guard(c['q']+c['R']+n<=M,'COUNTER_'+d)
  for d,n in costs.items():a['held'][d]+=n
  for d,n in credits.items():self.s['counters'][h][d]['R']+=n
  self.peaks[h]={d:max(self.peaks.get(h,{}).get(d,0),a['used'][d]+a['held'][d]) for d in DIMS}
  self.s['allocations'][owner]={'host':h,'slots':list(kinds),'held':costs}
 def spend(self,owner,kind,actual=None):
  guard(owner in self.s['allocations'],'UNFUNDED');o=copy.deepcopy(self.s['allocations'][owner]);self.s['allocations'][owner]=o;guard(kind in o['slots'],'SLOT_SPENT');h=o['host'];a=self.s['resources'][h]
  for d,n in vec(kind).items():
   if d=='workspace_bytes':guard(n<=o['held'][d],'WORKSPACE');continue
   guard(o['held'][d]>=n,'ENVELOPE');o['held'][d]-=n;a['held'][d]-=n;a['used'][d]+=n
  for d,n in WORK['transitions'][kind]['counter_increments'].items():
   c=self.s['counters'][h][d];guard(c['R']>=n and c['q']+n<=M,'COUNTER_'+d);c['R']-=n;c['q']+=(actual.get(d,n) if actual else n)
  o['slots'].remove(kind)
  self._charges.append((h,kind))
 def terminal_slack(self,owner):
  o=copy.deepcopy(self.s['allocations'][owner]);self.s['allocations'][owner]=o;a=self.s['resources'][o['host']]
  for d,n in o['held'].items():a['held'][d]-=n;o['held'][d]=0
  for k in o['slots']:
   for d,n in WORK['transitions'][k]['counter_increments'].items():self.s['counters'][o['host']][d]['R']-=n
  o['slots']=[]
 def optional(self,c,host):
  owner='command:'+key([host,c['key']]);self.reserve(host,owner,[c['kind']]);self.spend(owner,c['kind'],{'economic_revision':0} if c['kind']=='DECIDE' and c['payload']['verdict']=='DENY' else None);self.terminal_slack(owner)
 def role(self,c,required):
  a=c['authority'];require(a['command']==command_hash(c),'AUTH_COMMAND');require(a['document']in self.initial['authority_documents'],'AUTH_DOCUMENT');require(digest('authority',a) in self.initial['authority_observations'],'AUTH_OBSERVATION');guard(a['permission']==required,'AUTH_PERMISSION')
 def proof(self,p,kinds,fullkeys,binding=None):
  validate_shape('proof',p)
  binding=binding or self.s['enrollment'];require(p['store']==binding['store'] and p['scope']==binding['scope'] and p['registration']==binding['registration'],'PROOF_BINDING')
  require(p['fact_kind'] in kinds and p['full_key'] in fullkeys,'PROOF_KIND_KEY')
  require(p['trusted_observation_ref']==digest('authority',{a:b for a,b in p.items() if a!='trusted_observation_ref'}) and p['trusted_observation_ref'] in self.initial.get('trusted_observations',[]),'PROOF_AUTH')
  source=[x for x in self.s['segments'] if x['host']==p['host'] and x['ordinal']==p['ordinal']]
  require(len(source)==1,'PROOF_UNRESOLVED');source=source[0]
  require(digest('segment',source)==p['segment'] and source['result']['root']==p['root'],'PROOF_PREFIX')
  matches=[o for o in source['objects'] if o['kind']==p['fact_kind'] and o['full_key']==p['full_key'] and o['body_hash']==p['body_hash'] and o['bytes']==p['bytes'] and o['origin']=={f:p[f] for f in ['store','scope','registration','host','ordinal']}]
  require(len(matches)==1,'PROOF_MEMBERSHIP');return copy.deepcopy(matches[0])
 def execute(self,c):
  validate_shape('command',c)
  if c['kind']=='RECEIVE':require(c['key']==c['payload']['delivery'],'RECEIVE_KEY')
  require(len(canonical(c))<=262144,'OPERATION_BYTES')
  self._charges=[]
  before={a:(list(b) if a=='segments' else dict(b) if a in {'controls','allocations','grants','tokens','cases','families','object_inventory'} else copy.deepcopy(b)) for a,b in self.s.items()};effects=[];objects=[];h=self.host(c);j=self.journal(h);k=c['kind'];p=c['payload'];ck=key([h,c['key']]);ch=command_hash(c)
  token_ids=set()
  if isinstance(p.get('token'),str):token_ids.add(p['token'])
  if k in {'ADVANCE','ADVANCE_RECEIPT'}:
   gw=self.s['gateways'][p['gateway']]
   if k=='ADVANCE_RECEIPT' and int(p['through']) in gw['receipts']:token_ids.add(gw['receipts'][int(p['through'])])
   elif k=='ADVANCE':token_ids.update(tid for tid,t in self.s['tokens'].items() if t['body']['gateway']==p['gateway'] and t['body']['allocation']==p['through'])
  grant_ids={p['grant']} if isinstance(p.get('grant'),str) else {p['grant']['id']} if isinstance(p.get('grant'),dict) else set()
  case_ids={key(p['case'])} if 'case' in p else {key(p['submission']['case'])} if 'submission' in p else set()
  for tid in token_ids:
   if tid in self.s['tokens']:
    t=self.s['tokens'][tid];grant_ids.add(t['body']['grant'])
    if t['case']:case_ids.add(t['case'])
    self.s['tokens'][tid]=copy.deepcopy(t)
  for gid in grant_ids:
   if gid in self.s['grants']:self.s['grants'][gid]=copy.deepcopy(self.s['grants'][gid])
  for cid in case_ids:
   if cid in self.s['cases']:self.s['cases'][cid]=copy.deepcopy(self.s['cases'][cid])
  if k=='CLOSE':self.s['cases']=copy.deepcopy(self.s['cases']);self.s['families']=copy.deepcopy(self.s['families'])
  try:
   if k=='RECEIVE':
    ns=self.s['gateways'][h]['namespace'];prefix='gw1.'+ns['tag']+'.';guard(p['delivery'][0]==ns['scope'] and p['delivery'][2].startswith(prefix) and 1<=len(p['delivery'][2][len(prefix):].encode())<=91,'NAMESPACE')
   if ck in self.s['controls']:
    old=self.s['controls'][ck];self.role(c,'read');guard(old['digest']==ch,'IDENTITY_CONFLICT');self.s['duplicates']+=1;return {'status':'DUPLICATE','code':'EXACT_RETRY','effects':copy.deepcopy(old['effects']),'root':j['root']}
   self.role(c,{'ENROLL':'enroll','RECEIVE':'submit','SUPPLEMENT':'submit','DECIDE':('adjust' if p.get('path')=='ADJUSTMENT' else 'decide'),'CORRECT':'correct','BEGIN':'close','CLOSE':'close','ABORT':'close','REPLACE_WRITER':'replace'}.get(k,'capacity'))
   require(c['authority']['head']==j['root'],'AUTH_HEAD')
   if k not in {'ENROLL','PREPARE_ENROLL'}:guard(self.s['enrollment']is not None,'NOT_ENROLLED');guard(c['key'][0]==self.s['enrollment']['scope'],'SCOPE')
   if k in LOCAL and k not in {'PREPARE_ENROLL','LOCAL_GRANT','REPLACE_WRITER'}:
    gw=self.s['gateways'][h];cap=self.initial['writer_capabilities'].get(h);guard(cap is not None,'WRITER_CAPABILITY')
   if k in {'PREPARE_ROUND','LOCAL_GRANT','REGISTER_GRANT','ACTIVATE','RETURN_UNUSED','IMPORT','RECONCILE','LOCAL_TERMINAL','SEAL_BEGIN','DRAIN','INSTALL','ACK_INSTALL'}:
    expected={'PREPARE_ROUND':({'ENROLLMENT'},[self.s['enrollment']['registration']]),'LOCAL_GRANT':({'ENROLLMENT'},[self.s['enrollment']['registration']]),'REGISTER_GRANT':({'GRANT'},[p.get('grant',{}).get('id') if isinstance(p.get('grant'),dict) else None]),'ACTIVATE':({'CLAIM'},[p.get('token')]),'RETURN_UNUSED':({'CLAIM'},[p.get('token')]),'IMPORT':({'RECEIPT','ALIAS'},[p.get('token')]),'RECONCILE':({'RECEIPT','ALIAS','RETURNED_UNUSED'},[p.get('token')]),'LOCAL_TERMINAL':({'RETIREMENT','RECONCILIATION'},[p.get('grant'),self.s['grants'].get(p.get('grant') if isinstance(p.get('grant'),str) else '',{}).get('token')]),'SEAL_BEGIN':({'BEGIN'},[p.get('round')]),'DRAIN':({'SEAL'},[p.get('round')]),'INSTALL':({'TERMINAL'},[p.get('round')]),'ACK_INSTALL':({'INSTALLATION'},[p.get('round')])}[k]
    source_object=self.proof(p['proof'],*expected)
    source_host=(p['grant']['gateway'] if k=='REGISTER_GRANT' else p['gateway'] if k in {'DRAIN','ACK_INSTALL'} else self.s['tokens'][p['token']]['body']['gateway'] if k in {'IMPORT','RECONCILE'} else self.s['enrollment']['store'])
    require(p['proof']['host']==source_host,'PROOF_HOST')
    objects=[source_object]
   if k=='PREPARE_ENROLL':
    guard(self.s['enrollment']is None and h not in self.s['preparations'],'PREPARATION_EXISTS');require(c['key'][0]==p['scope'] and p['namespace']['scope']==p['scope'] and p['namespace']['gateway']==h and h!=p['store'],'PREPARATION_BINDING');cap=self.initial['writer_capabilities'].get(h);require(cap is not None,'WRITER_CAPABILITY');self.optional(c,h)
    self.s['gateways'][h]={'namespace':copy.deepcopy(p['namespace']),'epoch':int(cap['epoch']),'state':'OPEN','round':None,'installed':0,'clock_floor':'0001-01-01T00:00:00.000000Z','allocation':0,'receipt':0,'allocation_prefix':0,'receipt_prefix':0,'receipts':{},'seals':{}}
    for i in range(32):self.reserve(h,'close:'+str(i)+':'+h,WORK['bundles']['finish_gateway']['slots'])
    self.s['preparations'][h]=copy.deepcopy(p)
   elif k=='PREPARE_ROUND':
    n=int(p['round']);gw=self.s['gateways'][h];prep=key([p['round'],h]);guard(prep not in self.s['round_preparations'] and gw['state']=='OPEN' and gw['installed']==int(p['predecessor']) and n==int(p['predecessor'])+1,'ROUND_PREPARATION');require(p['enrollment']==digest('enrollment',self.s['enrollment']),'ROUND_ENROLLMENT');self.optional(c,h);self.reserve(h,'optional-round:'+str(n)+':'+h,WORK['bundles']['cancel_gateway']['slots']);self.s['round_preparations'][prep]=copy.deepcopy(p)
   elif k=='ENROLL':
    guard(self.s['enrollment']is None,'ENROLLED');require(p['base_receipt']==self.initial['original_base_receipt'] and p['base_manifest']==self.initial['original_base_manifest'],'ORIGINAL_BASE')
    require(c['key'][0]==p['scope'],'SCOPE');families=p['families'];fkeys=[key(f['key']) for f in families];require(len(set(fkeys))==len(fkeys),'FAMILY_DUPLICATE')
    for i,f in enumerate(families):
     require(f['key'][0]==p['scope'] and f['key'][3]==p['target'],'FAMILY_TARGET');require(all(0<=a<len(families) and a!=i for a in f['prerequisites']),'PREREQUISITE')
    def visit(i,seen):
     require(i not in seen,'CYCLE')
     for n in families[i]['prerequisites']:visit(n,seen|{i})
    for i in range(len(families)):visit(i,set())
    ns=p['gateways'];require(len({g['gateway'] for g in ns})==len(ns) and len({g['tag'] for g in ns})==len(ns),'GATEWAY_DUPLICATE')
    preparation_objects=[];require(len(p['preparations'])==len(ns),'PREPARATION_COUNT');intent=digest('enrollment',{a:b for a,b in p.items() if a!='preparations'})
    for n in ns:
     g=n['gateway'];matches=[q for q in p['preparations'] if q['host']==g];require(len(matches)==1,'PREPARATION_HOST');proof=matches[0];preparation_objects.append(self.proof(proof,{'ENROLL_PREPARATION'},[g],p));prepared=self.s['preparations'].get(g);require(prepared is not None and prepared['intent']==intent and prepared['namespace']==n and prepared['store']==p['store'] and prepared['scope']==p['scope'] and prepared['registration']==p['registration'],'PREPARATION_BINDING');require(proof['root']==self.journal(g)['root'],'PREPARATION_CURRENT')
    for f in families:self.s['families'][key(f['key'])]={'terms':copy.deepcopy(f),'closed':False,'unavailable':False}
    for f in families:
     require(f['roles']['bearer']==f['roles']['payer'] or f['roles'].get('payer_delegation') in self.initial['authority_documents'],'PAYER_DELEGATION');require(f['assent'] in self.initial['authority_documents'],'ASSENT');require(f['starts_at']<f['occurs_before'] and f['received_by']<=f['accepted_by']<=f['correction_by'],'TERMS_WINDOW')
    for u in p['suppliers']:require(int(u['maximum'])==sum(int(u[d]) for d in ['consumed','held','released']),'SUPPLIER_CONSERVATION');self.s['suppliers'][u['id']]=copy.deepcopy(u)
    require(len({q['id'] for q in p['pools']})==len(p['pools']) and len({q['id'] for q in p['suppliers']})==len(p['suppliers']),'POOL_DUPLICATE')
    for q in p['pools']:
     for au in q['authorizations']:require(au['assent'] in self.initial['authority_documents'] and (au['roles']['payer']==au['roles']['bearer'] or au['roles'].get('payer_delegation') in self.initial['authority_documents']),'POOL_AUTHORITY')
     self.s['pools'][q['id']]={'terms':copy.deepcopy(q),'used':0,'positive':0,'negative':0,'gross':0}
    for f in families:require((f['book']=='RETAIL' and f['supplier_pool']=='none') or (f['book']=='SUPPLIER' and f['supplier_pool'] in self.s['suppliers']),'SUPPLIER_FAMILY')
    objects=copy.deepcopy(self.initial.get('original_objects',[]));require(bool(objects),'ORIGINAL_OBJECTS');require(sum(int(o['bytes']) for o in objects)<=1048576,'ORIGINAL_OBJECT_BYTES')
    for o in objects:validate_shape('object',o);require(o['kind']=='ORIGINAL_BASE','ORIGINAL_OBJECT_KIND');require(o['origin']=={'store':p['store'],'scope':p['scope'],'registration':p['registration'],'host':h,'ordinal':str(int(j['ordinal'])+1)},'ORIGINAL_ORIGIN');raw=base64.b64decode(o['body']);require(len(raw)==int(o['bytes']) and hashlib.sha256(raw).hexdigest()==o['body_hash'],'ORIGINAL_OBJECT_HASH');require(canonical(strict(raw))==raw,'ORIGINAL_CANONICAL')
    require(any(o['body_hash']==p['base_receipt'] for o in objects) and any(o['body_hash']==p['base_manifest'] for o in objects),'ORIGINAL_MEMBERSHIP')
    from base_bridge import verify
    verify(objects,p,canonical,require)
    objects+=preparation_objects
    self.s['enrollment']=copy.deepcopy(p);self.s['customer']=int(p['base_atoms']);self.optional(c,h)
    for i in range(32):
     self.reserve(h,'close:'+str(i),WORK['bundles']['finish_central']['slots'])
   elif k=='LOCAL_GRANT':
    g=p['grant'];gid=g['id'];prefix='gr1.'+g['namespace']['tag']+'.';require(gid.startswith(prefix) and 1<=len(gid[len(prefix):].encode())<=91,'GRANT_NAMESPACE');require(g['authentication']==digest('grant',{a:b for a,b in g.items() if a!='authentication'}),'GRANT_DIGEST');require(g['authentication']in self.initial['grant_authentications'],'GRANT_AUTH')
    guard(gid not in self.s['grants'],'GRANT_USED');gw=self.s['gateways'][h];require(g['journal_head']==j['root'],'GRANT_HEAD');require(g['store']==self.s['enrollment']['store'] and g['registration']==self.s['enrollment']['registration'] and g['namespace']==gw['namespace'],'GRANT_BINDING');guard(gw['state']=='OPEN','SEALED')
    expected_bundle=WORK['bundles']['local_grant'];expected_vector=dict(zip(DIMS,[expected_bundle['retained']['segment_bytes'],expected_bundle['retained']['new_trusted_bytes'],expected_bundle['retained']['records'],expected_bundle['retained']['index_path_pages'],expected_bundle['retained']['index_value_pages'],expected_bundle['peak_workspace']]))
    require(all(int(g['resources'][d])>=n for d,n in expected_vector.items()) and all(int(g['counters'][d])>=n for d,n in expected_bundle['counters'].items()),'GRANT_ENVELOPE')
    self.reserve(h,'grant-local:'+gid,WORK['bundles']['local_grant']['slots']);self.spend('grant-local:'+gid,k)
    self.s['grants'][gid]={'body':copy.deepcopy(g),'state':'LOCAL_HELD','local_terminal':False,'token':None}
   elif k=='REGISTER_GRANT':
    g=p['grant'];x=self.s['grants'].get(g['id']);guard(x is not None and x['body']==g,'GRANT_PROOF');guard(x['state']=='LOCAL_HELD','GRANT_STATE');self.reserve(h,'grant-central:'+g['id'],WORK['bundles']['central_grant']['slots']);self.spend('grant-central:'+g['id'],k);x['state']='REGISTERED_UNCLAIMED'
   elif k=='ISSUE':
    x=self.s['grants'].get(p['grant']);guard(x is not None and x['state']=='REGISTERED_UNCLAIMED','GRANT_UNAVAILABLE');t=p['token'];g=t['gateway'];gw=self.s['gateways'][g];guard(t['grant']==p['grant'] and g==x['body']['gateway'],'GRANT_BINDING');guard(t['id']not in self.s['tokens'],'TOKEN_USED')
    guard(self.s['active']is None or t['category']=='ADJUSTMENT','ISSUANCE_FROZEN');require(int(t['allocation'])==gw['allocation']+1,'ALLOCATION_GAP');require(t['claim']==digest('claim',[t['grant'],t['id'],g,t['allocation'],t['category']]),'CLAIM_DIGEST')
    self.reserve(h,'token:'+t['id'],WORK['bundles']['central_token']['slots']);self.spend('token:'+t['id'],k);gw['allocation']+=1;x['state']='CLAIMED';self.terminal_slack('grant-central:'+p['grant']);x['token']=t['id'];self.s['tokens'][t['id']]={'body':copy.deepcopy(t),'state':'ISSUED','imported':False,'reconciled':False,'advanced':False,'receipt_advanced':False,'case':None,'receipt':None,'delivery':None}
   elif k=='ACTIVATE':
    t=self.s['tokens'][p['token']];guard(t['body']['gateway']==h and t['state']=='ISSUED','TOKEN_STATE');guard(self.s['gateways'][h]['state']=='OPEN','SEALED');self.spend('grant-local:'+t['body']['grant'],k);t['state']='ACTIVE'
   elif k=='RECEIVE':
    t=self.s['tokens'][p['token']];gw=self.s['gateways'][h];dk=key(p['delivery']);sub=p['submission'];cid=key(sub['case']);sh=digest('submission',sub)
    if dk in self.s['deliveries']:
     old=self.s['deliveries'][dk];guard(old['submission']==sh,'IDENTITY_CONFLICT');self.role(c,'read');self.s['duplicates']+=1;return {'status':'DUPLICATE','code':'RECEIPT_RETRY','effects':[{'kind':'RECEIPT','body':old['receipt']}],'root':j['root']}
    guard(t['body']['gateway']==h and t['state']=='ACTIVE','TOKEN_STATE');guard(gw['state']=='OPEN','SEALED');guard(int(p['epoch'])==gw['epoch'],'WRITER_EPOCH');guard(sub['occurred_at']<=p['received_at']==c['authority']['observed_at'],'RECEIPT_TIME');guard(p['received_at']>=gw['clock_floor'],'CLOCK_BEHIND')
    ns=gw['namespace'];ext=p['delivery'][2];prefix='gw1.'+ns['tag']+'.';guard(p['delivery'][0]==ns['scope'] and ext.startswith(prefix) and 1<=len(ext[len(prefix):].encode())<=91,'NAMESPACE')
    order=list(self.s['gateways']);owner=order[int(digest('route',sub['case']),16)%len(order)];guard(owner==h,'WRONG_OWNER')
    require(key(sub['case'][0])in self.s['families'],'UNKNOWN_FAMILY');require(sub['case'][1]==self.s['families'][key(sub['case'][0])]['terms']['source'],'SOURCE')
    for e in sub['evidence']:require(hashlib.sha256(base64.b64decode(e['body'])).hexdigest()==e['sha256'],'EVIDENCE_HASH')
    existing=self.s['cases'].get(cid)
    if existing:guard(existing['submission']==sh,'CASE_CONFLICT');receipt=copy.deepcopy(existing['receipt']);t['state']='ALIAS'
    else:
     gw['receipt']+=1;receipt={'case':copy.deepcopy(sub['case']),'delivery':copy.deepcopy(p['delivery']),'submission':sh,'token':p['token'],'gateway':h,'epoch':p['epoch'],'position':str(gw['receipt']),'received_at':p['received_at'],'journal_head':j['root']};gw['receipts'][gw['receipt']]=p['token'];t['state']='NEW_CASE';self.s['cases'][cid]={'key':copy.deepcopy(sub['case']),'submission':sh,'input':copy.deepcopy(sub),'receipt':receipt,'imported':False,'state':'LOCAL','transfer':None,'revision':0,'signed':0,'evidence':copy.deepcopy(sub['evidence'])}
    self.spend('grant-local:'+t['body']['grant'],k,{'receipt':0} if t['state']=='ALIAS' else None);t.update({'case':cid,'receipt':receipt,'delivery':copy.deepcopy(p['delivery'])});self.s['deliveries'][dk]={'submission':sh,'receipt':receipt,'token':p['token']};effects=[{'kind':'RECEIPT','body':receipt}]
   elif k=='RETURN_UNUSED':
    t=self.s['tokens'][p['token']];guard(t['body']['gateway']==h and t['state']in {'ISSUED','ACTIVE'},'TOKEN_STATE');require(p['claim']==t['body']['claim'],'CLAIM_PROOF');self.spend('grant-local:'+t['body']['grant'],k);t['state']='RETURNED_UNUSED'
   elif k=='IMPORT':
    t=self.s['tokens'][p['token']];guard(t['state']in {'NEW_CASE','ALIAS'} and not t['imported'],'TOKEN_STATE');case=self.s['cases'][t['case']]
    if t['state']=='ALIAS':guard(case['imported'],'ORIGINAL_NOT_IMPORTED')
    else:
     case['imported']=True;case['admission']=int(j['ordinal'])+1;case['state']='ADJUSTMENT_PENDING' if self.s['families'][key(case['key'][0])]['unavailable'] else 'ORDINARY_PENDING'
    self.spend('token:'+p['token'],k);t['imported']=True;gw=self.s['gateways'][t['body']['gateway']]
   elif k=='RECONCILE':
    t=self.s['tokens'][p['token']];guard(not t['reconciled'] and (t['state']=='RETURNED_UNUSED' or t['imported']),'UNRECONCILED');self.spend('token:'+p['token'],k);t['reconciled']=True
   elif k=='ADVANCE':
    g=p['gateway'];gw=self.s['gateways'][g];n=int(p['through']);guard(n==gw['allocation_prefix']+1,'PREFIX_GAP');ts=[(tid,t) for tid,t in self.s['tokens'].items() if t['body']['gateway']==g and int(t['body']['allocation'])==n];require(len(ts)==1,'ALLOCATION_MEMBERSHIP');tid,t=ts[0];guard(t['reconciled'],'UNRECONCILED');self.spend('token:'+tid,k);t['advanced']=True;gw['allocation_prefix']=n
    if t['state']!='NEW_CASE' or t['receipt_advanced']:self.terminal_slack('token:'+tid)
   elif k=='ADVANCE_RECEIPT':
    g=p['gateway'];gw=self.s['gateways'][g];n=int(p['through']);guard(n==gw['receipt_prefix']+1 and n in gw['receipts'],'RECEIPT_PREFIX_GAP');tid=gw['receipts'][n];t=self.s['tokens'][tid];guard(t['imported'] and t['state']=='NEW_CASE','RECEIPT_NOT_IMPORTED');self.spend('token:'+tid,k);t['receipt_advanced']=True;gw['receipt_prefix']=n
    if t['advanced']:self.terminal_slack('token:'+tid)
   elif k=='RETIRE_GRANT':
    x=self.s['grants'][p['grant']];guard(x['state']=='REGISTERED_UNCLAIMED','GRANT_CLAIMED');self.spend('grant-central:'+p['grant'],k);x['state']='RETIRED_UNCLAIMED';self.terminal_slack('grant-central:'+p['grant'])
   elif k=='LOCAL_TERMINAL':
    gid=p['grant'];x=self.s['grants'][gid];guard(x['body']['gateway']==h and not x['local_terminal'],'GRANT_STATE');guard(x['state']=='RETIRED_UNCLAIMED' or (x['token']and self.s['tokens'][x['token']]['reconciled']),'TERMINAL_PROOF');self.spend('grant-local:'+gid,k);x['local_terminal']=True;self.terminal_slack('grant-local:'+gid)
   elif k=='BEGIN':
    n=int(p['round']);guard(self.s['active']is None,'ACTIVE_ROUND');guard(n==self.s['last_round']+1 and int(p['predecessor'])==self.s['last_round'],'ROUND_PREDECESSOR');fs=[key(f) for f in p['families']];guard(all(f in self.s['families'] and not self.s['families'][f]['closed'] for f in fs),'CLOSE_RIGHT');guard(all(g in self.s['gateways'] for g in p['gateways']),'GATEWAY')
    if p['mode']=='FINISH_ONLY':require(p['preparations']==[],'FINISH_PREPARATION');owner='close:'+str(next(i for i,f in enumerate(self.s['enrollment']['families']) if key(f['key'])in fs))
    else:
     require(len(p['preparations'])==len(p['gateways']),'ROUND_PREPARATIONS');owner='optional-round:'+str(n);self.reserve(h,owner,WORK['bundles']['cancel_central']['slots'])
     for g in p['gateways']:
      matches=[q for q in p['preparations'] if q['host']==g];require(len(matches)==1,'ROUND_PREPARATION_HOST');objects.append(self.proof(matches[0],{'ROUND_PREPARATION'},[digest('namespace',[g,p['round']])]));prepared=self.s['round_preparations'].get(key([p['round'],g]));require(prepared is not None and prepared['predecessor']==p['predecessor'] and prepared['enrollment']==digest('enrollment',self.s['enrollment']),'ROUND_PREPARATION_BINDING')
    self.spend(owner,k);self.s['rounds'][n]={'id':n,'mode':p['mode'],'families':copy.deepcopy(p['families']),'gateways':list(p['gateways']),'cutoffs':{g:self.s['gateways'][g]['allocation'] for g in p['gateways']},'state':'DRAINING','owner':owner,'sealed':{},'drained':set(),'installed':set(),'acknowledged':set(),'predecessor':int(p['predecessor'])};self.s['active']=n;effects=[{'kind':'ROUND_BEGIN','body':{'round':p['round'],'predecessor':p['predecessor'],'mode':p['mode'],'cutoffs':sorted_set([{'gateway':g,'cutoff':str(a)} for g,a in self.s['rounds'][n]['cutoffs'].items()])}}]
   elif k in {'SEAL_BEGIN','SEALED','DRAIN','READY','CLOSE','ABORT','INSTALL','ACK_INSTALL'}:
    n=int(p['round']);r=self.s['rounds'].get(n);guard(r is not None,'ROUND_UNKNOWN');g=p.get('gateway');owner=r['owner'];gw=self.s['gateways'].get(g)
    if k=='SEAL_BEGIN':guard(g in r['gateways'] and int(p['predecessor'])==r['predecessor'] and gw['installed']==r['predecessor'] and gw['state']=='OPEN','ROUND_STALE');self.spend(owner+':'+g,k);gw['state']='SEALING';gw['round']=n
    elif k=='SEALED':
     guard(gw['round']==n and gw['state']=='SEALING','ROUND_STALE');known_facts=[json.loads(z) for z in self.s['object_inventory'].get(g,{})];local_claims={z[2] for z in known_facts if z[1]=='CLAIM'};local_dispositions={z[2] for z in known_facts if z[0]['host']==g and z[1]in {'RECEIPT','ALIAS','RETURNED_UNUSED'}};ts=[t for tid,t in self.s['tokens'].items() if tid in local_claims and tid in local_dispositions and t['body']['gateway']==g and int(t['body']['allocation'])<=r['cutoffs'][g]];positions=sorted(int(t['body']['allocation']) for t in ts);guard(len(positions)==r['cutoffs'][g] and all(a==i+1 for i,a in enumerate(positions)) and all(t['state']in {'NEW_CASE','ALIAS','RETURNED_UNUSED'} for t in ts),'UNRESOLVED_TOKEN');self.spend(owner+':'+g,k);gw['state']='SEALED';r['sealed'][g]={'high':gw['receipt'],'receipt_root':digest('receipt',[[str(a),self.s['tokens'][b]['receipt']] for a,b in sorted(gw['receipts'].items())]),'disposition_root':digest('result',[[t['body']['allocation'],t['body']['id'],t['state']] for t in sorted(ts,key=lambda t:int(t['body']['allocation']))])}
     effects=[{'kind':'SEAL','body':{'round':p['round'],'gateway':g,'cutoff':str(r['cutoffs'][g]),'receipt_high':str(gw['receipt']),'disposition_root':r['sealed'][g]['disposition_root'],'receipt_root':r['sealed'][g]['receipt_root']}}]
    elif k=='DRAIN':guard(g in r['sealed'] and gw['allocation_prefix']>=r['cutoffs'][g] and gw['receipt_prefix']>=r['sealed'][g]['high'],'UNRECONCILED_FENCE');self.spend(owner,k);r['drained'].add(g);r['sealed'][g]['observation']=p['proof']['trusted_observation_ref']
    elif k=='READY':guard(r['state']=='DRAINING' and r['drained']==set(r['gateways']),'UNRECONCILED_FENCE');self.spend(owner,k);r['state']='READY'
    elif k=='ABORT':guard(r['mode']=='CANCELLABLE' and r['state']in {'DRAINING','READY'},'ABORT_REFUSED');self.spend(owner,k);r['state']='ABORTED'
    elif k=='CLOSE':
     guard(r['state']=='READY','NOT_READY');require(p['closed_at']==c['authority']['observed_at'],'CLOSE_TIME');closed=[key(f) for f in r['families']];guard(all(not self.s['families'][f]['closed'] for f in closed),'CLOSE_RIGHT');self.spend(owner,k)
     family_heads=[]
     for fk,family in self.s['families'].items():
      ent={'status':'UNCONSUMED'}
      if fk in self.s['entitlements']:
       consumer=self.s['cases'][self.s['entitlements'][fk]];ent={'status':'CONSUMED','case':consumer['key'],'revision':str(consumer['revision']),'head':digest('result',{'case':consumer['key'],'state':consumer['state'],'revision':str(consumer['revision']),'signed':str(consumer['signed']),'receipt':digest('receipt',consumer['receipt'])})}
      family_heads.append({'family':family['terms']['key'],'terms':digest('enrollment',family['terms']),'closed':family['closed'],'unavailable':family['unavailable'],'entitlement':ent})
     for f in closed:self.s['families'][f]['closed']=True;self.s['families'][f]['unavailable']=True
     ordered=list(self.s['families']);change=True
     while change:
      change=False
      for fk,f in self.s['families'].items():
       if not f['unavailable'] and any(self.s['families'][ordered[a]]['unavailable'] and ordered[a]not in self.s['entitlements'] for a in f['terms']['prerequisites']):f['unavailable']=True;change=True
     sb=[];sa=[]
     for sid,sup in self.s['suppliers'].items():
      relevant=[f for f in self.s['families'].values() if f['terms']['supplier_pool']==sid]
      if relevant and any(self.s['families'][f]['terms']['supplier_pool']==sid for f in closed) and all(f['closed'] for f in relevant):sb.append(copy.deepcopy(sup));sup['released']=str(int(sup['released'])+int(sup['held']));sup['held']='0';sa.append(copy.deepcopy(sup))
     coverage=[]
     for gg in r['gateways']:
      ggstate=self.s['gateways'][gg];seal=r['sealed'][gg];coverage.append({'gateway':gg,'status':'COMPLETE_GATEWAY_CUTOFF','cutoff':str(r['cutoffs'][gg]),'allocation_prefix':str(ggstate['allocation_prefix']),'receipt_high':str(seal['high']),'receipt_prefix':str(ggstate['receipt_prefix']),'disposition_root':seal['disposition_root'],'receipt_root':seal['receipt_root'],'observation':seal['observation']})
     cert={'predecessor':j['root'],'enrollment':digest('enrollment',self.s['enrollment']),'family_heads':sorted_set(family_heads),'families':sorted_set(r['families']),'unavailable':sorted_set([f['terms']['key'] for f in self.s['families'].values() if f['unavailable']]),'supplier_before':sorted_set(sb),'supplier_after':sorted_set(sa),'round':str(n),'cutoffs':sorted_set(coverage),'closed_at':p['closed_at']};chash=digest('closure',cert)
     for case in self.s['cases'].values():
      if case['state']=='ORDINARY_PENDING' and self.s['families'][key(case['key'][0])]['unavailable']:case['state']='ADJUSTMENT_PENDING';case['transfer']=case['transfer']or chash
     self.s['certificates'].append(cert);r['state']='COMMITTED';r['closed_at']=p['closed_at'];effects=[{'kind':'CLOSURE','body':cert}]
    elif k=='INSTALL':
     guard(g in r['gateways'] and r['state']in {'COMMITTED','ABORTED'} and p['outcome']==r['state'] and gw['installed']==r['predecessor'] and gw['round']in {None,n},'ROUND_STALE');self.spend(owner+':'+g,k);gw['installed']=n;gw['state']='OPEN';gw['round']=None;r['installed'].add(g)
     if r['state']=='COMMITTED':gw['clock_floor']=max(gw['clock_floor'],r['closed_at'])
     self.terminal_slack(owner+':'+g)
    elif k=='ACK_INSTALL':guard(g in r['installed'] and g not in r['acknowledged'],'INSTALL_UNKNOWN');self.spend(owner,k);r['acknowledged'].add(g)
    if r['state']in {'COMMITTED','ABORTED'} and r['acknowledged']==set(r['gateways']):self.s['active']=None;self.s['last_round']=n;self.terminal_slack(owner)
   elif k=='SUPPLEMENT':
    case=self.s['cases'][key(p['case'])];guard(case['state']in {'ORDINARY_PENDING','ADJUSTMENT_PENDING'},'CASE_FINAL')
    for e in p['evidence']:require(hashlib.sha256(base64.b64decode(e['body'])).hexdigest()==e['sha256'],'EVIDENCE_HASH')
    merged={e['sha256']:e for e in case['evidence']+p['evidence']};guard(len(merged)<=16,'EVIDENCE_LIMIT');self.optional(c,h);case['evidence']=sorted_set(list(merged.values()))
   elif k in {'DECIDE','CORRECT'}:
    cid=key(p['case']);case=self.s['cases'][cid];fk=key(case['key'][0]);f=self.s['families'][fk];terms=f['terms'];now=c['authority']['observed_at']
    if k=='CORRECT':
     guard(case['state']=='FINAL_ALLOW' and int(p['expected_revision'])==case['revision'],'REVISION');guard(p['replacement']in terms['correction_atoms'] and now<=terms['correction_by'],'CORRECTION_TERMS');guard(p['roles']==case['roles'] and p['assent']==terms['assent'],'CORRECTION_AUTH');self.optional(c,h);old=case['signed'];new=int(p['replacement']);case['revision']+=1;case['signed']=new;amounts=[('INVERSE',-old),('REPLACEMENT',new)];self.s['customer']+=(new-old if terms['book']=='RETAIL' else 0)
    else:
     guard(case['state']in {'ORDINARY_PENDING','ADJUSTMENT_PENDING'},'CASE_FINAL');guard(case['receipt']['received_at']<=now,'DECISION_TIME')
     if p['verdict']=='DENY':self.optional(c,h);case['state']='FINAL_DENY';amounts=[]
     else:
      guard(fk not in self.s['entitlements'],'ENTITLEMENT');a=int(p['signed_atoms']);guard(abs(a)<=M,'AMOUNT');path=p['path']
      if path=='ORDINARY':
       guard(case['state']=='ORDINARY_PENDING' and not f['unavailable'],'ORDINARY_CLOSED');guard(p['signed_atoms']==terms['ordinary_atoms'] and p['roles']==terms['roles'] and p['assent']==terms['assent'],'ORIGINAL_TERMS');guard(all(key(self.s['enrollment']['families'][i]['key'])in self.s['entitlements'] for i in terms['prerequisites']),'PREREQUISITE');guard(terms['starts_at']<=case['input']['occurred_at']<terms['occurs_before'] and case['receipt']['received_at']<=terms['received_by'] and now<=terms['accepted_by'],'WINDOW')
       sid=terms['supplier_pool']
       if terms['book']=='SUPPLIER':
        sup=self.s['suppliers'][sid];guard(a>=0 and a<=int(sup['held']),'SUPPLIER_CAPACITY');sup['consumed']=str(int(sup['consumed'])+a);sup['held']=str(int(sup['held'])-a)
       guard(sum(max(0,x['body']['signed_atoms'] and int(x['body']['signed_atoms'])) for x in self.s['actions'] if x['body']['kind']=='ORDINARY' and x['body']['book']=='RETAIL')+(max(0,a) if terms['book']=='RETAIL' else 0)<=int(self.s['enrollment']['premium_cap']),'PREMIUM_CAP')
      else:
       guard(case['state']=='ADJUSTMENT_PENDING' and f['unavailable'],'ADJUSTMENT_PATH');pool=self.s['pools'].get(p['pool']);guard(pool is not None and any(au['direction']==('POSITIVE' if a>0 else 'NEGATIVE' if a<0 else 'ZERO') and p['roles']==au['roles'] and p['assent']==au['assent'] for au in pool['terms']['authorizations']),'ADJUSTMENT_AUTH');mag=abs(a);pos=max(a,0);neg=max(-a,0)
       for field,n in [('funding',pool['used']+mag),('gross',pool['gross']+mag),('positive',pool['positive']+pos),('negative',pool['negative']+neg)]:guard(n<=int(pool['terms'][field]),'ADJUSTMENT_'+field)
       pool['used']+=mag;pool['gross']+=mag;pool['positive']+=pos;pool['negative']+=neg;self.s['gross']+=mag;self.s['positive']+=pos;self.s['negative']+=neg
      self.optional(c,h);self.s['entitlements'][fk]=cid;case['state']='FINAL_ALLOW';case['revision']=1;case['signed']=a;case['roles']=copy.deepcopy(p['roles']);self.s['customer']+=(a if terms['book']=='RETAIL' else 0);amounts=[(path,a)]
    for ak,a in amounts:
     if a:effects.append({'kind':'ACTION','body':{'book':terms['book'],'case':copy.deepcopy(case['key']),'revision':str(case['revision']),'kind':ak,'signed_atoms':str(a),'magnitude':str(abs(a)),'roles':copy.deepcopy(p['roles']),'assent':p['assent']}})
    self.s['actions']+=copy.deepcopy(effects)
   elif k=='REPLACE_WRITER':
    gw=self.s['gateways'][h];cap=self.initial['writer_capabilities'][h];guard(int(p['old_epoch'])==gw['epoch'] and int(p['new_epoch'])==gw['epoch']+1,'WRITER_EPOCH');require(p['fence']==cap['fence'] and p['journal_head']==j['root'],'FENCE_PROOF');self.optional(c,h);gw['epoch']=int(p['new_epoch'])
   elif k=='EXTEND_RESOURCES':
    self.optional(c,h);host=p['host'];a=self.s['resources'][host]
    for d in DIMS:guard(a['provisioned'][d]+int(p['resources'][d])<=M,'RESOURCE_MAX');a['provisioned'][d]+=int(p['resources'][d])
   else:raise Invalid('COMMAND_KIND')
   # Typed nonrecursive source facts bind exact payload/effects to journal membership.
   fact_kind={'PREPARE_ENROLL':'ENROLL_PREPARATION','PREPARE_ROUND':'ROUND_PREPARATION','ENROLL':'ENROLLMENT','LOCAL_GRANT':'GRANT','ISSUE':'CLAIM','RECEIVE':('ALIAS' if self.s['tokens'].get(p.get('token') if isinstance(p.get('token'),str) else '',{}).get('state')=='ALIAS' else 'RECEIPT'),'RETURN_UNUSED':'RETURNED_UNUSED','RECONCILE':'RECONCILIATION','RETIRE_GRANT':'RETIREMENT','BEGIN':'BEGIN','SEALED':'SEAL','CLOSE':'TERMINAL','ABORT':'TERMINAL','INSTALL':'INSTALLATION'}.get(k)
   if fact_kind:
    full_key=(p['gateway'] if k=='PREPARE_ENROLL' else digest('namespace',[p['gateway'],p['round']]) if k=='PREPARE_ROUND' else p['registration'] if k=='ENROLL' else p['grant']['id'] if k=='LOCAL_GRANT' else p['token']['id'] if k=='ISSUE' else p['token'] if k in {'RECEIVE','RETURN_UNUSED','RECONCILE'} else p['grant'] if k=='RETIRE_GRANT' else p['round'])
    raw=canonical({'payload':p,'effects':effects});require(len(raw)<=262144,'FACT_BYTES');objects.append({'origin':{'store':p['store'] if k=='PREPARE_ENROLL' else self.s['enrollment']['store'],'scope':p['scope'] if k=='PREPARE_ENROLL' else self.s['enrollment']['scope'],'registration':p['registration'] if k=='PREPARE_ENROLL' else self.s['enrollment']['registration'],'host':h,'ordinal':str(int(j['ordinal'])+1)},'kind':fact_kind,'full_key':full_key,'body':base64.b64encode(raw).decode(),'body_hash':hashlib.sha256(raw).hexdigest(),'bytes':str(len(raw))})
   self.s['object_inventory'][h]=dict(self.s['object_inventory'].get(h,{}));known=self.s['object_inventory'][h]
   introduced=[]
   for o in objects:
    identity=key([o['origin'],o['kind'],o['full_key'],o['body_hash'],o['bytes']])
    if identity not in known:introduced.append(o)
   objects=introduced
   # Actual derived index versions are distinct from reserved maximum page slots.
   index_counts={'PREPARE_ENROLL':40,'PREPARE_ROUND':7,'ENROLL':4+len(objects)+len(p.get('families',[]))+2*len(p.get('gateways',[]))+len(p.get('suppliers',[]))+len(p.get('pools',[]))+32+1,'LOCAL_GRANT':6,'REGISTER_GRANT':6,'ISSUE':8,'ACTIVATE':6,'RECEIVE':5 if self.s['tokens'].get(p.get('token') if isinstance(p.get('token'),str) else '',{}).get('state')=='ALIAS' else 7,'RETURN_UNUSED':6,'IMPORT':6 if self.s['tokens'].get(p.get('token') if isinstance(p.get('token'),str) else '',{}).get('state')=='ALIAS' else 7,'RECONCILE':5,'ADVANCE':5,'ADVANCE_RECEIPT':5,'LOCAL_TERMINAL':5,'RETIRE_GRANT':5,'BEGIN':5 if p.get('mode')=='FINISH_ONLY' else 6,'SEAL_BEGIN':5,'SEALED':5,'DRAIN':5,'READY':5,'CLOSE':6+len(p.get('families',[])),'ABORT':5,'INSTALL':5,'ACK_INSTALL':5,'SUPPLEMENT':6,'DECIDE':6 if p.get('verdict')=='DENY' else 9+(2 if effects else 0),'CORRECT':7+len(effects),'REPLACE_WRITER':6,'EXTEND_RESOURCES':6}
   actual_index=index_counts[k]+(len(objects) if k!='ENROLL' else 0)
   if k=='CLOSE':actual_index=6+len(self.s['rounds'][int(p['round'])]['families'])+len(effects[0]['body']['supplier_after'])+len(objects)
   for charged_host,charged_kind in self._charges:
    maximum=WORK['transitions'][charged_kind]['counter_increments']['index_cardinality'];require(actual_index<=maximum,'INDEX_ENVELOPE');self.s['counters'][charged_host]['index_cardinality']['q']-=maximum-actual_index
   objects=sorted_set(objects)
   # Retain exactly one segment in the owning journal. No global distributed commit.
   nr=digest('replay',[j['root'],ch,effects]);result={'status':'COMMITTED','code':k,'effects':effects,'root':nr};seg={'host':h,'profile':'central-adjudication-r3/1','ordinal':str(int(j['ordinal'])+1),'previous':j['segment'],'previous_root':j['root'],'command':copy.deepcopy(c),'result':copy.deepcopy(result),'dependencies':sorted_set(([p['proof']['segment']] if 'proof' in p and isinstance(p['proof'],dict) else [])+[q['segment'] for q in p.get('preparations',[])]),'objects':objects};validate_shape('segment',seg);require(len(canonical(c))+len(canonical(result))+sum(int(o['bytes']) for o in {o['body_hash']:o for o in objects}.values())<=2097152,'TRUST_BYTES');require(len(canonical(seg))<=8388608,'SEGMENT_BYTES');j.update({'ordinal':seg['ordinal'],'segment':digest('segment',seg),'root':nr});self.s['segments'].append(seg)
   for o in objects:known[key([o['origin'],o['kind'],o['full_key'],o['body_hash'],o['bytes']])]=True
   self.s['controls'][ck]={'digest':ch,'effects':copy.deepcopy(effects)};self.read_index.setdefault(h,[]).append({'bytes':canonical(seg),'segment':j['segment'],'root':nr,'previous':seg['previous'],'previous_root':seg['previous_root']});return result
  except Refused as e:self.s=before;self.s['refused']+=1;return {'status':'REFUSED','code':str(e),'effects':[],'root':self.s['journals'].get(h,{'root':ZERO})['root']}
  except Exception:self.s=before;raise
 def snapshot(self):
  def decimals(value):
   if type(value)is int:return str(value)
   if type(value)is dict:return {str(k):decimals(a) for k,a in value.items()}
   if type(value)is list:return [decimals(a) for a in value]
   return copy.deepcopy(value)
  return {'preparations':copy.deepcopy(self.s['preparations']),'round_preparations':copy.deepcopy(self.s['round_preparations']),'object_inventory':{host:sorted(values) for host,values in self.s['object_inventory'].items()},'resources':decimals(self.s['resources']),'counters':decimals(self.s['counters']),'allocations':decimals(self.s['allocations']),'grants':{k:{f:copy.deepcopy(a[f]) for f in ['state','local_terminal','token']} for k,a in self.s['grants'].items()},'tokens':{k:{f:a[f] for f in ['state','imported','reconciled','advanced','receipt_advanced']} for k,a in self.s['tokens'].items()},'cases':{k:dict(state=a['state'],transfer=a['transfer'],revision=str(a['revision']),signed=str(a['signed'])) for k,a in self.s['cases'].items()},'entitlements':copy.deepcopy(self.s['entitlements']),'pools':{k:{f:str(a[f]) for f in ['used','positive','negative','gross']} for k,a in self.s['pools'].items()},'suppliers':sorted(copy.deepcopy(list(self.s['suppliers'].values())),key=lambda a:a['id']),'actions':copy.deepcopy(self.s['actions']),'certificates':copy.deepcopy(self.s['certificates'])}
 def summary(self,n):
  return {'commands':n,'segments':len(self.s['segments']),'duplicates':self.s['duplicates'],'refused':self.s['refused'],'grants':len(self.s['grants']),'tokens':len(self.s['tokens']),'receipts':sum(g['receipt'] for g in self.s['gateways'].values()),'cases':len(self.s['cases']),'aliases':sum(t['state']=='ALIAS' for t in self.s['tokens'].values()),'customer_atoms':str(self.s['customer']),'adjustment_gross':str(self.s['gross']),'entitlements':len(self.s['entitlements']),'suppliers':sorted(self.s['suppliers'].values(),key=lambda x:x['id']),'round':str(self.s['last_round']),'allocation_prefix':{g:str(v['allocation_prefix']) for g,v in self.s['gateways'].items()},'receipt_prefix':{g:str(v['receipt_prefix']) for g,v in self.s['gateways'].items()},'root':digest('replay',sorted([[g,j['root']] for g,j in self.s['journals'].items()])),'journals':copy.deepcopy(self.s['journals'])}
def replay(trace):
 require(type(trace)is dict and set(trace)=={'format','initial','commands'} and trace['format']=='r3-trace/1','TRACE_SHAPE');l=Ledger(trace['initial']);results=[l.execute(c) for c in trace['commands']];return {'accepted':True,'summary':l.summary(len(results)),'results':results,'snapshot':l.snapshot()}
def main():
 try:
  if sys.argv[1]=='--self-test':
   from tests import main as run_tests
   run_tests();return
  x=strict(Path(sys.argv[1]).read_bytes());r=replay(x);print(json.dumps(r,separators=(',',':')))
 except Exception as e:print(json.dumps({'accepted':False,'error':str(e)}));sys.exit(1)
if __name__=='__main__':main()
