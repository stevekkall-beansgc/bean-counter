// Independent candidate byte/hash and structured-key reconstruction. No Python,
// Rust, computed vectors or expected digests supply hash/identity preimages.
import {createHash} from 'node:crypto';
import {readFileSync,readdirSync} from 'node:fs';
import {join} from 'node:path';
import assert from 'node:assert/strict';
const root=process.argv[2]||'.', dir=join(root,'contracts/candidates/v2/goldens');
const prefix={'evidence':'ed','policy-snapshot':'po','event':'ev','base-posting':'bp','target-basis':'tb','admission':'ad','claim':'cl','claim-revision':'rv','effect':'ef','action':'ac','obligation':'ob','limit-evidence':'li','explanation':'xp','replay-input':'rp','intention':'in','decision-manifest':'dc','receipt':'rc','binding-snapshot':'bs','base-evaluation':'be','target-snapshot':'ts','base-acceptance':'ba','authority-decision':'au'};
function jcs(x) {
  if(Array.isArray(x)) return '['+x.map(jcs).join(',')+']';
  if(x!==null && typeof x==='object') return '{'+Object.keys(x).sort().map(k=>JSON.stringify(k)+':'+jcs(x[k])).join(',')+'}';
  assert(x!==null && ['string','boolean','number'].includes(typeof x));
  if(typeof x==='number') assert(Number.isSafeInteger(x)&&!Object.is(x,-0));
  return JSON.stringify(x);
}
const raw=b=>createHash('sha256').update(b).digest('hex');
const hash=(d,v)=>raw(Buffer.concat([Buffer.from(`ledgerlab/${d}/2-candidate.3`),Buffer.from([0]),Buffer.from(jcs(v))]));
const originalHash=(d,v)=>raw(Buffer.concat([Buffer.from(`ledgerlab/${d}/1`),Buffer.from([0]),Buffer.from(jcs(v))]));
const id=(k,v)=>prefix[k]+'2_'+hash(k,v);
const cmp=(a,b)=>Buffer.compare(Buffer.from(a),Buffer.from(b));
const ref=r=>({kind:r.kind,id:r.id,content_hash:r.content_hash});
const order=(a,b)=>cmp(a.kind,b.kind)||cmp(jcs(a.id),jcs(b.id));
function derive(r) {
  const {kind:k,scope:s,body:b}=r;
  switch(k) {
    case 'evidence': case 'policy-snapshot': case 'target-basis': case 'replay-input': case 'binding-snapshot': case 'base-evaluation': case 'target-snapshot': return id(k,[s,b]);
    case 'event': return id(k,[s,b.data.source,b.data.external_id]);
    case 'base-posting': return id(k,[s,b.event_id,b.agreement_id,b.book,b.ordinal]);
    case 'claim': return id(k,[s,b.agreement_id,b.family_id,b.target]);
    case 'claim-revision': return id(k,[b.claim_id,b.number]);
    case 'effect': return id(k,[b.claim_id,b.revision_id,b.slot]);
    case 'action': return id(k,[b.effect_id]);
    case 'obligation': return id(k,[s,b.agreement_id,b.book,b.currency,b.scale,b.roles]);
    case 'authority-decision': case 'admission': case 'decision-manifest': case 'receipt': return id(k,[b.event_id]);
    case 'limit-evidence': return id(k,[b.event_id,0]);
    case 'explanation': return id(k,[b.event_id,b.ordinal]);
    case 'base-acceptance': return id(k,[b.target]);
    case 'intention': return id(k,[s,b.destination,b.obligation_id,b.action_ids]);
    case 'link': return [s,'outcome_of',b.event_id,b.target];
    case 'dependency': return [s,b.dependent,b.input.kind,b.input.id];
    case 'delivery-key': return [s,b.source,b.external_id];
    case 'chain-revision': return [s,b.chain_id,b.number];
    default: throw new Error('unknown record kind '+k);
  }
}
let rows=0, decisions=0;
const custom=process.argv[3]==='--histories';
const entries=custom?JSON.parse(readFileSync(process.argv[4])).map((history,i)=>['rehashed-'+i,history]):readdirSync(dir).filter(n=>!['vectors.json','inventory.json'].includes(n)).map(n=>{const raw=readFileSync(join(dir,n));const h=JSON.parse(raw);assert.equal(jcs(h),raw.toString('utf8'),'noncanonical file');return [n,h];});
for(const [filename,history] of entries) {
  const known=new Map();
  for(const group of [history.seed,...history.decisions.map(d=>d.records)]) {
    assert.deepEqual([...group].sort(order),group);
    for(const r of group) {
      assert.deepEqual(derive(r),r.id,`${filename}: ${r.kind} identity`);
      const actual='sha256:'+hash(r.kind==='decision-manifest'?'decision-content':'record-content',r.kind==='decision-manifest'?r.body:[r.kind,2,r.body]);
      assert.equal(actual,r.content_hash);
      const k=jcs([r.kind,r.id]); assert(!known.has(k),'duplicate record identity'); known.set(k,r); rows++;
      if(r.kind==='effect') {
        const a=group.find(a=>a.kind==='action'&&a.id===r.body.action_id).body;
        const f=Object.fromEntries(Object.entries(a).filter(([k])=>!['schema','event_id','effect_id','policy_snapshot'].includes(k)));
        assert.equal(r.body.facts_hash,'sha256:'+hash('effect-facts',f));
      }
      if(r.kind==='delivery-key') assert.equal(r.body.ingress_hash,'sha256:'+hash('ingress',r.body.ingress));
    }
  }
  function references(x) {
    if(Array.isArray(x)){for(const v of x)references(v);return;}
    if(x!==null&&typeof x==='object'){
      if(Object.keys(x).sort().join(',')==='content_hash,id,kind') assert.deepEqual(ref(known.get(jcs([x.kind,x.id]))),x,'retained reference');
      else for(const v of Object.values(x))references(v);
    }
  }
  for(const r of known.values()) {
    references(r.body);
    for(const field of ['utf8','evaluation_utf8','binding_utf8','policy_utf8','original_event_utf8','original_ingress_utf8']) {
      if(field in r.body) assert.equal(jcs(JSON.parse(r.body[field])),r.body[field],'lossless canonical source');
    }
    if(r.kind==='evidence') {
      const b=r.body,h=originalHash('document',[b.document_type,b.document_version,JSON.parse(b.utf8)]);
      assert.equal(b.document_id,'doc_'+h);assert.equal(b.document_hash,'sha256:'+h);
    }
    if(r.kind==='base-evaluation') {
      const b=r.body,e=JSON.parse(b.original_event_utf8),ingress=JSON.parse(b.original_ingress_utf8);
      const source=known.get(jcs(['event',b.event_id])).body.data.source;
      assert.equal(b.original_event_id,'ev_'+originalHash('event',[...r.scope,source,e.id]));
      assert.equal(b.original_event_hash,'sha256:'+originalHash('event-content',e));
      assert.equal(b.original_ingress_hash,'sha256:'+originalHash('ingress',ingress));
    }
    if(r.kind==='base-acceptance') {
      const b=r.body;
      assert.deepEqual(b.members,history.seed.filter(r=>r.kind!=='base-acceptance').map(ref).sort(order));
      const receipt={schema:'ledger-base-receipt/2-candidate.3',target:b.target,base_evaluation:b.base_evaluation,target_snapshot:b.target_snapshot,accepted_at:b.accepted_at,membership_hash:'sha256:'+hash('base-membership',b.members)};
      assert.equal(b.original_receipt_utf8,jcs(receipt));
    }
  }
  for(const d of history.decisions) {
    const m=d.records.find(r=>r.kind==='decision-manifest'),r=d.records.find(r=>r.kind==='receipt');
    const replay=d.records.find(r=>r.kind==='replay-input');
    const membership=new Map([...d.records.filter(r=>!['decision-manifest','receipt'].includes(r.kind)).map(ref),...replay.body.inputs].map(r=>[jcs([r.kind,r.id]),r]));
    assert.deepEqual([...membership.values()].sort(order),m.body.members,'complete manifest');
    for(const v of m.body.members) assert.deepEqual(ref(known.get(jcs([v.kind,v.id]))),v);
    assert.equal(r.body.decision_hash,m.content_hash);
    assert.equal(r.body.event_hash,known.get(jcs(['event',r.body.event_id])).content_hash);
    assert.equal(d.receipt_utf8,jcs(r.body),'original receipt bytes'); decisions++;
  }
  for(const p of history.probes) if(p.kind.endsWith('_retry')) assert.equal(p.original_receipt_utf8,history.decisions[p.step].receipt_utf8);
}
// UTF-16 (astral before BMP private-use), control escaping and no normalization.
assert.equal(jcs({'\ue000':1,'😀':2}),'{"😀":2,"":1}');
assert.notEqual(hash('event',['é']),hash('event',['e\u0301']));
console.log(JSON.stringify({status:'passed',independent:custom?'Node fully rehashed attack integrity':'Node byte/hash/key reconstruction',records:rows,decisions}));
