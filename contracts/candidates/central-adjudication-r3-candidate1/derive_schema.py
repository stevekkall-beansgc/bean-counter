import json,sys
from pathlib import Path
root=Path(__file__).resolve().parent
D={}
def ref(n):return {'$ref':'#/$defs/'+n}
def obj(**p):return {'type':'object','additionalProperties':False,'required':list(p),'properties':p}
def arr(t,n,m=0,unique=False):return {'type':'array','items':t,'minItems':m,'maxItems':n,**({'uniqueItems':True} if unique else {})}
def tup(*t):return {'type':'array','prefixItems':list(t),'items':False,'minItems':len(t),'maxItems':len(t)}
def en(*v):return {'enum':list(v)}
def text(n,controls=False):return {'type':'string','minLength':1,'x-max-utf8':n,**({} if controls else {'pattern':r'^[^\x00-\x1f\x7f]+$'})}
D['id']=text(128);D['source']=text(256);D['text']=text(4096,True);D['count']={'type':'string','pattern':r'^(0|[1-9][0-9]{0,29})$'};D['atoms']={'type':'string','pattern':r'^(0|-?[1-9][0-9]{0,29})$'};D['digest']={'type':'string','pattern':'^[0-9a-f]{64}$'};D['time']={'type':'string','pattern':r'^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z$','format':'date-time'}
D['scope']=tup(ref('id'),ref('id'));D['family']=tup(ref('scope'),ref('id'),ref('id'),ref('id'));D['case']=tup(ref('family'),ref('source'),ref('id'));D['delivery']=tup(ref('scope'),ref('source'),ref('id'))
D['roles']=obj(provider=ref('id'),cost_originator=ref('id'),bearer=ref('id'),payer=ref('id'),beneficiary=ref('id'),recipient=ref('id'))
D['roles']['properties']['payer_delegation']=ref('digest')
D['evidence']=obj(body={'type':'string','maxLength':5464,'x-base64-bytes':4096},sha256=ref('digest'))
D['namespace']=obj(scope=ref('scope'),tag={'type':'string','pattern':'^[0-9a-f]{32}$'},gateway=ref('id'),route={'const':'full-case-sha256-mod/1'})
D['head']=obj(store=ref('id'),scope=ref('scope'),target=ref('id'),enrollment=ref('digest'),ordinal=ref('count'),segment=ref('digest'),root=ref('digest'))
D['authority']=obj(principal=ref('id'),permission=en('enroll','capacity','submit','read','decide','adjust','correct','close','replace'),document=ref('digest'),revision=ref('count'),observed_at=ref('time'),command=ref('digest'),head=ref('digest'))
D['family_terms']=obj(book=en('RETAIL','SUPPLIER'),key=ref('family'),prerequisites=arr({'type':'integer','minimum':0,'maximum':31},31,0,True),ordinary_atoms=ref('atoms'),correction_atoms=arr(ref('atoms'),32,1,True),source=ref('source'),supplier_pool=ref('id'),roles=ref('roles'),assent=ref('digest'),starts_at=ref('time'),occurs_before=ref('time'),received_by=ref('time'),accepted_by=ref('time'),correction_by=ref('time'))
D['supplier']=obj(id=ref('id'),maximum=ref('count'),consumed=ref('count'),held=ref('count'),released=ref('count'))
D['pool_authorization']=obj(direction=en('POSITIVE','NEGATIVE','ZERO'),roles=ref('roles'),assent=ref('digest'))
D['pool']=obj(id=ref('id'),funding=ref('count'),positive=ref('count'),negative=ref('count'),gross=ref('count'),authorizations=arr(ref('pool_authorization'),3,1,True))
D['resource']=obj(**{k:ref('count') for k in ['canonical_bytes','trusted_bytes','records','index_pages','index_values','workspace_bytes']})
COUNTERS=['segment','head_revision','grant','grant_registry','allocation','receipt','control','round','import','terminal','allocation_prefix','receipt_prefix','index_cardinality','writer_epoch','economic_revision','resource_revision']
D['counters']=obj(**{k:ref('count') for k in COUNTERS})
D['grant']=obj(id=ref('id'),store=ref('id'),registration=ref('id'),gateway=ref('id'),namespace=ref('namespace'),template={'const':'complete-ingress/1'},resources=ref('resource'),counters=ref('counters'),journal_head=ref('digest'),authentication=ref('digest'))
D['fact_kind']=en('ENROLLMENT','ENROLL_PREPARATION','ROUND_PREPARATION','GRANT','CLAIM','RECEIPT','ALIAS','RETURNED_UNUSED','RETIREMENT','RECONCILIATION','INSTALLATION','ORIGINAL_BASE','AUTHORITY','BEGIN','SEAL','TERMINAL')
D['proof']=obj(store=ref('id'),scope=ref('scope'),registration=ref('id'),host=ref('id'),ordinal=ref('count'),segment=ref('digest'),root=ref('digest'),fact_kind=ref('fact_kind'),full_key={'oneOf':[ref('delivery'),ref('case'),ref('id')]},body_hash=ref('digest'),bytes=ref('count'),trusted_observation_ref=ref('digest'))
D['object_origin']=obj(store=ref('id'),scope=ref('scope'),registration=ref('id'),host=ref('id'),ordinal=ref('count'))
D['object']=obj(origin=ref('object_origin'),kind=ref('fact_kind'),full_key={'oneOf':[ref('delivery'),ref('case'),ref('id')]},body={'type':'string','maxLength':349528,'x-base64-bytes':262144},body_hash=ref('digest'),bytes=ref('count'))
D['token']=obj(id=ref('id'),grant=ref('id'),gateway=ref('id'),allocation=ref('count'),category=en('ORDINARY','ADJUSTMENT'),claim=ref('digest'))
D['receipt']=obj(case=ref('case'),delivery=ref('delivery'),submission=ref('digest'),token=ref('id'),gateway=ref('id'),epoch=ref('count'),position=ref('count'),received_at=ref('time'),journal_head=ref('digest'))
D['coverage']={'oneOf':[obj(gateway=ref('id'),status={'const':'COMPLETE_GATEWAY_CUTOFF'},cutoff=ref('count'),allocation_prefix=ref('count'),receipt_high=ref('count'),receipt_prefix=ref('count'),disposition_root=ref('digest'),receipt_root=ref('digest'),observation=ref('digest')),obj(gateway=ref('id'),status={'const':'UNRECONCILED'},observation=ref('digest')),obj(gateway=ref('id'),status={'const':'UNKNOWN_GATEWAY_COVERAGE'})]}
D['submission']=obj(case=ref('case'),occurred_at=ref('time'),evidence=arr(ref('evidence'),16,0,True),sender_backfill={'type':'boolean'})
D['action']=obj(book=en('RETAIL','SUPPLIER'),case=ref('case'),revision=ref('count'),kind=en('ORDINARY','ADJUSTMENT','INVERSE','REPLACEMENT'),signed_atoms=ref('atoms'),magnitude=ref('count'),roles=ref('roles'),assent=ref('digest'))
D['entitlement_head']={'oneOf':[obj(status={'const':'UNCONSUMED'}),obj(status={'const':'CONSUMED'},case=ref('case'),revision=ref('count'),head=ref('digest'))]}
D['family_head']=obj(family=ref('family'),terms=ref('digest'),closed={'type':'boolean'},unavailable={'type':'boolean'},entitlement=ref('entitlement_head'))
D['certificate']=obj(predecessor=ref('digest'),enrollment=ref('digest'),family_heads=arr(ref('family_head'),32,1,True),families=arr(ref('family'),32,1,True),unavailable=arr(ref('family'),32,0,True),supplier_before=arr(ref('supplier'),8,0,True),supplier_after=arr(ref('supplier'),8,0,True),round=ref('count'),cutoffs=arr(ref('coverage'),4,0,True),closed_at=ref('time'))
# Atomic commands. Optional fields are deliberately absent, never null.
P={
'PREPARE_ENROLL':obj(store=ref('id'),scope=ref('scope'),registration=ref('id'),gateway=ref('id'),namespace=ref('namespace'),intent=ref('digest')),
'PREPARE_ROUND':obj(round=ref('count'),predecessor=ref('count'),gateway=ref('id'),mode={'const':'CANCELLABLE'},enrollment=ref('digest'),proof=ref('proof')),
'ENROLL':obj(preparations=arr(ref('proof'),4,1,True),registration=ref('id'),store=ref('id'),scope=ref('scope'),target=ref('id'),base_receipt=ref('digest'),base_manifest=ref('digest'),base_atoms=ref('count'),supplier_booked=ref('count'),premium_cap=ref('count'),families={**arr(ref('family_terms'),32,1,True),'x-ordered':True},gateways={**arr(ref('namespace'),4,1,True),'x-ordered':True},suppliers=arr(ref('supplier'),8,0,True),pools=arr(ref('pool'),3,0,True)),
'LOCAL_GRANT':obj(grant=ref('grant'),proof=ref('proof')),
'REGISTER_GRANT':obj(grant=ref('grant')),
'ISSUE':obj(grant=ref('id'),token=ref('token')),
'ACTIVATE':obj(token=ref('id'),gateway=ref('id')),
'RECEIVE':obj(token=ref('id'),gateway=ref('id'),epoch=ref('count'),delivery=ref('delivery'),submission=ref('submission'),received_at=ref('time')),
'RETURN_UNUSED':obj(token=ref('id'),gateway=ref('id'),claim=ref('digest')),
'IMPORT':obj(token=ref('id'),proof=ref('proof')),
'RECONCILE':obj(token=ref('id'),proof=ref('proof')),
'ADVANCE':obj(gateway=ref('id'),through=ref('count')),
'ADVANCE_RECEIPT':obj(gateway=ref('id'),through=ref('count')),
'LOCAL_TERMINAL':obj(grant=ref('id'),gateway=ref('id'),proof=ref('digest')),
'RETIRE_GRANT':obj(grant=ref('id')),
'BEGIN':obj(preparations=arr(ref('proof'),4,0,True),round=ref('count'),predecessor=ref('count'),mode=en('FINISH_ONLY','CANCELLABLE'),families=arr(ref('family'),32,1,True),gateways=arr(ref('id'),4,0,True)),
'SEAL_BEGIN':obj(round=ref('count'),gateway=ref('id'),predecessor=ref('count')),
'SEALED':obj(round=ref('count'),gateway=ref('id')),
'DRAIN':obj(round=ref('count'),gateway=ref('id')),
'READY':obj(round=ref('count')),
'CLOSE':obj(round=ref('count'),closed_at=ref('time')),
'ABORT':obj(round=ref('count')),
'INSTALL':obj(round=ref('count'),gateway=ref('id'),outcome=en('COMMITTED','ABORTED')),
'ACK_INSTALL':obj(round=ref('count'),gateway=ref('id')),
'SUPPLEMENT':obj(case=ref('case'),evidence=arr(ref('evidence'),16,1,True)),
'DECIDE':obj(case=ref('case'),verdict=en('ALLOW','DENY'),path=en('ORDINARY','ADJUSTMENT'),signed_atoms=ref('atoms'),pool=ref('id'),roles=ref('roles'),assent=ref('digest'),reason=text(256)),
'CORRECT':obj(case=ref('case'),expected_revision=ref('count'),replacement=ref('atoms'),roles=ref('roles'),assent=ref('digest')),
'REPLACE_WRITER':obj(gateway=ref('id'),old_epoch=ref('count'),new_epoch=ref('count'),journal_head=ref('digest'),fence=ref('digest')),
'EXTEND_RESOURCES':obj(host=ref('id'),resources=ref('resource')),
}
for k in ['REGISTER_GRANT','ACTIVATE','RETURN_UNUSED','LOCAL_TERMINAL','SEAL_BEGIN','DRAIN','INSTALL','ACK_INSTALL']:
 P[k]['properties']['proof']=ref('proof')
 if 'proof' not in P[k]['required']: P[k]['required'].append('proof')
