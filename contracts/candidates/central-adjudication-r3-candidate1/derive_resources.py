import json,math,sys
from pathlib import Path
R=Path(__file__).resolve().parent;S=json.loads((R/'protocol/schema.json').read_text());D=S['$defs']
def can(x):return json.dumps(x,separators=(',',':'),ensure_ascii=False)
def size(s):
 if '$ref'in s:return size(D[s['$ref'].split('/')[-1]])
 if 'const'in s:return len(can(s['const']).encode())
 if 'enum'in s:return max(len(can(x).encode()) for x in s['enum'])
 if 'oneOf'in s:return max(map(size,s['oneOf']))
 t=s['type']
 if t=='object':return 2+sum(len(can(k).encode())+1+size(v) for k,v in s['properties'].items())+max(0,len(s['properties'])-1)
 if t=='array':
  n=s['maxItems'];return 2+(sum(map(size,s['prefixItems'])) if 'prefixItems'in s else n*size(s['items']))+max(0,n-1)
 if t=='boolean':return 5
 if t=='integer':return max(len(str(s['minimum'])),len(str(s['maximum'])))
 if t=='string':
  if 'x-max-utf8'in s:return 2+s['x-max-utf8']*(2 if 'pattern'in s else 6)
  if 'x-base64-bytes'in s:return 2+4*math.ceil(s['x-base64-bytes']/3)
  if s.get('format')=='date-time':return 29
  p=s.get('pattern','')
  if p=='^[0-9a-f]{64}$':return 66
  if p=='^[0-9a-f]{32}$':return 34
  if '-?'in p:return 33
  if '{0,29}'in p:return 32
  raise ValueError(s)
 raise ValueError(s)
# Full flattened case: tenant,environment,agreement,family,target,source,business.
# Revision index adds revision. This is the maximum of every listed index key.
keys={'delivery':[128,128,256,128],'case':[128]*5+[256,128],'revision':[128]*5+[256,128,30],'grant':[128]*6,'token':[128,128,128,128],'round':[128,128,128,30],'family':[128]*5,'allocation':[128,128,128,128,30],'receipt':[128,128,128,128,30],'namespace':[128,128,32],'authority':[128,128,128,128],'resource':[128,128,128,256,128],'control':[128,128,128,256,128],'object':[128,128,128,128,128,30,32,128,64,30]}
K=max(9+sum(2+x for x in a) for a in keys.values());L=8*K+1;P=128
counts={'PREPARE_ENROLL':40,'PREPARE_ROUND':7,'ENROLL':224,'LOCAL_GRANT':6,'REGISTER_GRANT':6,'ISSUE':8,'ACTIVATE':6,'RECEIVE':7,'RETURN_UNUSED':6,'IMPORT':7,'RECONCILE':5,'ADVANCE':5,'ADVANCE_RECEIPT':5,'LOCAL_TERMINAL':5,'RETIRE_GRANT':5,'BEGIN':6,'SEAL_BEGIN':5,'SEALED':5,'DRAIN':5,'READY':5,'CLOSE':80,'ABORT':5,'INSTALL':5,'ACK_INSTALL':5,'SUPPLEMENT':6,'DECIDE':11,'CORRECT':9,'REPLACE_WRITER':6,'EXTEND_RESOURCES':6}
effect_types={'BEGIN':('round_begin',1),'SEALED':('seal',1),'RECEIVE':('receipt',1),'DECIDE':('action',1),'CORRECT':('action',2),'CLOSE':('certificate',1)}
# Fixed segment excludes command/result/dependency values, supplied separately.
empty={'host':'\\'*128,'profile':'central-adjudication-r3/1','ordinal':'9'*30,'previous':'f'*64,'previous_root':'f'*64,'command':None,'result':None,'dependencies':['f'*64]*128,'objects':[]}
fixed=len(can(empty))-8
rows={}
for alt in D['command']['oneOf']:
 k=alt['properties']['kind']['const'];c=size(alt);et,n=effect_types.get(k,('receipt',0));e=n*(size(D[et])+len('{"kind":"REPLACEMENT","body":}')+1)
 result=2+len('"status":"COMMITTED","code":')+130+len(',"effects":[] ,"root":')+66+e
 source_fact_kinds={'PREPARE_ENROLL','PREPARE_ROUND','ENROLL','LOCAL_GRANT','ISSUE','RECEIVE','RETURN_UNUSED','RECONCILE','RETIRE_GRANT','BEGIN','SEALED','CLOSE','ABORT','INSTALL'}
 own_fact=c+result+32 if k in source_fact_kinds else 0
 source_map={'ENROLL':['PREPARE_ENROLL']*4,'BEGIN':['PREPARE_ROUND']*4,'PREPARE_ROUND':['ENROLL'],'LOCAL_GRANT':['ENROLL'],'REGISTER_GRANT':['LOCAL_GRANT'],'ACTIVATE':['ISSUE'],'RETURN_UNUSED':['ISSUE'],'IMPORT':['RECEIVE'],'RECONCILE':['RECEIVE'],'LOCAL_TERMINAL':['RECONCILE'],'SEAL_BEGIN':['BEGIN'],'DRAIN':['SEALED'],'INSTALL':['CLOSE'],'ACK_INSTALL':['INSTALL']}
 def fact_bound(source):
  st,sn=effect_types.get(source,('receipt',0));return size(D[source.lower()])+sn*(size(D[st])+64)+64
 copied_fact=sum(fact_bound(t) for t in source_map.get(k,[]))
 base=1048576 if k=='ENROLL' else 0
 introduced=own_fact+copied_fact+base
 object_count=int(own_fact>0)+len(source_map.get(k,[]))+(128 if base else 0)
 object_metadata=size(D['object'])-(2+4*math.ceil(262144/3))
 b=fixed+c+result+4*math.ceil(introduced/3)+object_count*(object_metadata+4)
 assert c<=262144,(k,c)
 assert b<=8388608,(k,b)
 touches=counts[k]+(object_count if k!='ENROLL' else 4)
 # Value pages bounded by one whole command/result per touched key; no transitive archive copy.
 vp=math.ceil((c+result+introduced)/4032)
 rows[k]={'command_bytes':c,'result_bytes':result,'segment_bytes':b,'new_trusted_bytes':c+result+introduced,'records':1+n+object_count,'index_updates':touches,'index_path_pages':touches*(L+1),'index_value_pages':touches*vp,'index_page_bytes':touches*(L+1)*P,'index_value_bytes':touches*vp*4096,'logical_workspace_bytes':2*b+touches*(L+1)*P+touches*vp*4096,'counter_increments':{x:(touches if x=='index_cardinality' else int(x in {'segment','head_revision','resource_revision'} or (x=='grant' and k=='LOCAL_GRANT') or (x=='grant_registry' and k=='REGISTER_GRANT') or (x=='allocation' and k=='ISSUE') or (x=='receipt' and k=='RECEIVE') or (x=='control' and k in {'PREPARE_ENROLL','PREPARE_ROUND','LOCAL_GRANT','ACTIVATE','RETURN_UNUSED','LOCAL_TERMINAL','SEAL_BEGIN','SEALED','INSTALL','REPLACE_WRITER'}) or (x=='round' and k=='BEGIN') or (x=='import' and k=='IMPORT') or (x=='terminal' and k in {'RECONCILE','RETIRE_GRANT','LOCAL_TERMINAL','INSTALL','ACK_INSTALL'}) or (x=='allocation_prefix' and k=='ADVANCE') or (x=='receipt_prefix' and k=='ADVANCE_RECEIPT') or (x=='writer_epoch' and k=='REPLACE_WRITER') or (x=='economic_revision' and k in {'DECIDE','CORRECT'}))) for x in S['x-counters']}}
