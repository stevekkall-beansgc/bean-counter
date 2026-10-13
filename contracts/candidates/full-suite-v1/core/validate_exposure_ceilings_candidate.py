#!/usr/bin/env python3
"""Literal author codec vectors; no authority/pricing implementation imported."""
import json,re

def encode(x):
 return json.dumps(x,sort_keys=True,separators=(',',':')).encode()
def norm(xs):
 if not isinstance(xs,list) or len(xs)>16:raise ValueError('INVALID_SHAPE')
 seen=set();out=[]
 for x in xs:
  if not isinstance(x,dict) or set(x)!={'binding_slot','maximum'}:raise ValueError('INVALID_SHAPE')
  k=x['binding_slot'];a=x['maximum']
  if k in seen:raise ValueError('INVALID_SHAPE')
  seen.add(k)
  if not isinstance(a,dict) or set(a)!={'currency','scale','atoms'} or a['currency']!='USD' or a['scale']!=18 or not isinstance(a['atoms'],str) or re.fullmatch(r'0|[1-9][0-9]{0,29}',a['atoms']) is None:raise ValueError('INVALID_SCALAR')
  out.append(x)
 return sorted(out,key=encode)
def entry(slot,atoms):return {'binding_slot':slot,'maximum':{'currency':'USD','scale':18,'atoms':atoms}}
a=entry('fixture:a','10');b=entry('fixture:b','10')
cases=[([],[]),([b,a],[a,b]),([a,a],'INVALID_SHAPE'),([a,entry('fixture:a','11')],'INVALID_SHAPE'),([entry('fixture:a','-1')],'INVALID_SCALAR'),([entry('fixture:a','01')],'INVALID_SCALAR'),([entry('fixture:a','9'*30)],[entry('fixture:a','9'*30)]),([entry('fixture:a','1'+'0'*30)],'INVALID_SCALAR')]
for x,expected in cases:
 try:actual=norm(x)
 except ValueError as e:assert str(e)==expected
 else:assert actual==expected
print('8 exposure-ceiling author codec vectors PASS; authority/implementation unqualified')
