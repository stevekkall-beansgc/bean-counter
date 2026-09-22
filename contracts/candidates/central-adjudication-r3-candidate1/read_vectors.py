"""Closed expected-prefix/read/comparison and seal-fold author vectors."""
import copy,json,sys
import validate as v
from fixtures import HERE,customer_story
from reads import Reader,expected,compare
from seal_reader import SealReader

def build():
 b=customer_story();current=expected(b.l);historical=copy.deepcopy(current);first=next(s for s in b.l.s['segments'] if s['host']=='center');historical.update(ordinal=first['ordinal'],segment=v.digest('segment',first),root=first['result']['root']);large={'bytes':str(v.M),'pages':str(v.M),'segments':str(v.M)};unknown=[dict(gateway=g,status='UNKNOWN_GATEWAY_COVERAGE') for g in sorted(b.l.s['gateways'])];rows=[]
 for name,prefix,budget in [('current',current,large),('historical',historical,large),('partial',current,{'bytes':'1','pages':'1','segments':'1'})]:
  request={'expected':prefix,'budget':budget};rows.append(dict(name=name,kind='read',request=request,response=Reader(b.l).read(request)))
 for name,replacement,budget,coverage in [('comparison-unknown','1500',large,unknown),('comparison-failure','6000',large,unknown),('comparison-incomplete','1500',{'bytes':'1','pages':'1','segments':'1'},unknown),('comparison-historical-coverage','1500',large,b.l.s['certificates'][-1]['cutoffs'])]:
  request=dict(expected=current,policy={'resolution_atoms':replacement},budget=budget,coverage=coverage);rows.append(dict(name=name,kind='compare',request=request,response=compare(b.l,request)))
 bad=copy.deepcopy(rows[-1]['request']);bad['coverage'][0]['receipt_high']='999';rows.append(dict(name='known-observation-wrong-coverage',kind='compare',request=bad,error='COVERAGE_EXACT_PREFIX'))
 bad=copy.deepcopy(rows[0]['request']);bad['expected']['root']='f'*64;rows.append(dict(name='wrong-expected-root',kind='read',request=bad,error='EXPECTED_PREFIX'))
 bad=copy.deepcopy(rows[3]['request']);bad['policy']['invent_decision']=True;rows.append(dict(name='unsupported-policy-field',kind='compare',request=bad,error='UNKNOWN_FIELD'))
 for row in rows:
  call=lambda:Reader(b.l).read(row['request']) if row['kind']=='read' else compare(b.l,row['request'])
  if 'error'in row:
   try:call()
   except ValueError:pass
   else:raise AssertionError(row['name'])
  else:assert call()==row['response']
 trace=v.strict((HERE/'vectors/seal-scan.json').read_bytes());ledger=v.Ledger(trace['initial']);through=0
 for c in trace['commands']:
  ledger.execute(c);through+=1
  if c['kind']=='SEAL_BEGIN':break
 scan=SealReader(ledger,'g0','1');partial=scan.read(1,1);scan.abort();complete=SealReader(ledger,'g0','1').read(v.M,v.M)
 return dict(format='r3-read-vectors/1',source_trace='customer-trace.json',operations=rows,seal=dict(source_trace='vectors/seal-scan.json',through=str(through),gateway='g0',round='1',partial_budget={'bytes':'1','pages':'1'},partial=partial,complete=complete,abort_error='SCAN_ABORTED',resume_budget={'bytes':'7','pages':'2'}))
def main():
 raw=json.dumps(build(),indent=2)+'\n';path=HERE/'read-vectors.json'
 if '--write' in sys.argv:path.write_text(raw)
 else:assert path.read_text()==raw,'READ_VECTOR_DRIFT';print('read/seal vectors verified')
if __name__=='__main__':main()