# One slot is a conservative complete transition envelope; enrollment and CLOSE are larger.
unit={d:max(v[d] for k,v in rows.items() if k!='ENROLL') for d in ['segment_bytes','new_trusted_bytes','records','index_path_pages','index_value_pages','logical_workspace_bytes']}
# Distinct transitions, conservative union of mutually exclusive branches.
bundles={'local_grant':['LOCAL_GRANT','ACTIVATE','RECEIVE','RETURN_UNUSED','LOCAL_TERMINAL'], 'central_grant':['REGISTER_GRANT','RETIRE_GRANT'], 'central_token':['ISSUE','IMPORT','RECONCILE','ADVANCE','ADVANCE_RECEIPT'], 'finish_central':['BEGIN','READY','CLOSE']+['DRAIN']*4+['ACK_INSTALL']*4, 'finish_gateway':['SEAL_BEGIN','SEALED','INSTALL'], 'cancel_central':['BEGIN','READY','CLOSE','ABORT']+['DRAIN']*4+['ACK_INSTALL']*4,'cancel_gateway':['SEAL_BEGIN','SEALED','INSTALL']}
x={'format':'r3-resource-worksheet/1','actual_counter_exceptions':{'RECEIVE_ALIAS':{'receipt':0},'DECIDE_DENY':{'economic_revision':0}},'scope':'CANONICAL_LOGICAL_ONLY; actual physical page/WAL envelopes pending independent store proof','key_components':keys,'maximum_key_bytes':K,'binary_radix_depth':L,'node_bytes':P,'pages_per_index_update':L+1,'value_page_bytes':4096,'value_page_header_bytes':64,'value_page_payload_bytes':4032,'node_layout':{'tag':1,'flags':1,'depth':2,'reserved_header':4,'left_hash':32,'right_hash':32,'value_hash':32,'reserved_tail':24},'seal_scan':{'page_bytes':4096,'payload_bytes':4032,'staging_bytes':16384,'owner':'active local round finish_gateway or cancel_gateway workspace; held through INSTALL','retained_progress':False},'schema_maxima':{k:size(v) for k,v in D.items() if k not in ['command','segment']},'transitions':rows,'conservative_slot_unit':unit,'bundles':{k:{'slots':v,'slot_count':len(v),'retained':{d:sum(rows[t][d] for t in v) for d in ['segment_bytes','new_trusted_bytes','records','index_path_pages','index_value_pages']},'peak_workspace':max(rows[t]['logical_workspace_bytes'] for t in v),'counters':{c:sum(rows[t]['counter_increments'][c] for t in v) for c in S['x-counters']}} for k,v in bundles.items()}}
expected=json.dumps(x,indent=2)+'\n'
if '--write' in sys.argv:(R/'protocol/resources.json').write_text(expected)
else:assert (R/'protocol/resources.json').read_text()==expected,'RESOURCE_DERIVATION_MISMATCH';print('K,L,P',K,L,P,'max command',max(v['command_bytes'] for v in rows.values()),'max segment',max(v['segment_bytes'] for v in rows.values()))
