"""Independent Python construction of the frozen first-slice contract. No product imports."""
from pathlib import Path
import hashlib, json, re
from fractions import Fraction

ROOT = Path(__file__).resolve().parents[2]
SCOPE = ['demo', 'sandbox']
T = '2026-09-20T14:00:00.000000Z'
PREFIX = {'event':'ev','claim':'cl','decision':'dc','receipt':'rc','effect':'ef','action':'ac','obligation':'ob','intention':'in','document':'doc','snapshot-ref':'sr','explanation':'xp','control-transition':'ct'}

def canonical(v):
    if isinstance(v, dict):
        return '{' + ','.join(canonical(k)+':'+canonical(v[k]) for k in sorted(v,key=lambda x:x.encode('utf-16-be'))) + '}'
    if isinstance(v, list): return '['+','.join(map(canonical,v))+']'
    if isinstance(v,int) and not isinstance(v,bool): assert abs(v)<=9007199254740991
    assert v is None or isinstance(v,(str,int,bool))
    return json.dumps(v,ensure_ascii=False,separators=(',',':'),allow_nan=False)

def hash_value(domain,v):
    return hashlib.sha256(('ledgerlab/'+domain+'/1\0'+canonical(v)).encode()).hexdigest()

def money(n): return {'currency':'USD','scale':2,'atoms':str(n)}
def ratio(n,d=1):
    f=Fraction(n,d)
    return {'numerator':str(f.numerator),'denominator':str(f.denominator)}

