#!/usr/bin/env python3
"""Read-only author checks for UNFROZEN PUB-TRANSPORT-01; not runtime proof."""
import base64,copy,hashlib,json,pathlib
from jsonschema import Draft202012Validator
P=pathlib.Path(__file__).resolve().parent
S=json.loads((P/'publication-transport.schema.json').read_text());Draft202012Validator.check_schema(S)
V=Draft202012Validator(S)
F=json.loads((P/'publication-transport-fixture-input.json').read_text())
def j(x):return json.dumps(x,ensure_ascii=False,sort_keys=True,separators=(',',':')).encode()
def h(d,x):return 'sha256:'+hashlib.sha256(('bean-counter/'+d+'/1\0').encode()+j(x)).hexdigest()
def fragment_check(x):
 n=int(x['document_bytes']);part=int(x['part']);b=base64.b64decode(x['bytes_base64'],validate=True)
 assert base64.b64encode(b).decode()==x['bytes_base64'] and 1<=n<=262144
 assert part in range((n+131071)//131072)
 assert len(b)==min(131072,n-part*131072)
 return b
requests=F['requests'];count=0
for x in requests:
 V.validate(x);assert len(j(x))<=262144;count+=1
V.validate(F['canonical_read']);count+=1
parts=[x['payload'] for x in requests if x['payload']['schema']=='ledger-workflow-definition-fragment/1'];parts.sort(key=lambda x:int(x['part']))
raw=b''.join(map(fragment_check,parts));assert len(raw)==int(parts[0]['document_bytes'])
assert hashlib.sha256(raw).hexdigest()==parts[0]['canonical_document_sha256'];body=json.loads(raw);assert j(body)==raw
source_schema={**S,'oneOf':[{'$ref':'#/$defs/SourceDocument'}]};Draft202012Validator(source_schema).validate(body);count+=1
refs=[]
for x in parts:
 key=requests[0]['key'];b={**x,'schema':'ledger-workflow-definition-fragment-record/1','logical_store_id':key['logical_store_id'],'scope':key['scope'],'source':key['source']}
 z=h('workflow-document',[b['schema'],b]);refs.append({'kind':'workflow-definition-fragment','id':'wdoc_'+z[7:],'digest':z})
assert sorted(refs,key=j)==requests[-1]['payload']['fragment_refs'];count+=1
for n in [131071,131072,131073,262144]:
 for part in range((n+131071)//131072):
  x={**parts[0],'document_bytes':str(n),'part':str(part),'bytes_base64':base64.b64encode(b'a'*min(131072,n-part*131072)).decode()};fragment_check(x);count+=1
for mutate in [lambda x:x.update(document_bytes='262145'),lambda x:x.update(part='1',document_bytes='1'),lambda x:x.update(bytes_base64='YQ==')]:
 x=copy.deepcopy(parts[0]);mutate(x)
 try:fragment_check(x)
 except (AssertionError,ValueError):count+=1
 else:raise AssertionError('bad fragment accepted')
for n,v in [('unknown field',{'unexpected':True}),('zero bytes',{'document_bytes':'0'}),('invalid part',{'part':'2'})]:
 x=copy.deepcopy(requests[1]);x['payload'].update(v);assert list(V.iter_errors(x)),n;count+=1
maxbody=4194304;maxwire=4*((maxbody+2)//3)+2048;assert maxwire<8388608;count+=1
print(json.dumps({'status':'AUTHOR_CONTRACT_CHECKS_PASS_NOT_FREEZE','checks':count,'reassembled_bytes':len(raw),'request_bytes':F['wire_input_bytes'],'maximum_canonical_read_wire_upper_bound':maxwire},sort_keys=True))
