"""Semantic scalar boundaries and complete retained-source round trips."""
import copy
import json
import subprocess
from pathlib import Path
import jsonschema
from profile import canonical, envelope, reference, strict
from retained import event_projection, binding_projection, family_projection
from scalars import Validator


def cases():
    # These cases also feed the disposable approved-Rust comparison. Canonical
    # storage uses normalized strings; ingress normalization is a separate test.
    result=[]
    def add(kind, values):
        result.extend(dict(kind=kind, value=v, accepted=ok) for v,ok in values)
    add('text', [('',False),('é'*64,True),('é'*64+'a',False),('😀'*32,True),('😀'*32+'a',False),
                 ('a'*128,True),('a'*129,False),('a\x85',False),('e\u0301',True)])
    add('source', [('urn:'+('é'*126),True),('urn:'+('é'*126)+'a',False),('relative',False),
                   ('urn:has space',False),('1urn:value',False),('urn:ok',True),('urn:synthetic:\ufeffoutcome',True)])
    add('slug', [('a'*64,True),('a'*65,False),('é',False),('a_1.-',True),('1a',False)])
    add('decimal', [('0',True),('1',True),('-1',False),('+1',False),('-0',False),('0.1',True),
                    ('0.'+'0'*17+'1',True),('0.'+'0'*18+'1',False),('9'*30,True),('1'+'0'*30,False)])
    add('decimal-percent', [('0',True),('100',True),('100.000000000000000001',False),('-1',False)])
    add('positive-decimal', [('0',False),('0.000000000000000001',True),('-1',False),('9'*30,True)])
    add('uint', [('0',True),('9223372036854775807',True),('9223372036854775808',False),('-1',False),('01',False)])
    add('atoms', [('0',True),('9'*30,True),('-'+'9'*30,True),('1'+'0'*30,False),('-0',False),('+1',False)])
    add('time', [('0001-01-01T00:00:00.000000Z',True),('9999-12-31T23:59:59.999999Z',True),
                 ('0000-01-01T00:00:00.000000Z',False),('2024-02-29T00:00:00.000000Z',True),
                 ('2026-02-29T00:00:00.000000Z',False),('2026-09-21T12:00:60.000000Z',False)])
    add('ratio', [({'numerator':'-1','denominator':'2'},True),({'numerator':'0','denominator':'1'},True),
                  ({'numerator':'2','denominator':'4'},False),({'numerator':'1','denominator':'0'},False),
                  ({'numerator':str(2**512-1),'denominator':'1'},True),
                  ({'numerator':str(2**512),'denominator':'1'},False)])
    add('nonnegative-atoms',[('0',True),('9'*30,True),('-1',False),('-0',False),('01',False)])
    # The whole spelling must pass BEFORE numeric conversion. In particular,
    # Python int/Fraction and JavaScript BigInt accept whitespace themselves.
    samples={'atoms':'2500','nonnegative-atoms':'2500','uint':'1','slug':'success',
             'decimal':'2.5','positive-decimal':'2.5','decimal-percent':'2.5',
             'time':'2026-09-21T12:00:00.000000Z','source':'urn:synthetic:work'}
    for kind,value in samples.items():
        add(kind,[(value+suffix,False) for suffix in ('\n','\r','\r\n','\t',' ','\x85','\u2028')])
        add(kind,[(' '+value,False)])
    for field in ('numerator','denominator'):
        for suffix in ('\n','\r','\r\n','\t',' ','\x85','\u2028'):
            value={'numerator':'1','denominator':'2'};value[field]+=suffix
            add('ratio',[(value,False)])
    for kind,prefix in [('original-event-id','ev_'),('original-claim-id','cl_'),('original-action-id','ac_'),('original-effect-id','ef_'),('original-obligation-id','ob_'),('document-id','doc_'),('event-id','ev2_'),('hash','sha256:')]:
        for value,ok in [(prefix+'a'*64,True)]+[(prefix+'a'*64+s,False) for s in ('\n','\r','\t',' ','\x85','\u2028')]+[(prefix+'A'*64,False)]:
            result.append(dict(kind=kind,value=value,accepted=ok,rust_prefix=prefix))
    return result


