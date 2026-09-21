"""Read-only Python byte, schema, semantic-reference and complete-journal audit."""
import copy,hashlib,json,re,subprocess,sys
from fractions import Fraction
from datetime import datetime, timezone
from pathlib import Path
import jsonschema
from referencing import Registry,Resource
from canonical_records import ROOT,build,artifacts,canonical,hash_value

def strict_json(raw):
    def pairs(items):
        d={}
        for k,v in items:
            if k in d:raise ValueError('duplicate key')
            d[k]=v
        return d
    def integer(s):
        if s=='-0' or abs(int(s))>9007199254740991:raise ValueError('bad integer')
        return int(s)
    def no_number(s):raise ValueError('unsupported numeric token')
    return json.loads(raw,object_pairs_hook=pairs,parse_int=integer,parse_float=no_number,parse_constant=no_number)

def semantic_values(v):
    if isinstance(v,dict):
        if set(v)=={'numerator','denominator'}:
            n=int(v['numerator']);d=int(v['denominator']);assert d>0 and abs(n).bit_length()<=512 and d.bit_length()<=512
            f=Fraction(n,d);assert (str(f.numerator),str(f.denominator))==(v['numerator'],v['denominator'])
        if v.get('kind')=='decimal':
            f=Fraction(v['value']);assert f==Fraction(int(v['exact']['numerator']),int(v['exact']['denominator']))
        for k,x in v.items():
            if k.endswith('_at') and isinstance(x,str):
                parsed=datetime.fromisoformat(x.replace('Z','+00:00'));assert parsed.strftime('%Y-%m-%dT%H:%M:%S.%fZ')==x
            if k in ['revision','from_revision','to_revision','from_event_count','to_event_count'] and isinstance(x,str):assert 0<=int(x)<=9223372036854775807
            if k=='atoms':assert abs(int(x))<=10**30-1
            if k in ['quantity','maximum_quantity','fixed','percent','unit_price'] and isinstance(x,str):
                f=Fraction(x);assert f>=0
            semantic_values(x)
    elif isinstance(v,list):
        for x in v:semantic_values(x)

