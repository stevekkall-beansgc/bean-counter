"""Live legacy-proposal regression inputs and independent per-attempt results.

These are derived synthetic cases, not rewrites of existing fixture expectations.
"""
from copy import deepcopy
import json
from pathlib import Path
from reference import Reference

FIX = Path(__file__).resolve().parents[2] / 'fixtures/phase2-proposed-v1'

def fixture(name):
    return json.loads((FIX / (name + '.json')).read_text())

def make(name, f, steps, change=None):
    model = Reference(f['config'])
    output=[]
    for i, (alias, received) in enumerate(steps):
        config = None
        if change is not None and i == len(steps)-1:
            change(model.config)
            config = deepcopy(model.config)
        result=model.submit(f['events'][alias], received)
        output.append(dict(event=alias, received=received, config=config, result=result,
                           journal=deepcopy(model.journal)))
    return dict(name=name, config=f['config'], events=f['events'], steps=output)

f=fixture('later-acquisition')
cases=[make('receipt-before-occurrence', f, [('g','50'),('p','50'),('a','39')])]
f=fixture('later-acquisition');f['events']['p']['status']='failed'
cases.append(make('failed-publication',f,[('g','50'),('p','50'),('a','50')]))
f=fixture('later-acquisition');f['events']['r']['targets']=['p']
cases.append(make('reversed-publication',f,[('g','50'),('p','50'),('r','90'),('a','90')]))
f=fixture('multi-uncapped');f['events']['p']['links']=[dict(relation='published_as',event='g',source=f['events']['g']['source'])]
cases.append(make('unrelated-supplier-completion',f,[('g','50'),('o','50'),('p','50'),('a','50')]))
def change(config):
    config['premium_atoms']='9999'
    config['suppliers'][0]['correction_sources']=[]
    config['suppliers'][0]['roles']['recipient']='changed-today'
f=fixture('multi-capped')
cases.append(make('reversal-ignores-current-terms',f,[('g','50'),('o','50'),('p','50'),('a','50'),('r','90')],change))
print(json.dumps(cases,separators=(',',':')))