def build():
    vectors=[]; records=[]; seeds=[]; labels={}
    def H(name,domain,value,prefix=None):
        c=canonical(value); pre=('ledgerlab/'+domain+'/1\0'+c).encode(); h=hashlib.sha256(pre).hexdigest()
        v={'name':name,'domain':domain,'value':value,'canonical_utf8':c,'canonical_hex':c.encode().hex(),'hash_input_hex':pre.hex(),'sha256':h}
        if prefix: v['id']=prefix+'_'+h
        vectors.append(v)
        return v.get('id','sha256:'+h)
    def ID(name,domain,value): return H(name,domain,value,PREFIX[domain])
    def row(label,kind,id,body,seed=False,doc_type=None):
        if kind=='document': digest='sha256:'+id[4:]
        elif kind=='event': digest=H('event-content','event-content',body)
        elif kind=='decision-manifest': digest=H('decision-content','decision-content',body)
        else: digest=H(label+'.content','record-content',[kind,1,body])
        r={'kind':kind,'scope':SCOPE,'id':id,'body':body,'content_hash':digest}
        if doc_type: r['document_type']=doc_type
        (seeds if seed else records).append(r); labels[label]=r
        return r
    def doc(label,typ,body,seed=True):
        id=ID(label,'document',[typ,1,body]); row(label,'document',id,body,seed,typ); return id
    def body(record_kind,**kw): return dict(schema='ledger-'+record_kind+'/1',**kw)
    roles={'provider':'demo-host','cost_originator':'demo-host','bearer':'demo-customer','payer':'demo-customer','beneficiary':'demo-customer','recipient':'demo-host'}
    policy=body('policy',id='demo-retail-v1',currency='USD',scale=2,rounding='nearest_ties_away',rules=[
      dict(id='generation-base',on='content.generated',op='base',component='generation.base',book='retail',amount={'fixed':'1'}),
      dict(id='tier-discount',on='content.generated',op='discount',component='generation.discount',book='retail',when=[{'field':'binding.tier','eq':'enterprise'}],amount={'percent':'20','basis':'self.generation.base'},discount_mode='additive')])
    p=doc('P','policy',policy)
    ro=doc('RO','roles',body('roles',**roles))
    evidence_bytes=b'synthetic:fixture-1'
    assent=body('assent',mode='demo',agreement_id='demo-retail',terms_version='1',acceptor='demo-admin',bearer='demo-customer',payer='demo-customer',recipient='demo-host',accepted_at=T,evidence_ref=evidence_bytes.decode(),evidence_digest='sha256:'+hashlib.sha256(evidence_bytes).hexdigest())
    ass=doc('AS','assent',assent)
    grant=body('source-grant',id='demo-source-grant-v1',principal_id='demo-app',source='urn:demo:app',event_types=['content.generated'],relations=[],permissions=['read','submit'],starts_at=T)
    gr=doc('G','source-grant',grant)
    context=body('context',tier='enterprise',funding='byok',currency='USD',scale=2,binding_ids=['demo-retail-v1'])
    cx=doc('CX','context',context)
    binding=body('binding',id='demo-retail-v1',agreement_id='demo-retail',version=1,policy=p,roles=ro,assent=ass,context=cx,acceptor='demo-admin',accepted_at=T,starts_at=T,service='generation',customer='demo-customer',sources=['urn:demo:app'],event_types=['content.generated'],unit='call',maximum_quantity='1',correction_sources=['urn:demo:app'],allocation_view=False)
    b=doc('B','binding',binding)
    event=body('event',id='generation-1',source='urn:demo:app',operation_id='generation-1',type='content.generated',customer='demo-customer',chain='demo-slice',quantity='1',status='succeeded',unit='call',links=[],evidence=[],extensions={})
    e=ID('E','event',SCOPE+['urn:demo:app','generation-1'])
    c=ID('C','claim',[SCOPE,'urn:demo:app','generation-1','completion','completion'])
    d=ID('D','decision',[e]); r=ID('R','receipt',[e])
    f1=ID('F1','effect',[SCOPE,'demo-retail','generation.base',c,'self','original'])
    f2=ID('F2','effect',[SCOPE,'demo-retail','generation.discount',c,'self','original'])
    a1=ID('A1','action',[f1]); a2=ID('A2','action',[f2])
    o=ID('O','obligation',[SCOPE,'demo-retail','retail','USD',2,roles])
    i=ID('I','intention',[SCOPE,'fake',o,sorted([a1,a2])])
    docs={'policy':p,'roles':ro,'assent':ass,'source_grant':gr,'binding':b,'chain_context':cx}
    # Purpose references are a set: sort canonical element bytes, NOT purpose field.
    inputs=sorted([{'purpose':purpose,'document_id':id} for purpose,id in docs.items()],key=lambda x:canonical(x).encode())
    snapshot=body('snapshot',scope=SCOPE,dsl_version=1,semantics_version=1,documents=inputs,context={'tier':'enterprise','funding':'byok','currency':'USD','scale':2,'binding_ids':['demo-retail-v1']},authority=[{'principal_id':'demo-app','source':'urn:demo:app','grant_id':'demo-source-grant-v1','grant_document':gr,'revision':'1','active':True}],prior_actions=[],decision_context={})
    s=doc('S','snapshot',snapshot,False)
    for purpose,id in {**docs,'decision_snapshot':s}.items():
        sr=ID('SR.'+purpose,'snapshot-ref',[e,purpose,id])
        row('SR.'+purpose,'snapshot-ref',sr,body('snapshot-ref',id=sr,scope=SCOPE,event_id=e,purpose=purpose,document_id=id))
    er=row('E','event',e,event)
    ingress=H('ingress','ingress',event)
    dkid=[SCOPE,'urn:demo:app','generation-1']
    row('DK','delivery-key',dkid,body('delivery-key',scope=SCOPE,source='urn:demo:app',external_id='generation-1',canonical_event_id=e,kind='original',ingress=event,ingress_hash=ingress))
    facts=body('claim-facts',type='content.generated',chain='demo-slice',customer='demo-customer',status='succeeded',quantity='1',unit='call',links=[],evidence=[])
    cf=H('claim-facts','claim-facts',facts)
    row('C','claim',c,body('claim',id=c,scope=SCOPE,source='urn:demo:app',operation_id='generation-1',kind='completion',token='completion',facts_hash=cf,event_id=e))
    for ix,(f,a,component,kind,atoms,rule,dependencies) in enumerate([(f1,a1,'generation.base','charge',100,'generation-base',[]),(f2,a2,'generation.discount','discount',-20,'tier-discount',[a1])],1):
        efacts=body('effect-facts',scope=SCOPE,agreement_id='demo-retail',claim_id=c,component=component,match_key='self',namespace='original',kind=kind,book='retail',amount=money(atoms),roles=roles,sources=[e],links=[],inputs=dependencies)
        efh=H('effect-facts.'+str(ix),'effect-facts',efacts)
        row('F'+str(ix),'effect',f,body('effect',id=f,scope=SCOPE,agreement_id='demo-retail',component=component,claim_id=c,match_key='self',namespace='original',facts_hash=efh,action_id=a))
        row('A'+str(ix),'action',a,body('action',id=a,scope=SCOPE,event_id=e,decision_id=d,effect_id=f,obligation_id=o,kind=kind,book='retail',component=component,amount=money(atoms),roles=roles,roles_doc=ro,binding_id='demo-retail-v1',rule_id=rule,sources=[e],links=[],inputs=dependencies,snapshot_doc=s))
        row('SOURCE.'+str(ix),'action-source',[SCOPE,a,e],body('action-source',scope=SCOPE,action_id=a,event_id=e))
    row('DEPENDENCY','action-dependency',[SCOPE,a2,a1],body('action-dependency',scope=SCOPE,action_id=a2,input_action_id=a1))
    xp=[]
    for ordinal in (0,1):
        x=ID('XP'+str(ordinal),'explanation',[e,ordinal]); xp.append(x)
        common=dict(id=x,scope=SCOPE,event_id=e,ordinal=ordinal,rule_id=['generation-base','tier-discount'][ordinal],outcome='applied',code=['BASE_APPLIED','DISCOUNT_APPLIED'][ordinal],binding_id='demo-retail-v1')
        if ordinal==0:
            common.update(input_refs=[p],inputs=[{'kind':'decimal','name':'fixed','value':'1','exact':ratio(1)}],unrounded_atoms=ratio(100),rounded_atoms='100',action_ids=[a1])
        else:
            common.update(input_refs=sorted([a1,cx,p]),basis_name='self.generation.base',basis=ratio(100),inputs=[{'kind':'binding_field','name':'binding.tier','value':'enterprise'},{'kind':'decimal','name':'percent','value':'20','exact':ratio(20)},{'kind':'action_ref','name':'basis','value':a1}],unrounded_atoms=ratio(-20),rounded_atoms='-20',action_ids=[a2])
        row('XP'+str(ordinal),'explanation',x,body('explanation',**common))
    payload=body('obligation-delta',type='obligation_delta',obligation_id=o,agreement_id='demo-retail',book='retail',amount=money(80),roles=roles,actions=[{'action_id':a1,'kind':'charge','component':'generation.base','amount':money(100)},{'action_id':a2,'kind':'discount','component':'generation.discount','amount':money(-20)}])
    H('intention-payload','intention-payload',payload)
    row('I','intention',i,body('intention',id=i,scope=SCOPE,event_id=e,destination_id='fake',idempotency_key=i,obligation_id=o,action_ids=sorted([a1,a2]),amount=money(80),depends_on=[],payload=payload))
    ct=ID('CT','control-transition',[SCOPE,'chain','demo-slice','1'])
    row('CT','control-transition',ct,body('control-transition',id=ct,scope=SCOPE,control_kind='chain',control_id='demo-slice',from_revision='0',to_revision='1',event_id=e,document_id=s,from_event_count='0',to_event_count='1'))
    row('CR','chain-revision',[SCOPE,'demo-slice','1'],body('chain-revision',scope=SCOPE,chain_id='demo-slice',revision='1',event_id=e,decision_id=d))
    order=lambda v:(v['kind'].encode(),canonical(v['id']).encode())
    members=[{k:v[k] for k in ('kind','id','content_hash')} for v in sorted(seeds+records,key=order)]
    assert len(members)==29
    manifest=body('decision-manifest',id=d,scope=SCOPE,event_id=e,chain_id='demo-slice',revision='1',explanation_ids=xp,members=members)
    mr=row('D','decision-manifest',d,manifest)
    row('R','receipt',r,body('receipt',id=r,event_id=e,decision_id=d,chain_id='demo-slice',revision='1',content_hash=er['content_hash'],decision_hash=mr['content_hash'],action_ids=sorted([a1,a2]),intention_ids=[i]))
    seed_records=[]
    for party in ['demo-customer','demo-host']:
        pr=body('party',id=party,scope=SCOPE,role_metadata_doc=ro)
        seed_records.append({'kind':'party','scope':SCOPE,'id':party,'body':pr,'content_hash':H('party.'+party,'record-content',['party',1,pr])})
    for kind,id,extra in [('source-grant-record','demo-source-grant-v1',dict(principal_id='demo-app',source='urn:demo:app',grant_doc=gr)),('binding-record','demo-retail-v1',dict(agreement_id='demo-retail',version=1,policy_doc=p,roles_doc=ro,assent_doc=ass,context_doc=cx,currency='USD',scale=2))]:
        pr=body(kind,id=id,scope=SCOPE,**extra)
        seed_records.append({'kind':kind,'scope':SCOPE,'id':id,'body':pr,'content_hash':H(kind,'record-content',[kind,1,pr])})
    preseed={'schema':'ledger-first-slice-state/1','scope':SCOPE,'logical_store_id':'store-demo-slice','mode':'sandbox','admission':'open','dispatch_enabled':False,'dispatch_hold':True,'chain':{'id':'demo-slice','customer':'demo-customer','currency':'USD','scale':2,'binding_set_doc':cx,'context_doc':cx,'revision':'0','event_count':'0'},'authority_head':{'id':'demo-source-grant-v1','grant_id':'demo-source-grant-v1','revision':'1','active':True},'binding_head':{'id':'demo-retail-selector','binding_id':'demo-retail-v1','selector_doc':b,'revision':'1','active':True},'principals':['demo-admin','demo-app'],'credentials':'excluded'}
    post={'schema':'ledger-first-slice-operational/1','scope':SCOPE,'received_at':T,'delivery_key_observed_at':T,'chain':{**preseed['chain'],'revision':'1','event_count':'1'},'delivery_state':{'intention_id':i,'state':'held','attempts':'0','generation':'0','next_attempt_at':T}}
    return dict(vectors=vectors,seed_documents=sorted(seeds,key=order),seed_records=sorted(seed_records,key=order),records=sorted(records,key=order),labels=labels,claim_facts=facts,preseed=preseed,post=post,roles=roles)