def verify_journal(rows,seeds):
    order=lambda r:(r['kind'].encode(),canonical(r['id']).encode())
    assert rows==sorted(rows,key=order)
    allrows=rows+seeds
    lookup={(r['kind'],canonical(r['id'])):r for r in allrows};assert len(lookup)==len(allrows)
    def select(kind):return [r for r in rows if r['kind']==kind]
    def get(kind,id):return lookup[kind,canonical(id)]
    def digest(domain,value):return 'sha256:'+hash_value(domain,value)
    for r in allrows:
        b=r['body'];kind=r['kind'];semantic_values(b)
        assert r['scope']==['demo','sandbox']
        if 'scope' in b:assert b['scope']==r['scope']
        if kind=='document':
            h=hash_value('document',[r['document_type'],1,b]);assert r['id']=='doc_'+h;assert r['content_hash']=='sha256:'+h
        elif kind=='event':
            assert r['id']=='ev_'+hash_value('event',r['scope']+[b['source'],b['id']]);assert r['content_hash']==digest('event-content',b)
        elif kind=='decision-manifest':assert r['content_hash']==digest('decision-content',b)
        else:assert r['content_hash']==digest('record-content',[kind,1,b])
        if kind not in ['document','event','chain-revision','action-source','action-dependency','delivery-key']:assert r['id']==b['id']
    e=select('event')[0];eb=e['body'];eid=e['id'];scope=e['scope']
    d=select('decision-manifest')[0];m=d['body'];receipt=select('receipt')[0]['body']
    assert d['id']=='dc_'+hash_value('decision',[eid]);assert receipt['id']=='rc_'+hash_value('receipt',[eid])
    assert receipt['event_id']==eid and receipt['decision_id']==d['id'] and receipt['content_hash']==e['content_hash'] and receipt['decision_hash']==d['content_hash']
    expected=[{k:r[k] for k in ('kind','id','content_hash')} for r in sorted([r for r in allrows if r['kind'] not in ['decision-manifest','receipt']],key=order)]
    assert m['members']==expected and len(expected)==29
    assert m['revision']==receipt['revision']=='1';assert m['chain_id']==receipt['chain_id']==eb['chain']
    associations=select('snapshot-ref');assert len(associations)==7
    for r in associations:
        b=r['body'];assert b['event_id']==eid;assert r['id']=='sr_'+hash_value('snapshot-ref',[eid,b['purpose'],b['document_id']]);get('document',b['document_id'])
    purpose={r['body']['purpose']:r['body']['document_id'] for r in associations}
    assert set(purpose)=={'policy','roles','assent','source_grant','binding','chain_context','decision_snapshot'}
    snapshot=get('document',purpose['decision_snapshot'])['body'];binding=get('document',purpose['binding'])['body']
    assert snapshot['documents']==sorted([{'purpose':k,'document_id':v} for k,v in purpose.items() if k!='decision_snapshot'],key=lambda x:canonical(x).encode())
    assert snapshot['context']=={k:v for k,v in get('document',purpose['chain_context'])['body'].items() if k!='schema'}
    assert binding['context']==purpose['chain_context'] and binding['policy']==purpose['policy'] and binding['assent']==purpose['assent'] and binding['roles']==purpose['roles']
    grant=get('document',purpose['source_grant'])['body'];authority=snapshot['authority'][0]
    assert authority['principal_id']==grant['principal_id'] and authority['source']==grant['source']==eb['source'] and authority['grant_id']==grant['id'] and authority['grant_document']==purpose['source_grant'] and authority['revision']=='1' and authority['active']
    claim=select('claim')[0];cb=claim['body']
    assert cb['event_id']==eid and cb['source']==eb['source'] and cb['operation_id']==eb['operation_id']
    assert claim['id']=='cl_'+hash_value('claim',[scope,cb['source'],cb['operation_id'],cb['kind'],cb['token']])
    claimfacts={'schema':'ledger-claim-facts/1',**{k:eb[k] for k in ['type','chain','customer','status','quantity','unit','links','evidence']}}
    for k in ['binding_id','invocation_id','occurred_at','corrects']:
        if k in eb:claimfacts[k]=eb[k]
    assert cb['facts_hash']==digest('claim-facts',claimfacts)
    actions=select('action');assert len(actions)==2
    roles={k:v for k,v in get('document',purpose['roles'])['body'].items() if k!='schema'}
    for a in actions:
        b=a['body'];f=get('effect',b['effect_id'])['body']
        assert f['id']=='ef_'+hash_value('effect',[scope,f['agreement_id'],f['component'],f['claim_id'],f['match_key'],f['namespace']])
        assert a['id']=='ac_'+hash_value('action',[f['id']]);assert f['action_id']==a['id'] and f['claim_id']==claim['id']
        assert b['event_id']==eid and b['decision_id']==d['id'] and b['snapshot_doc']==purpose['decision_snapshot'] and b['roles']==roles and b['roles_doc']==purpose['roles']
        assert b['binding_id']==binding['id'] and f['agreement_id']==binding['agreement_id'] and b['component']==f['component']
        assert b['obligation_id']=='ob_'+hash_value('obligation',[scope,binding['agreement_id'],b['book'],b['amount']['currency'],b['amount']['scale'],roles])
        ff={'schema':'ledger-effect-facts/1',**{k:f[k] for k in ['scope','agreement_id','component','claim_id','match_key','namespace']},**{k:b[k] for k in ['kind','book','amount','roles','sources','links','inputs']}}
        for k in ['reverses','allocation_parent']:
            if k in b:ff[k]=b[k]
        assert f['facts_hash']==digest('effect-facts',ff)
    sourceids=sorted([canonical([scope,a['id'],source]) for a in actions for source in a['body']['sources']])
    depids=sorted([canonical([scope,a['id'],prior]) for a in actions for prior in a['body']['inputs']])
    assert sourceids==sorted(canonical(r['id']) for r in select('action-source'));assert depids==sorted(canonical(r['id']) for r in select('action-dependency'))
    for r in select('action-source'):assert r['id']==[scope,r['body']['action_id'],r['body']['event_id']]
    for r in select('action-dependency'):assert r['id']==[scope,r['body']['action_id'],r['body']['input_action_id']]
    steps=sorted(select('explanation'),key=lambda r:r['body']['ordinal']);assert [r['body']['ordinal'] for r in steps]==[0,1]
    assert m['explanation_ids']==[r['id'] for r in steps]
    for r in steps:
        b=r['body'];assert r['id']=='xp_'+hash_value('explanation',[eid,b['ordinal']]);assert b['event_id']==eid
        assert b['input_refs']==sorted(b['input_refs'])
        for id in b['input_refs']:get('action' if id.startswith('ac_') else 'document',id)
        assert b['rounded_atoms']==get('action',b['action_ids'][0])['body']['amount']['atoms']
    assert steps[0]['body']['unrounded_atoms']=={'numerator':'100','denominator':'1'}
    assert Fraction(steps[1]['body']['unrounded_atoms']['numerator']) == -Fraction(steps[1]['body']['basis']['numerator'])*Fraction(20,100)
    intention=select('intention')[0];ib=intention['body'];payload=ib['payload'];assert sum(int(a['body']['amount']['atoms']) for a in actions)==int(ib['amount']['atoms'])==80
    assert ib['idempotency_key']==intention['id']=='in_'+hash_value('intention',[scope,ib['destination_id'],ib['obligation_id'],ib['action_ids']])
    assert ib['action_ids']==receipt['action_ids']==sorted(a['id'] for a in actions);assert receipt['intention_ids']==[ib['id']]
    assert payload['amount']==ib['amount'] and payload['roles']==roles and payload['obligation_id']==ib['obligation_id']
    assert payload['actions']==[{'action_id':a['id'],**{k:a['body'][k] for k in ['kind','component','amount']}} for a in sorted(actions,key=lambda a:a['id'])]
    ct=select('control-transition')[0]['body'];assert ct['id']=='ct_'+hash_value('control-transition',[scope,ct['control_kind'],ct['control_id'],ct['to_revision']])
    assert ct['from_revision']==ct['from_event_count']=='0' and ct['to_revision']==ct['to_event_count']=='1' and ct['document_id']==purpose['decision_snapshot']
    cr=select('chain-revision')[0];assert cr['id']==[scope,eb['chain'],'1'] and cr['body']['event_id']==eid and cr['body']['decision_id']==d['id']
    dk=select('delivery-key')[0];assert dk['id']==[scope,eb['source'],eb['id']] and dk['body']['canonical_event_id']==eid and dk['body']['ingress']==eb and dk['body']['ingress_hash']==digest('ingress',eb)

