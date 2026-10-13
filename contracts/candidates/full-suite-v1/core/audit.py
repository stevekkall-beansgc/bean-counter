"""Read-only candidate checks; does NOT claim full Draft2020-12 validation."""
import hashlib,json,re
from pathlib import Path
HERE=Path(__file__).parent
root=json.loads((HERE/'workflow.schema.json').read_text())
def walk(v):
 if isinstance(v,dict):
  if '$ref' in v:
   assert v['$ref'].startswith('#/$defs/'),v
   assert v['$ref'][8:] in root['$defs'],v
  if v.get('type')=='object':
   assert v.get('additionalProperties') is False or v is root['$defs']['Facts']
  for x in v.values():walk(x)
 elif isinstance(v,list):
  for x in v:walk(x)
walk(root)
# Small explicit checker for keywords used by these fixtures, not general JSONSchema.
def valid(v,s):
 if '$ref' in s:return valid(v,root['$defs'][s['$ref'][8:]])
 if 'oneOf' in s:return sum(valid(v,x) for x in s['oneOf'])==1
 if 'const' in s:return v==s['const'] and type(v)==type(s['const'])
 if 'enum' in s:return v in s['enum']
 t=s.get('type')
 if t=='object':
  if not isinstance(v,dict):return False
  if any(k not in v for k in s.get('required',[])):return False
  if s.get('additionalProperties') is False and any(k not in s.get('properties',{}) for k in v):return False
  if len(v)>s.get('maxProperties',len(v)):return False
  return all(valid(x,s['properties'][k] if k in s.get('properties',{}) else s['additionalProperties']) and ('propertyNames' not in s or valid(k,s['propertyNames'])) for k,x in v.items())
 if t=='array':
  if not isinstance(v,list) or not s.get('minItems',0)<=len(v)<=s.get('maxItems',len(v)):return False
  if s.get('uniqueItems') and len({json.dumps(x,sort_keys=True) for x in v})!=len(v):return False
  if 'prefixItems' in s:return len(v)==len(s['prefixItems']) and all(valid(x,y) for x,y in zip(v,s['prefixItems']))
  return all(valid(x,s['items']) for x in v)
 if t=='string':return isinstance(v,str) and s.get('minLength',0)<=len(v)<=s.get('maxLength',len(v)) and ('pattern' not in s or re.search(s['pattern'],v) is not None)
 if t=='boolean':return type(v) is bool
 raise AssertionError(('unsupported checker keyword/type',s))
fixtures=json.loads((HERE/'shape-fixtures.json').read_text())
for case in fixtures:
 assert valid(case['value'],root['$defs'][case['definition']])==case['valid'],case['name']
bytevectors=json.loads((HERE/'byte-vectors.json').read_text())
for v in bytevectors:
 # This author fixture has ASCII object keys; independent UTF16/JCS gate remains required.
 encoded=json.dumps(v['value'],ensure_ascii=False,separators=(',',':'),sort_keys=True).encode()
 assert encoded.hex()==v['canonical_utf8_hex']
 assert hashlib.sha256(v['frame'].encode()+b'\0'+encoded).hexdigest()==v['sha256']
print(f'candidate local-ref/closed-shape audit: {len(root["$defs"])} definitions; {len(fixtures)} shape cases; {len(bytevectors)} ASCII byte/hash vectors')
print('NOT full JSON Schema metaschema validation, semantic compiler acceptance, independent freeze, or runtime proof')
