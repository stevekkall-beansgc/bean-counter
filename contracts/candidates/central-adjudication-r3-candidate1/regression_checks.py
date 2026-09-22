#!/usr/bin/env python3
"""Derive each published regression checkpoint from its own serialized prefix."""
import copy,json
from pathlib import Path
import validate as v
HERE=Path(__file__).resolve().parent

def verify(row):
 assert set(row)=={'trace','through','round','suppliers','certificate'},'CHECKPOINT_SHAPE'
 path=HERE/row['trace'];assert path.parent==HERE/'vectors' and path.suffix=='.json','CHECKPOINT_PATH'
 trace=v.strict(path.read_bytes());through=row['through'];assert type(through)is int and 0<through<=len(trace['commands']),'CHECKPOINT_PREFIX'
 assert trace['commands'][through-1]['kind']=='CLOSE','CHECKPOINT_CLOSE'
 trace['commands']=trace['commands'][:through];result=v.replay(trace);cert=result['snapshot']['certificates'][-1]
 assert cert['round']==row['round'],'CHECKPOINT_ROUND'
 assert result['snapshot']['suppliers']==row['suppliers'],'CHECKPOINT_SUPPLIERS'
 assert v.digest('closure',cert)==row['certificate'],'CHECKPOINT_CERTIFICATE'

def main():
 data=v.strict((HERE/'regression-checkpoints.json').read_bytes());assert set(data)=={'format','close_prefixes'} and data['format']=='r3-regression-checkpoints/1','CHECKPOINT_ENVELOPE'
 rows=data['close_prefixes'];assert len(rows)==4 and len({(r['trace'],r['through']) for r in rows})==4,'CHECKPOINT_SET'
 for row in rows:verify(row)
 bad=copy.deepcopy(rows[0]);bad['suppliers'][0].update(held='0',released='170')
 try:verify(bad)
 except AssertionError as error:assert str(error)=='CHECKPOINT_SUPPLIERS'
 else:raise AssertionError('stale future supplier state accepted')
 print(json.dumps(dict(passed=len(rows)+1,failed=0,ignored=0,checkpoints=len(rows),stale_future_state_rejected=True)))
if __name__=='__main__':main()
