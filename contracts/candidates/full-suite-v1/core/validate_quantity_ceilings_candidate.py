#!/usr/bin/env python3
"""Author reference vectors only; no product imports, auth or money evaluation."""
import json,re
from fractions import Fraction
from pathlib import Path
P=Path(__file__).resolve().parent
f=json.loads((P/'quantity-ceilings-fixture-input.json').read_text())
def canonical(x):
 if isinstance(x,dict):return '{'+','.join(canonical(k)+':'+canonical(x[k]) for k in sorted(x,key=lambda k:k.encode('utf-16-be')))+'}'
 if isinstance(x,list):return '['+','.join(map(canonical,x))+']'
 return json.dumps(x,separators=(',',':'),ensure_ascii=False)
def error(code):raise ValueError(code)
def decimal(raw):
 if not isinstance(raw,str) or len(raw.encode())>64 or not re.fullmatch(r'[0-9]+(?:\.[0-9]+)?',raw):error('INVALID_SCALAR')
 a,_,b=raw.partition('.')
 if len(b)>18:error('INVALID_SCALAR')
 a=a.lstrip('0') or '0';b=b.rstrip('0')
 if len((a+b).lstrip('0'))>30:error('INVALID_SCALAR')
 return a+('.'+b if b else '')
def normalize(entries):
 if not isinstance(entries,list) or len(entries)>64:error('INVALID_SHAPE')
 seen=set();out=[]
 for x in entries:
  if set(x)!={'activity','field','unit','maximum'}:error('INVALID_SHAPE')
  x=dict(x);x['maximum']=decimal(x['maximum']);key=(x['activity'],x['field'])
  if key in seen:error('INVALID_SHAPE')
  seen.add(key);out.append(x)
 out.sort(key=lambda x:canonical(x).encode())
 return out
def resolve(entries):
 for x in entries:
  field=f['activity_fields'].get(x['activity'],{}).get(x['field'])
  if field is None or not field['required'] or field['kind']not in ['integer','decimal'] or x['unit']!=field.get('unit') or x['unit']not in f['original_units']:error('INVALID_REFERENCE')
  value=Fraction(x['maximum']);scale=len(x['maximum'].partition('.')[2])
  if field['kind']=='integer' and value.denominator!=1:error('INVALID_SCALAR')
  if field['kind']=='decimal' and scale>field['max_scale']:error('INVALID_SCALAR')
  if not Fraction(field['min'])<=value<=Fraction(field['max']):error('INVALID_SCALAR')
 return entries
for case in f['cases']:
 try:out=resolve(normalize(case['entries']))
 except ValueError as ex:assert str(ex)==case.get('error'),case['name']
 else:assert out==case.get('normalized'),case['name']
print(len(f['cases']),'quantity-ceiling normalization/semantic vectors PASS; authority and product implementation UNRUN')