def artifacts(data):
    # Hash vectors include exact framed input, not merely the final digest.
    result={}
    for name,key in [('seed-documents.jsonl','seed_documents'),('seed-records.jsonl','seed_records'),('accepted-records.jsonl','records')]:
        result[name]=''.join(canonical(v)+'\n' for v in data[key]).encode()
    for name,key in [('preseed-state.json','preseed'),('post-acceptance-state.json','post'),('claim-facts.json','claim_facts')]: result[name]=canonical(data[key]).encode()
    for name,label in [('snapshot.json','S'),('manifest.json','D'),('receipt.json','R'),('explanation-0.json','XP0'),('explanation-1.json','XP1')]: result[name]=canonical(data['labels'][label]['body']).encode()
    for n in (1,2): result[f'effect-facts-{n}.json']=canonical(next(v['value'] for v in data['vectors'] if v['name']==f'effect-facts.{n}')).encode()
    result['vectors.json']=canonical({'schema':'ledger-canonical-vectors/1','vectors':sorted(data['vectors'],key=lambda x:x['name'].encode())}).encode()
    result['synthetic-assent-evidence.txt']=b'synthetic:fixture-1'
    hashes={name:{'bytes':len(raw),'sha256':hashlib.sha256(raw).hexdigest()} for name,raw in result.items()}
    result['file-digests.json']=canonical({'schema':'ledger-fixture-digests/1','files':hashes}).encode()
    return result