def audit(root=ROOT):
    root=Path(root).resolve();fixture=root/'fixtures/journals/first-slice';x=build();expected=artifacts(x)
    assert sorted(p.name for p in fixture.iterdir())==sorted(expected)
    for name,raw in expected.items():assert (fixture/name).read_bytes()==raw,('Python byte mismatch',name)
    schemasdir=root/'contracts/schemas/v1';schema=strict_json((schemasdir/'canonical-records.schema.json').read_bytes());jsonschema.Draft202012Validator.check_schema(schema)
    validator=jsonschema.Draft202012Validator(schema)
    rows=[strict_json(line) for line in (fixture/'accepted-records.jsonl').read_bytes().splitlines()]
    seeds=[strict_json(line) for line in (fixture/'seed-documents.jsonl').read_bytes().splitlines()]
    seedrows=[strict_json(line) for line in (fixture/'seed-records.jsonl').read_bytes().splitlines()]
    for r in rows+seeds+seedrows:validator.validate(r)
    verify_journal(rows,seeds)
    def val(n,v):jsonschema.Draft202012Validator({**schema,'$ref':'#/$defs/'+n}).validate(v);semantic_values(v)
    for name,n in [('claim-facts.json','claim-facts'),('effect-facts-1.json','effect-facts'),('effect-facts-2.json','effect-facts')]:val(n,strict_json((fixture/name).read_bytes()))
    markdown=(root/'docs/design/CANONICAL-RECORDS-V1.md').read_text()
    blocks=[strict_json(b) for b in re.findall(r'```json\n(.*?)\n```',markdown,re.S)]
    assert blocks[-1]==schema
    for r in rows+seeds:assert r['body'] in blocks
    for r in seedrows:assert r in blocks
    for v in x['vectors']:assert v['sha256'] in markdown
    resources=[]
    for path in schemasdir.glob('*.schema.json'):
        s=strict_json(path.read_bytes());jsonschema.Draft202012Validator.check_schema(s);resources.append((path.as_uri(),Resource.from_contents(s)));resources.append((s['$id'],Resource.from_contents(s)))
    registry=Registry().with_resources(resources)
    examples={'snapshot':x['labels']['S']['body'],'explanation':x['labels']['XP1']['body'],'explanation-input':x['labels']['XP1']['body']['inputs'][1],'effect-facts':next(v['value'] for v in x['vectors'] if v['name']=='effect-facts.1'),'decision-manifest':x['labels']['D']['body'],'receipt':x['labels']['R']['body'],'action':x['labels']['A1']['body'],'intention':x['labels']['I']['body']}
    for name,v in examples.items():
        wrapper=strict_json((schemasdir/(name+'.schema.json')).read_bytes())
        # Resolve the unmodified convenience contract through the bundled URN registry.
        jsonschema.Draft202012Validator(wrapper,registry=registry).validate(v)
    tagged_positive=[]
    for kind,value in [('decimal','20'),('money',{'currency':'USD','scale':2,'atoms':'100'}),('boolean',True),('source_id','urn:demo:app'),('binding_field','enterprise'),('document_ref',x['labels']['P']['id']),('action_ref',x['labels']['A1']['id'])]:
        sample={'kind':kind,'name':'test', 'value':value}
        if kind=='decimal':sample['exact']={'numerator':'20','denominator':'1'}
        val('explanation-input',sample);tagged_positive.append(kind)
    negative=[]
    def reject(name,fn):
        try:fn()
        except (AssertionError,ValueError,jsonschema.ValidationError,KeyError):negative.append(name)
        else:raise AssertionError('accepted negative '+name)
    def mutate_example(n,change):
        v=copy.deepcopy(examples[n]);change(v);val(n,v)
    reject('unknown snapshot field',lambda:mutate_example('snapshot',lambda v:v.update(extra=True)))
    reject('invalid Gregorian time',lambda:val('source-grant',{**x['labels']['G']['body'],'starts_at':'2026-02-30T14:00:00.000000Z'}))
    reject('unknown nested context field',lambda:mutate_example('snapshot',lambda v:v['context'].update(extra=True)))
    reject('null authority revision',lambda:mutate_example('snapshot',lambda v:v['authority'][0].update(revision=None)))
    reject('unsigned revision overflow',lambda:mutate_example('snapshot',lambda v:v['authority'][0].update(revision='9223372036854775808')))
    reject('missing named basis',lambda:mutate_example('explanation',lambda v:v.pop('basis_name')))
    reject('unknown explanation tag',lambda:mutate_example('explanation-input',lambda v:v.update(kind='arbitrary')))
    reject('decimal exact ratio mismatch',lambda:mutate_example('explanation-input',lambda v:v['exact'].update(numerator='21')))
    reject('nonreduced ratio',lambda:mutate_example('explanation',lambda v:v.update(basis={'numerator':'200','denominator':'2'})))
    reject('negative zero',lambda:mutate_example('explanation',lambda v:v['basis'].update(numerator='-0')))
    reject('numeric atom token',lambda:mutate_example('action',lambda v:v['amount'].update(atoms=100)))
    reject('unknown nested payload field',lambda:mutate_example('intention',lambda v:v['payload'].update(delivery_state='held')))
    reject('composite ID converted to string',lambda:mutate_example('decision-manifest',lambda v:next(m for m in v['members'] if isinstance(m['id'],list)).update(id='ambiguous|composite')))
    def changed_journal(change):
        rs=copy.deepcopy(rows);change(rs);verify_journal(rs,seeds)
    reject('missing edge envelope',lambda:changed_journal(lambda rs:rs.remove(next(r for r in rs if r['kind']=='action-dependency'))))
    reject('manifest incomplete',lambda:changed_journal(lambda rs:next(r for r in rs if r['kind']=='decision-manifest')['body']['members'].pop()))
    reject('manifest wrong order',lambda:changed_journal(lambda rs:next(r for r in rs if r['kind']=='decision-manifest')['body']['members'].reverse()))
    reject('same-ID body tamper',lambda:changed_journal(lambda rs:next(r for r in rs if r['kind']=='action')['body']['amount'].update(atoms='101')))
    reject('duplicate record identity',lambda:changed_journal(lambda rs:rs.append(copy.deepcopy(rs[0]))))
    def reseal(rs):
        # Recompute all hashes after mutation to exercise semantic checks beyond digest equality.
        for r in rs:
            if r['kind'] not in ['document','event','decision-manifest','receipt']:r['content_hash']='sha256:'+hash_value('record-content',[r['kind'],1,r['body']])
        lookup={(r['kind'],canonical(r['id'])):r['content_hash'] for r in rs+seeds}
        manifest=next(r for r in rs if r['kind']=='decision-manifest')
        for member in manifest['body']['members']:
            key=(member['kind'],canonical(member['id']))
            if key in lookup:member['content_hash']=lookup[key]
        manifest['content_hash']='sha256:'+hash_value('decision-content',manifest['body'])
        receipt=next(r for r in rs if r['kind']=='receipt');receipt['body']['decision_hash']=manifest['content_hash'];receipt['content_hash']='sha256:'+hash_value('record-content',['receipt',1,receipt['body']])
    def semantic_mutant(change):
        rs=copy.deepcopy(rows);change(rs);reseal(rs);verify_journal(rs,seeds)
    reject('rehashed manifest wrong order',lambda:semantic_mutant(lambda rs:next(r for r in rs if r['kind']=='decision-manifest')['body']['members'].reverse()))
    reject('rehashed action economics mismatch',lambda:semantic_mutant(lambda rs:next(r for r in rs if r['kind']=='action')['body']['amount'].update(atoms='101')))
    reject('rehashed incomplete manifest',lambda:semantic_mutant(lambda rs:next(r for r in rs if r['kind']=='decision-manifest')['body']['members'].pop()))
    for raw in ['{"a":1,"a":2}','{"a":-0}','{"a":1.0}','{"a":1e0}','{"a":9007199254740992}']:reject('strict JSON '+raw,lambda raw=raw:strict_json(raw))
    node=json.loads(subprocess.check_output(['node',str(root/'scripts/contract_checks/check_records.mjs'),str(root)],text=True))
    assert node['journal_sha256']==hashlib.sha256(expected['accepted-records.jsonl']).hexdigest()
    return {'status':'passed','python_and_node':'independent constructions match every byte','schemas':len(resources)//2,'schema_definitions':len(schema['$defs']),'vectors':len(x['vectors']),'seed_documents':len(seeds),'seed_immutable_rows':len(seedrows),'accepted_immutable_records':len(rows),'manifest_members':29,'fixture_files':len(expected),'tagged_input_variants':tagged_positive,'negative_checks':negative,'journal_sha256':node['journal_sha256'],'decision_hash':x['labels']['D']['content_hash'],'receipt_record_hash':x['labels']['R']['content_hash'],'addendum_sha256':hashlib.sha256(markdown.encode()).hexdigest(),'node':node,'limits':['Document/fixture checks only; no production engine or database tests.','Full-v0 variants and remaining Phase 0 deliverables are outside this five-blocker repair.']}
if __name__=='__main__':
    result=audit(Path(sys.argv[1]) if len(sys.argv)>1 else ROOT)
    print(json.dumps(result,indent=2))
