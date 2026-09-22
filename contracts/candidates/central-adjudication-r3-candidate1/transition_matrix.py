"""Before/after canonical commit cuts and exact retry for every durable kind."""
import copy,json
import validate as v
from fixtures import HERE

def main():
 seen={};required={s['properties']['kind']['const'] for s in v.SCHEMA['$defs']['command']['oneOf']}
 paths=['customer-trace.json']+['vectors/'+n+'.json' for n in ['grant-retirement','terminal-race-abort','writer-epoch','independent-host-identity','supplement-control','mixed257']]
 for path in paths:
  trace=v.strict((HERE/path).read_bytes());ledger=v.Ledger(trace['initial'])
  for index,command in enumerate(trace['commands']):
   result=ledger.execute(command)
   if result['status']!='COMMITTED':continue
   kind=command['kind']
   if kind in seen:continue
   before=copy.deepcopy({k:z for k,z in ledger.s.items() if k not in {'duplicates','refused'}});read=copy.deepcopy(ledger.read_index);scan=copy.deepcopy(ledger.seal_index);retry=copy.deepcopy(command);retry['authority']['permission']='read';retry['authority']['head']=v.ZERO;observation=v.digest('authority',retry['authority']);ledger.initial['authority_observations'].append(observation)
   for attempt in range(2):
    response=ledger.execute(retry);assert response['status']=='DUPLICATE' and response['effects']==result['effects'];assert before=={k:z for k,z in ledger.s.items() if k not in {'duplicates','refused'}};assert ledger.read_index==read and ledger.seal_index==scan
   seen[kind]=dict(trace=path,through=index+1,host=ledger.host(command),root=result['root'],saved_retries=2)
 assert set(seen)==required,sorted(required-set(seen));print(json.dumps(dict(status='canonical-atomic-model-only',kinds=len(seen),cases=seen,actual_unknown_commit_crash_ack_proofs='PENDING'),indent=2))
if __name__=='__main__':main()
