// Independent Node reconstruction from the addendum's literals and projections.
// Does not invoke/read Python builders or use fixture hashes/bodies as input.
import {createHash} from 'node:crypto';
import {readFileSync,readdirSync} from 'node:fs';
import {join,resolve} from 'node:path';
import assert from 'node:assert/strict';
export function jcs(x) {
  if (Array.isArray(x)) return `[${x.map(jcs).join(',')}]`;
  if(x!==null && typeof x==='object') return `{${Object.keys(x).sort().map(k=>JSON.stringify(k)+':'+jcs(x[k])).join(',')}}`;
  if(typeof x==='number') assert(Number.isSafeInteger(x) && !Object.is(x,-0));
  return JSON.stringify(x);
}
const rawHash=x=>createHash('sha256').update(x).digest('hex');
const cmp=(a,b)=>Buffer.compare(Buffer.from(a),Buffer.from(b));
const scope=['demo','sandbox'],time='2026-09-20T14:00:00.000000Z';
const prefix={event:'ev',claim:'cl',decision:'dc',receipt:'rc',effect:'ef',action:'ac',obligation:'ob',intention:'in',document:'doc','snapshot-ref':'sr',explanation:'xp','control-transition':'ct'};
export function reconstruct() {
  const vectors=[],seeds=[],records=[],labels={},seedRecords=[];
  function h(name,domain,value,idPrefix) {
    const canonical_utf8=jcs(value),pre=Buffer.concat([Buffer.from('ledgerlab/'+domain+'/1'),Buffer.from([0]),Buffer.from(canonical_utf8)]),sha256=rawHash(pre);
    const v={name,domain,value,canonical_utf8,canonical_hex:Buffer.from(canonical_utf8).toString('hex'),hash_input_hex:pre.toString('hex'),sha256};
    if(idPrefix)v.id=idPrefix+'_'+sha256;
    vectors.push(v);return v.id??'sha256:'+sha256;
  }
  const id=(alias,kind,input)=>h(alias,kind,input,prefix[kind]);
  const body=(kind,x)=>({schema:`ledger-${kind}/1`,...x});
  function add(alias,kind,key,b,preseed=false,typ) {
    const content_hash=kind==='document'?'sha256:'+key.slice(4):kind==='event'?h('event-content','event-content',b):kind==='decision-manifest'?h('decision-content','decision-content',b):h(alias+'.content','record-content',[kind,1,b]);
    const row={kind,scope,id:key,body:b,content_hash};if(typ)row.document_type=typ;
    (preseed?seeds:records).push(row);labels[alias]=row;return row;
  }
  function doc(alias,typ,b,seed=true) {const key=id(alias,'document',[typ,1,b]);add(alias,'document',key,b,seed,typ);return key;}
  const usd=n=>({atoms:String(n),scale:2,currency:'USD'}),q=n=>({numerator:String(n),denominator:'1'});
  const roles={bearer:'demo-customer',payer:'demo-customer',beneficiary:'demo-customer',provider:'demo-host',cost_originator:'demo-host',recipient:'demo-host'};
  const P=doc('P','policy',body('policy',{id:'demo-retail-v1',currency:'USD',scale:2,rounding:'nearest_ties_away',rules:[
    {id:'generation-base',on:'content.generated',op:'base',component:'generation.base',book:'retail',amount:{fixed:'1'}},
    {id:'tier-discount',on:'content.generated',op:'discount',component:'generation.discount',book:'retail',when:[{field:'binding.tier',eq:'enterprise'}],amount:{percent:'20',basis:'self.generation.base'},discount_mode:'additive'}]}));
  const RO=doc('RO','roles',body('roles',roles));
  const AS=doc('AS','assent',body('assent',{mode:'demo',agreement_id:'demo-retail',terms_version:'1',acceptor:'demo-admin',bearer:'demo-customer',payer:'demo-customer',recipient:'demo-host',accepted_at:time,evidence_ref:'synthetic:fixture-1',evidence_digest:'sha256:'+rawHash(Buffer.from('synthetic:fixture-1'))}));
  const G=doc('G','source-grant',body('source-grant',{id:'demo-source-grant-v1',principal_id:'demo-app',source:'urn:demo:app',event_types:['content.generated'],relations:[],permissions:['read','submit'],starts_at:time}));
  const CX=doc('CX','context',body('context',{tier:'enterprise',funding:'byok',currency:'USD',scale:2,binding_ids:['demo-retail-v1']}));
  const B=doc('B','binding',body('binding',{id:'demo-retail-v1',agreement_id:'demo-retail',version:1,policy:P,roles:RO,assent:AS,context:CX,acceptor:'demo-admin',accepted_at:time,starts_at:time,service:'generation',customer:'demo-customer',sources:['urn:demo:app'],event_types:['content.generated'],unit:'call',maximum_quantity:'1',correction_sources:['urn:demo:app'],allocation_view:false}));
  const E=id('E','event',[...scope,'urn:demo:app','generation-1']);
  const C=id('C','claim',[scope,'urn:demo:app','generation-1','completion','completion']);
  const D=id('D','decision',[E]),R=id('R','receipt',[E]);
  const F1=id('F1','effect',[scope,'demo-retail','generation.base',C,'self','original']);
  const F2=id('F2','effect',[scope,'demo-retail','generation.discount',C,'self','original']);
  const A1=id('A1','action',[F1]),A2=id('A2','action',[F2]);
  const O=id('O','obligation',[scope,'demo-retail','retail','USD',2,roles]);
  const I=id('I','intention',[scope,'fake',O,[A1,A2].sort()]);
  const documents={policy:P,roles:RO,assent:AS,source_grant:G,binding:B,chain_context:CX};
  const S=doc('S','snapshot',body('snapshot',{scope,dsl_version:1,semantics_version:1,documents:Object.entries(documents).map(([purpose,document_id])=>({purpose,document_id})).sort((a,b)=>cmp(jcs(a),jcs(b))),context:{tier:'enterprise',funding:'byok',currency:'USD',scale:2,binding_ids:['demo-retail-v1']},authority:[{principal_id:'demo-app',source:'urn:demo:app',grant_id:'demo-source-grant-v1',grant_document:G,revision:'1',active:true}],prior_actions:[],decision_context:{}}),false);
  for(const [purpose,document_id] of Object.entries({...documents,decision_snapshot:S})) {const key=id('SR.'+purpose,'snapshot-ref',[E,purpose,document_id]);add('SR.'+purpose,'snapshot-ref',key,body('snapshot-ref',{id:key,scope,event_id:E,purpose,document_id}));}
  const event=body('event',{chain:'demo-slice',customer:'demo-customer',id:'generation-1',source:'urn:demo:app',operation_id:'generation-1',quantity:'1',status:'succeeded',type:'content.generated',unit:'call',links:[],evidence:[],extensions:{}});
  const eRow=add('E','event',E,event),ingress_hash=h('ingress','ingress',event);
  add('DK','delivery-key',[scope,'urn:demo:app','generation-1'],body('delivery-key',{scope,source:'urn:demo:app',external_id:'generation-1',canonical_event_id:E,kind:'original',ingress:event,ingress_hash}));
  const claimFacts=body('claim-facts',{chain:'demo-slice',customer:'demo-customer',type:'content.generated',status:'succeeded',quantity:'1',unit:'call',links:[],evidence:[]});
  const facts_hash=h('claim-facts','claim-facts',claimFacts);
  add('C','claim',C,body('claim',{scope,id:C,event_id:E,source:'urn:demo:app',operation_id:'generation-1',kind:'completion',token:'completion',facts_hash}));
  const descriptions=[{n:1,f:F1,a:A1,component:'generation.base',kind:'charge',atoms:100n,rule:'generation-base',inputs:[]},{n:2,f:F2,a:A2,component:'generation.discount',kind:'discount',atoms:-20n,rule:'tier-discount',inputs:[A1]}];
  for(const x of descriptions) {
    const economic={kind:x.kind,book:'retail',amount:usd(x.atoms),roles,sources:[E],links:[],inputs:x.inputs};
    const key={scope,agreement_id:'demo-retail',component:x.component,claim_id:C,match_key:'self',namespace:'original'};
    const fh=h('effect-facts.'+x.n,'effect-facts',body('effect-facts',{...key,...economic}));
    add('F'+x.n,'effect',x.f,body('effect',{id:x.f,...key,facts_hash:fh,action_id:x.a}));
    add('A'+x.n,'action',x.a,body('action',{id:x.a,scope,event_id:E,decision_id:D,effect_id:x.f,obligation_id:O,component:x.component,...economic,roles_doc:RO,binding_id:'demo-retail-v1',rule_id:x.rule,snapshot_doc:S}));
    add('SOURCE.'+x.n,'action-source',[scope,x.a,E],body('action-source',{scope,action_id:x.a,event_id:E}));
  }
  add('DEPENDENCY','action-dependency',[scope,A2,A1],body('action-dependency',{scope,action_id:A2,input_action_id:A1}));
  const XP0=id('XP0','explanation',[E,0]);
  add('XP0','explanation',XP0,body('explanation',{id:XP0,scope,event_id:E,ordinal:0,rule_id:'generation-base',outcome:'applied',code:'BASE_APPLIED',binding_id:'demo-retail-v1',input_refs:[P],inputs:[{kind:'decimal',name:'fixed',value:'1',exact:q(1)}],unrounded_atoms:q(100),rounded_atoms:'100',action_ids:[A1]}));
  const XP1=id('XP1','explanation',[E,1]);
  add('XP1','explanation',XP1,body('explanation',{id:XP1,scope,event_id:E,ordinal:1,rule_id:'tier-discount',outcome:'applied',code:'DISCOUNT_APPLIED',binding_id:'demo-retail-v1',input_refs:[A1,CX,P].sort(),basis_name:'self.generation.base',basis:q(100),inputs:[{kind:'binding_field',name:'binding.tier',value:'enterprise'},{kind:'decimal',name:'percent',value:'20',exact:q(20)},{kind:'action_ref',name:'basis',value:A1}],unrounded_atoms:q(-20),rounded_atoms:'-20',action_ids:[A2]}));
  const payload=body('obligation-delta',{type:'obligation_delta',obligation_id:O,agreement_id:'demo-retail',book:'retail',amount:usd(80),roles,actions:descriptions.map(x=>({action_id:x.a,kind:x.kind,component:x.component,amount:usd(x.atoms)}))});
  h('intention-payload','intention-payload',payload);
  add('I','intention',I,body('intention',{id:I,scope,event_id:E,destination_id:'fake',idempotency_key:I,obligation_id:O,action_ids:[A1,A2].sort(),amount:usd(80),depends_on:[],payload}));
  const CT=id('CT','control-transition',[scope,'chain','demo-slice','1']);
  add('CT','control-transition',CT,body('control-transition',{id:CT,scope,control_kind:'chain',control_id:'demo-slice',from_revision:'0',to_revision:'1',event_id:E,document_id:S,from_event_count:'0',to_event_count:'1'}));
  add('CR','chain-revision',[scope,'demo-slice','1'],body('chain-revision',{scope,chain_id:'demo-slice',revision:'1',event_id:E,decision_id:D}));
  const order=(a,b)=>cmp(a.kind,b.kind)||cmp(jcs(a.id),jcs(b.id));
  const members=[...seeds,...records].sort(order).map(({kind,id,content_hash})=>({kind,id,content_hash}));assert.equal(members.length,29);
  const manifestRow=add('D','decision-manifest',D,body('decision-manifest',{id:D,scope,event_id:E,chain_id:'demo-slice',revision:'1',explanation_ids:[XP0,XP1],members}));
  add('R','receipt',R,body('receipt',{id:R,event_id:E,decision_id:D,chain_id:'demo-slice',revision:'1',content_hash:eRow.content_hash,decision_hash:manifestRow.content_hash,action_ids:[A1,A2].sort(),intention_ids:[I]}));
  function seed(kind,id,extra,name=kind){const b=body(kind,{id,scope,...extra});seedRecords.push({kind,scope,id,body:b,content_hash:h(name,'record-content',[kind,1,b])});}
  for(const party of ['demo-customer','demo-host'])seed('party',party,{role_metadata_doc:RO},'party.'+party);
  seed('source-grant-record','demo-source-grant-v1',{principal_id:'demo-app',source:'urn:demo:app',grant_doc:G});
  seed('binding-record','demo-retail-v1',{agreement_id:'demo-retail',version:1,policy_doc:P,roles_doc:RO,assent_doc:AS,context_doc:CX,currency:'USD',scale:2});
  const preseed={schema:'ledger-first-slice-state/1',scope,logical_store_id:'store-demo-slice',mode:'sandbox',admission:'open',dispatch_enabled:false,dispatch_hold:true,chain:{id:'demo-slice',customer:'demo-customer',currency:'USD',scale:2,binding_set_doc:CX,context_doc:CX,revision:'0',event_count:'0'},authority_head:{id:'demo-source-grant-v1',grant_id:'demo-source-grant-v1',revision:'1',active:true},binding_head:{id:'demo-retail-selector',binding_id:'demo-retail-v1',selector_doc:B,revision:'1',active:true},principals:['demo-admin','demo-app'],credentials:'excluded'};
  const post={schema:'ledger-first-slice-operational/1',scope,received_at:time,delivery_key_observed_at:time,chain:{...preseed.chain,revision:'1',event_count:'1'},delivery_state:{intention_id:I,state:'held',attempts:'0',generation:'0',next_attempt_at:time}};
  const files={};
  for(const [name,rows] of [['seed-documents.jsonl',seeds],['seed-records.jsonl',seedRecords],['accepted-records.jsonl',records]])files[name]=Buffer.from(rows.sort(order).map(jcs).join('\n')+'\n');
  const bodies={'preseed-state.json':preseed,'post-acceptance-state.json':post,'claim-facts.json':claimFacts,'snapshot.json':labels.S.body,'manifest.json':labels.D.body,'receipt.json':labels.R.body,'explanation-0.json':labels.XP0.body,'explanation-1.json':labels.XP1.body};
  for(const n of [1,2])bodies[`effect-facts-${n}.json`]=vectors.find(v=>v.name===`effect-facts.${n}`).value;
  bodies['vectors.json']={schema:'ledger-canonical-vectors/1',vectors:vectors.sort((a,b)=>cmp(a.name,b.name))};
  for(const [name,b] of Object.entries(bodies))files[name]=Buffer.from(jcs(b));
  files['synthetic-assent-evidence.txt']=Buffer.from('synthetic:fixture-1');
  const fileDigests=Object.fromEntries(Object.entries(files).map(([name,b])=>[name,{bytes:b.length,sha256:rawHash(b)}]));
  files['file-digests.json']=Buffer.from(jcs({schema:'ledger-fixture-digests/1',files:fileDigests}));
  return {files,records,seeds,seedRecords,vectors,labels};
}
export function verify(root) {
  const x=reconstruct(),dir=join(root,'fixtures/journals/first-slice');
  assert.deepEqual(readdirSync(dir).sort(),Object.keys(x.files).sort(),'unexpected/missing fixture file');
  for(const [name,b] of Object.entries(x.files))assert(readFileSync(join(dir,name)).equals(b),'independent Node byte mismatch: '+name);
  assert.equal(x.records.length,25);assert.equal(x.seeds.length,6);assert.equal(x.seedRecords.length,4);
  const actions=x.records.filter(r=>r.kind==='action');assert.equal(actions.reduce((s,r)=>s+BigInt(r.body.amount.atoms),0n),80n);
  assert.equal(x.labels.XP1.body.rounded_atoms,String(-BigInt(x.labels.XP1.body.basis.numerator)*20n/100n));
  const detail=readFileSync(join(root,'docs/design/CANONICAL-RECORDS-V1.md'),'utf8');
  for(const v of x.vectors)assert(detail.includes(v.sha256),'addendum omitted digest '+v.name);
  return {status:'passed',language:'Node',vectors:x.vectors.length,accepted_records:25,manifest_members:29,fixture_files:Object.keys(x.files).length,journal_sha256:rawHash(x.files['accepted-records.jsonl']),decision_hash:x.labels.D.content_hash,receipt_record_hash:x.labels.R.content_hash};
}
if(process.argv[1] && resolve(process.argv[1])===resolve(new URL(import.meta.url).pathname))console.log(JSON.stringify(verify(resolve(process.argv[2]||'.'))));
