#!/usr/bin/env python3
"""Small explicit canonical and independent arithmetic boundaries."""
from fractions import Fraction
import hashlib,json
from pathlib import Path
import validate as v
HERE=Path(__file__).resolve().parent

def away(q):
 n,d=q.numerator,q.denominator;a,r=divmod(abs(n),d);return (-1 if n<0 else 1)*(a+int(2*r>=d))
def build():
 arithmetic=[]
 for base,rate,n,d,rounded in [(10000,'12',1200,1,1200),(10000,'15',1500,1,1500),(10000,'-1.5',-150,1,-150),(10005,'10',2001,2,1001),(10005,'-10',-2001,2,-1001)]:
  q=Fraction(base)*Fraction(rate)/100;assert q==Fraction(n,d) and away(q)==rounded;arithmetic.append(dict(base_atoms=str(base),rate_percent=rate,numerator=str(n),denominator=str(d),rounded=str(rounded),inverse=str(-rounded)))
 routes=[];scope=['synthetic','sandbox'];base=[scope,'agreement','family','target'];found={}
 for i in range(1000):
  case=[base,'source',str(i)];h=v.digest('route',case);owner=int(h,16)%4
  if owner not in found:found[owner]=dict(case=case,hash=h,owner=owner)
  if len(found)==4:break
 routes=[found[i] for i in range(4)]
 byte_values=[{}, {'\ue000':1,'😀':2},{'quoted':'"\\','control':'\b\t\n\f\r\u0001'},[['x','y'],'source','external'],{'é':'e\u0301'}]
 return dict(format='r3-boundary-vectors/1',arithmetic=arithmetic,routes=routes,canonical=[dict(value=x,utf8_hex=v.canonical(x).hex(),sha256=hashlib.sha256(v.canonical(x)).hexdigest()) for x in byte_values],gross=[dict(positive='100',negative='150',gross='250',net='-50',cap=str(cap),allowed=cap>=250) for cap in [249,250,251]],limits=dict(operation=262144,trusted=2097152,segment=8388608,evidence=4096,counter=str(v.M)))
if __name__=='__main__':
 expected=json.dumps(build(),indent=2,ensure_ascii=False)+'\n'
 import sys
 if '--write' in sys.argv:(HERE/'boundary-vectors.json').write_text(expected)
 else:assert (HERE/'boundary-vectors.json').read_text()==expected