def run_boundaries(histories, schema, checked_row, verify):
    values=cases(); negative=0
    path=Path(__file__).resolve().parents[3]/'work/validation/v2-scalar-cases.json'
    path.parent.mkdir(parents=True,exist_ok=True);path.write_text(json.dumps(values,ensure_ascii=False))
    subprocess.run(['node',str(Path(__file__).with_name('scalars.mjs')),str(Path(__file__).resolve().parents[3]/'contracts/candidates/v2/schemas/canonical-records.schema.json'),str(path)],check=True)
    for case in values:
        validator=Validator({'$ref':'#/$defs/'+case['kind'],'$defs':schema['$defs']})
        actual=validator.is_valid(case['value'])
        assert actual==case['accepted'], ('SCALAR_BOUNDARY',case,actual)
        negative += not actual
    # Discover every top-level text field, including newly added identifiers.
    # The same definition must reject a >128-byte scalar regardless of its name.
    record_tests=0
    for kind in ('policy-snapshot','binding-snapshot','base-posting','admission','claim','action','effect','limit-evidence','authority-decision'):
        row=next(r for h in histories for r in h['seed']+[v for d in h['decisions'] for v in d['records']] if r['kind']==kind)
        for field,rule in schema['$defs'][kind]['properties'].items():
            if rule.get('$ref')!='#/$defs/text':continue
            for value,ok in [('é'*64,True),('é'*64+'x',False)]:
                b=copy.deepcopy(row['body']);b[field]=value
                changed=envelope(kind,row['scope'],b)
                try:checked_row(changed)
                except jsonschema.ValidationError:actual=False
                else:actual=True
                assert actual==ok, ('IDENTIFIER_FIELD',kind,field,actual)
                record_tests+=1;negative+=not actual
    # Signs/normalization differ between wire Decimal input and stored Decimal.
    for v in ('00','1.0','0.0','01.1','1.','1e0'):
        assert not Validator({'$ref':'#/$defs/decimal','$defs':schema['$defs']}).is_valid(v)
        negative+=1
    for name in ('binding-snapshot','invocation','retained-binding'):
        definition=schema['$defs'][name]['properties']['maximum_quantity']
        assert not Validator({**definition,'$defs':schema['$defs']}).is_valid('-1')
        negative+=1
    # Each schema string needs a fixed enum/pattern or an explicit byte bound.
    def coverage(v,path=''):
        if isinstance(v,dict):
            if v.get('type')=='string':
                assert 'x-utf8-maxBytes' in v or 'const' in v or 'enum' in v, ('UNBOUNDED_STRING',path)
            for k,x in v.items():coverage(x,path+'/'+k)
        elif isinstance(v,list):
            for i,x in enumerate(v):coverage(x,path+'/'+str(i))
    coverage(schema)
    h=next(h for h in histories if h['name']=='lossless-event-and-outcome-terms')
    bykind=lambda k:next(r for r in h['seed'] if r['kind']==k)
    evaluation=bykind('base-evaluation')['body'];m=strict(evaluation['evaluation_utf8'].encode())
    aliases={r['body']['document_id']:r['id'] for r in h['seed'] if r['kind']=='evidence'}
    assert m['event']['extensions']=={'source':'opaque extension, not authority','policy_version':'not operative','label':'é/😀','count':9007199254740991,'verified':False}
    assert canonical(m['event']).decode()==evaluation['original_event_utf8']
    assert event_projection(m['event'],m['source_authority']['source'],aliases)==bykind('event')['body']['data']
    binding=bykind('binding-snapshot')['body'];source=strict(binding['binding_utf8'].encode())
    assert source==m['bundle']['policies'][0]['binding']==m['actions'][0]['binding']
    assert set(source['outcome'])=={'source','window_us','report_grace_us','claim_namespace'}
    assert {k:v for k,v in binding.items() if k not in ('schema','binding_utf8','booked_net','supplier_invocation')}==binding_projection(source,aliases)
    target=bykind('target-snapshot')['body'];policy=strict(target['policy_utf8'].encode())
    assert family_projection(policy['families'][0])=={k:bykind('policy-snapshot')['body'][k] for k in family_projection(policy['families'][0])}
    # Every optional field in the approved retained Evaluation types has a slot;
    # serialization round trips preserve values, absence and ordered vectors.
    full=copy.deepcopy(m)
    full['closed_stage']='preserved-stage'
    full['context'].update(tier='enterprise',priority=False,stage=dict(id='stage',expected=[dict(source='urn:synthetic:work',operation_id='base',kind='content.generated',retail_components=['base-retail-0'])],closure_claim_namespace='sale'))
    a=full['actions'][0]
    a.update(reverses='ac_'+'a'*64,allocation_parent='ac_'+'b'*64,allocation_recipient='recipient',discount_target='ac_'+'c'*64)
    full['explanations'][0]['basis']={'numerator':'10000','denominator':'1'}
    rule=full['bundle']['policies'][0]['rules'][0]
    rule['matcher']=dict(kind='direct',relation='generated_from',target='content.generated')
    rule['when']=[dict(kind='tier',value='enterprise'),dict(kind='priority',value=False)]
    source=full['bundle']['policies'][0]['binding'];source['roles']['payer_delegation']=next(iter(aliases));source['offer']=next(iter(aliases));source['maximum_exposure']=dict(currency='USD',scale=2,atoms='10000')
    full['costs']=[dict(binding_id='retained-cost',event_id='ev_'+'d'*64,document=next(iter(aliases)),amount=dict(currency='USD',scale=2,atoms='1'))]
    Validator({'$ref':'#/$defs/base-material','$defs':schema['$defs']}).validate(full)
    assert strict(canonical(full))==full, 'LOSSLESS_COMPLETE_ROUNDTRIP'
    # This structural sample intentionally does not assert economic admission.
    for key in ('unknown_economic_field',):
        unknown=copy.deepcopy(full);unknown['bundle']['policies'][0]['binding'][key]='discard me'
        assert not Validator({'$ref':'#/$defs/base-material','$defs':schema['$defs']}).is_valid(unknown)
        negative+=1
    verify(h,reference(bykind('base-acceptance')))
    from semantics import exact_percentage
    from fractions import Fraction
    assert exact_percentage(100,{'numerator':'1','denominator':str(2**510+1)})==Fraction(1,2**510+1), 'PERCENT_CROSS_CANCEL'
    result=dict(scalar_cases=len(values),record_field_boundaries=record_tests,negative_checks=negative,lossless_roundtrips=2)
    print(json.dumps(dict(status='passed',boundary_audit=result)))
    return negative