for k,v in P.items():D[k.lower()]=v
D['command']={'oneOf':[obj(kind={'const':k},key=ref('delivery'),payload=ref(k.lower()),authority=ref('authority')) for k in P]}
D['round_begin']=obj(round=ref('count'),predecessor=ref('count'),mode=en('FINISH_ONLY','CANCELLABLE'),cutoffs=arr(obj(gateway=ref('id'),cutoff=ref('count')),4,0,True))
D['seal']=obj(round=ref('count'),gateway=ref('id'),cutoff=ref('count'),receipt_high=ref('count'),disposition_root=ref('digest'),receipt_root=ref('digest'))
D['effect']={'oneOf':[obj(kind={'const':k},body=ref(t)) for k,t in [('RECEIPT','receipt'),('ACTION','action'),('CLOSURE','certificate'),('ROUND_BEGIN','round_begin'),('SEAL','seal')]]}
D['result']=obj(status=en('COMMITTED','DUPLICATE','REFUSED','UNKNOWN'),code=text(64),effects=arr(ref('effect'),128),root=ref('digest'))
D['segment']=obj(host=ref('id'),profile={'const':'central-adjudication-r3/1'},ordinal=ref('count'),previous=ref('digest'),previous_root=ref('digest'),command=ref('command'),result=ref('result'),dependencies=arr(ref('digest'),128,0,True),objects=arr(ref('object'),133,0,True))
D['retry_response']=obj(receipt=ref('receipt'),knowledge=en('AUTHORITATIVE_AT_PREFIX','CACHED_VERIFIED_PREFIX','UNKNOWN'),current_lifecycle=en('UNKNOWN','ORDINARY_PENDING','ADJUSTMENT_PENDING','FINAL_ALLOW','FINAL_DENY'),central_admission=en('UNKNOWN','PRESENT_AT_PREFIX','ABSENT_AT_PREFIX'),prefix=ref('head'),coverage=arr(ref('coverage'),4,0,True))
D['retry_response']['required'].remove('prefix')
D['expected_prefix']=obj(store=ref('id'),scope=ref('scope'),registration=ref('id'),host=ref('id'),ordinal=ref('count'),segment=ref('digest'),root=ref('digest'))
D['read_budget']=obj(bytes=ref('count'),pages=ref('count'),segments=ref('count'))
D['read_cursor']=obj(expected=ref('expected_prefix'),ordinal=ref('count'),byte_offset=ref('count'),verified_root=ref('digest'),continuation=ref('digest'))
D['read_request']=obj(expected=ref('expected_prefix'),budget=ref('read_budget'))
D['read_request']['properties']['cursor']=ref('read_cursor')
D['read_response']={'oneOf':[obj(status={'const':'COMPLETE'},expected=ref('expected_prefix'),measured=ref('read_budget')),obj(status={'const':'INCOMPLETE'},cursor=ref('read_cursor'),measured=ref('read_budget')),obj(status={'const':'ERROR'},code=text(64))]}
D['comparison_policy']=obj(resolution_atoms=ref('count'))
D['comparison_request']=obj(expected=ref('expected_prefix'),policy=ref('comparison_policy'),budget=ref('read_budget'),coverage=arr(ref('coverage'),4,1,True))
D['comparison_response']={'oneOf':[obj(status={'const':'COMPARABLE'},expected=ref('expected_prefix'),actual=ref('atoms'),alternative=ref('atoms'),difference=ref('atoms'),supplier_booked=ref('count'),coverage=arr(ref('coverage'),4,1,True)),obj(status={'const':'UNSUPPORTED'},reason=text(256)),obj(status={'const':'POLICY_FAILURE'},at_case=ref('case'),reason=text(256)),obj(status={'const':'INCOMPLETE'},expected=ref('expected_prefix'),reason=text(256))]}
s={'$schema':'https://json-schema.org/draft/2020-12/schema','$id':'urn:ledgerlab:central-adjudication-r3:1','$ref':'#/$defs/segment','$defs':D,'x-counters':COUNTERS,'x-operation-limit':262144,'x-trusted-limit':2097152,'x-segment-limit':8388608}
expected=json.dumps(s,indent=2,ensure_ascii=False)+'\n'
if '--write' in sys.argv:(root/'protocol/schema.json').write_text(expected)
else:assert (root/'protocol/schema.json').read_text()==expected,'SCHEMA_DERIVATION_MISMATCH'
print(len(D),len(P))
