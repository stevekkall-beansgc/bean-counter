import assert from 'node:assert/strict';
import test from 'node:test';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { canonical, compare_policy, compare_request, create_comparison_session, create_read_session, create_seal_session, decodeIndexKey, digest, encodeIndexKey, percentAtoms, read_prefix, replay, strictParse, validate_shape } from './validate.mjs';

const file=(name)=>strictParse(readFileSync(new URL(name,import.meta.url)));
const minimal=()=>file('./minimal-trace.json');
const customer=()=>file('./customer-trace.json');
const vector=(name)=>file(`./vectors/${name}.json`);
const prefix=(t,n)=>({...t,commands:t.commands.slice(0,n)});
function replaceHash(set,oldHash,newHash) {
  const index=set.indexOf(oldHash);
  assert.notEqual(index,-1);
  set.splice(index,1,newHash);
  set.sort();
}
function resign(t,index,mutate) {
  const c=t.commands[index],old=digest('authority',c.authority);
  mutate(c,t);
  c.authority.command=digest('command',c.kind==='RECEIVE'?[c.kind,c.key,c.payload.submission]:[c.kind,c.key,c.payload]);
  replaceHash(t.initial.authority_observations,old,digest('authority',c.authority));
}
function mutateOriginal(t,kind,mutate) {
  const o=t.initial.original_objects.find(item=>strictParse(Buffer.from(item.body,'base64')).kind===kind);
  assert.ok(o);
  const x=strictParse(Buffer.from(o.body,'base64'));
  mutate(x);
  const profile=x.id?.startsWith('bp2_') || kind==='base-posting'?'2-candidate.4':'1';
  if(profile==='2-candidate.4') x.content_hash='sha256:'+createHash('sha256').update('ledgerlab/record-content/2-candidate.4\0').update(canonical([x.kind,2,x.body])).digest('hex');
  const bytes=Buffer.from(canonical(x));
  o.body=bytes.toString('base64');o.bytes=String(bytes.length);
  o.body_hash=createHash('sha256').update(bytes).digest('hex');o.full_key=o.body_hash;
  t.initial.original_objects.sort((a,b)=>canonical(a)<canonical(b)?-1:canonical(a)>canonical(b)?1:0);
  return t;
}

test('strict JSON and canonical Unicode reject lossy or ambiguous bytes',()=>{
  for(const input of ['{"x":1,"x":2}','{"x":1.0}','{"x":1e0}','{"x":-0}','{"x":9007199254740992}','{"x":"\\ud800"}','{"x":"\\udc00"}','{"x":1,}','[1,]','true false'])
    assert.throws(()=>strictParse(input),input);
  assert.throws(()=>strictParse(Buffer.from([0xef,0xbb,0xbf,0x7b,0x7d])));
  assert.throws(()=>strictParse(Buffer.from([0xc0,0xaf])));
  assert.equal(canonical(strictParse('{"\ue000":2,"\ud83d\ude00":1,"z":0}')),'{"z":0,"😀":1,"":2}');
  assert.equal(canonical(strictParse('{"s":"\\n\\t\\u0000"}')),'{"s":"\\n\\t\\u0000"}');
  assert.throws(()=>canonical({constructor:'unsafe'}));
});

test('all 42 independent binary full-key goldens roundtrip with bounded framing',()=>{
  const rows=file('./index-vectors.json').vectors;
  assert.equal(rows.length,42);
  const tags=new Set(),kinds=new Set();
  for(const row of rows) {
    const bytes=encodeIndexKey(row.kind,row.components);
    assert.equal(bytes.toString('hex'),row.hex,row.kind);
    assert.deepEqual(decodeIndexKey(bytes),{kind:row.kind,components:row.components});
    assert.ok(bytes.length<=1079);
    tags.add(bytes.subarray(0,8).toString('ascii'));kinds.add(row.kind);
    assert.throws(()=>decodeIndexKey(Buffer.concat([bytes,Buffer.from([0])])),{code:'INDEX_TRAILING'});
    assert.throws(()=>encodeIndexKey(row.kind,[...row.components,'extra']),{code:'INDEX_ARITY'});
  }
  assert.equal(kinds.size,14);assert.equal(tags.size,14);
  assert.equal(Math.max(...rows.map(row=>Buffer.from(row.hex,'hex').length)),1079);
  const worksheet=file('./protocol/resources.json');
  assert.equal(worksheet.maximum_key_bytes,1079);
  assert.equal(worksheet.binary_radix_depth,8*1079+1);
  assert.equal(worksheet.pages_per_index_update,8*1079+2);
  assert.equal(Object.values(worksheet.node_layout).reduce((a,b)=>a+b,0),128);
  assert.equal(worksheet.value_page_header_bytes+worksheet.value_page_payload_bytes,4096);
  for(const [kind,limits] of Object.entries(worksheet.key_components)) {
    assert.equal(9+limits.reduce((n,v)=>n+2+v,0)<=1079,true,kind);
  }
});

test('closed shapes enforce conditional nested schemas, amounts and exact evidence bytes',()=>{
  assert.equal(validate_shape('count','0'),true);
  assert.equal(validate_shape('count','00'),false);
  assert.equal(validate_shape('atoms','-0'),false);
  assert.equal(validate_shape('time','2024-02-29T23:59:59.123456Z'),true);
  assert.equal(validate_shape('time','2023-02-29T23:59:59.123456Z'),false);
  assert.equal(validate_shape('scope',['a','b','c']),false);
  assert.equal(validate_shape('evidence',{body:'YQ==',sha256:'0'.repeat(64)}),true);
  assert.equal(validate_shape('evidence',{body:'YQ=',sha256:'0'.repeat(64)}),false);
  const t=minimal();
  t.initial.initial_counters.center={segment:{q:'1',R:'0'}};
  assert.throws(()=>replay(t),{code:'INITIAL_RESERVATION'});
});

test('two-host enrollment reproduces both segment chains and the frozen 80-atom anchor',()=>{
  const x=replay(minimal()),s=x.summary;
  assert.equal(s.root,'897176331b938be83e1b914f3cda3deb29bfb1840bbb54d9ea272e23e5d3e24f');
  assert.equal(s.customer_atoms,'80');
  assert.equal(s.segments,2);
  assert.equal(s.journals.center.ordinal,'1');
  assert.equal(s.journals.g0.ordinal,'1');
  assert.equal(Object.keys(x.snapshot.object_inventory.center).length,33);
  const segment=file('./minimal-segment.json');
  assert.equal(digest('segment',segment),s.journals.center.segment);
  assert.ok(segment.objects.every(o=>o.origin.host==='center' || o.origin.host==='g0'));
});

test('frozen v2 source bodies reject coherently rehashed extra and missing nested fields',()=>{
  for(const mutate of [x=>{x.body.unapproved='x'},x=>{delete x.body.amount}]) {
    const t=mutateOriginal(customer(),'base-posting',mutate);
    assert.throws(()=>replay(prefix(t,6)),{code:'BASE_SCHEMA_SHAPE'});
  }
});

test('frozen v1 conditional and not rules reject rehashed nested violations',()=>{
  const t=mutateOriginal(minimal(),'document',x=>{x.body.unapproved='x'});
  assert.throws(()=>replay(t),{code:'BASE_SCHEMA_SHAPE'});
});

test('each of six center resource dimensions can block ENROLL without a partial central segment',()=>{
  for(const field of ['canonical_bytes','trusted_bytes','records','index_pages','index_values','workspace_bytes']) {
    const t=minimal();t.initial.initial_resources.center[field]='0';
    const s=replay(t).summary;
    assert.equal(s.segments,1,field);
    assert.equal(s.refused,1,field);
    assert.equal(s.journals.center.ordinal,'0',field);
  }
});

test('customer history keeps retail and supplier books separate through fifteen checkpoints',()=>{
  const t=customer(),rows=file('./customer-checkpoints.json');
  assert.equal(rows.length,15);
  const expected=[10000,10000,11200,11200,11700,11700,11700,11700,11700,11800,11650,11650,11450,11450,11450];
  for(let i=0;i<rows.length;i++) {
    const s=replay(prefix(t,rows[i].through)).summary;
    assert.equal(s.customer_atoms,String(expected[i]),rows[i].name);
    assert.equal(s.customer_atoms,rows[i].customer_atoms,rows[i].name);
  }
  const x=replay(t),s=x.summary;
  assert.equal(s.root,'7e37bc9682e1451c8435712ad8a4ea9d7d3e7855686f3a7078f32ba76a4e6b3a');
  assert.equal(s.adjustment_gross,'250');
  assert.equal(s.entitlements,5);
  assert.equal(s.round,'3');
  assert.equal(s.suppliers.length,0);
  assert.equal(Object.keys(s.journals).length,5);
  assert.equal(x.snapshot.certificates.length,3);
  assert.equal(x.snapshot.allocations['close:2'].slots.length,0);
  assert.ok(x.snapshot.allocations['close:3'].slots.length>0);
  assert.ok(x.snapshot.allocations['close:4'].slots.length>0);
  for(const [host,account] of Object.entries(x.snapshot.resources))for(const dimension of Object.keys(account.held)) {
    const sum=Object.values(x.snapshot.allocations).filter(owner=>owner.host===host).reduce((n,owner)=>n+BigInt(owner.held[dimension]),0n);
    assert.equal(String(sum),account.held[dimension],`${host}/${dimension}`);
  }
});

test('published host and delayed seal vectors preserve independent journals',()=>{
  const expected={
    'delayed-seal-observation':'85ef056273821242cd3326aea8edf635b4a471c1cab67f7f30ec488fb630cc4d',
    'independent-host-identity':'2059ad79192b449c7b88bc208d449cd29607c3aadcaf173b0ac38871b599d72d',
    'seal-scan':'d77a34b9a1909a4cd432be208338cb8884d6d4a86566b6bc19d3b7c5527e45aa',
  };
  for(const [name,root] of Object.entries(expected))assert.equal(replay(vector(name)).summary.root,root,name);
  const host=replay(vector('independent-host-identity')).summary;
  assert.equal(host.refused,2);
  assert.equal(host.journals.g0.ordinal,'9');
  assert.equal(host.journals.g1.ordinal,'9');
});

test('typed source membership rejects coherent wrong-key and copied-source attacks',()=>{
  assert.throws(()=>replay(file('./negative-vectors/wrong-key.json')),{code:'PROOF_KIND_KEY'});
  assert.throws(()=>replay(file('./negative-vectors/wrong-source.json')),{code:'PROOF_MEMBERSHIP'});
  const t=customer(),index=t.commands.findIndex(c=>c.kind==='IMPORT');
  resign(t,index,(c,trace)=>{
    const proof=c.payload.proof,old=proof.trusted_observation_ref;
    proof.body_hash='0'.repeat(64);
    const raw={...proof};delete raw.trusted_observation_ref;
    proof.trusted_observation_ref=digest('authority',raw);
    replaceHash(trace.initial.trusted_observations,old,proof.trusted_observation_ref);
  });
  assert.throws(()=>replay(prefix(t,index+1)),{code:'PROOF_MEMBERSHIP'});
});

test('economic role and authority substitutions cannot borrow an observed grant',()=>{
  const t=customer(),index=t.commands.findIndex(c=>c.kind==='DECIDE' && c.payload.path==='ADJUSTMENT');
  assert.ok(index>0);
  const forged=structuredClone(t);
  forged.commands[index].authority.principal='forged-principal';
  assert.throws(()=>replay(prefix(forged,index+1)),{code:'AUTHORITY_OBSERVATION'});
  resign(t,index,c=>{c.payload.roles.payer='vendor'});
  const s=replay(prefix(t,index+1)).summary;
  assert.equal(s.refused,1);
  assert.equal(s.adjustment_gross,'0');
});

test('directional signed rounding and gross budgets match independent boundary rows',()=>{
  const rows=file('./boundary-vectors.json');
  for(const row of rows.arithmetic) {
    const [whole,fraction='']=row.rate_percent.split('.');
    const scale=10n**BigInt(fraction.length);
    const numerator=BigInt(whole)*scale+(fraction?BigInt(whole.startsWith('-')?'-'+fraction:fraction):0n);
    assert.equal(percentAtoms(row.base_atoms,String(numerator),String(scale)),row.rounded);
    assert.equal(String(-BigInt(row.rounded)),row.inverse);
  }
  for(const row of rows.gross)assert.equal(BigInt(row.positive)+BigInt(row.negative)<=BigInt(row.cap),row.allowed);
  for(const row of rows.routes) {
    assert.equal(digest('route',row.case),row.hash);
    assert.equal(Number(BigInt('0x'+row.hash)%4n),row.owner);
  }
});

test('comparison preserves historical/UNKNOWN coverage and returns no failure total',()=>{
  const t=customer(),result=compare_policy(t,{resolution_atoms:'1500'});
  assert.equal(result.status,'COMPARABLE');
  assert.equal(result.actual,'11450');
  assert.equal(result.alternative,'11750');
  assert.equal(result.difference,'300');
  assert.equal(result.supplier_booked,'3000');
  assert.equal(compare_policy(t,{resolution_atoms:'6000'}).status,'POLICY_FAILURE');
  assert.equal(compare_policy(t,{resolution_atoms:'1500',change_roles:true}).status,'UNSUPPORTED');
  const historical=compare_policy(prefix(t,file('./customer-checkpoints.json')[2].through),{resolution_atoms:'1500'});
  assert.equal(historical.status,'COMPARABLE');
  assert.equal(historical.actual,'11200');
  assert.ok(historical.coverage.every(row=>row.status==='UNKNOWN_GATEWAY_COVERAGE'&&Object.keys(row).length===2));
  assert.notEqual(historical.expected.root,result.expected.root);
});

test('closed read and seal shapes cannot attach a successful total to incomplete work',()=>{
  const rows=file('./read-vectors.json');
  for(const o of rows.operations) {
    if(o.kind==='read') {
      assert.equal(validate_shape('read_request',o.request),true,o.name);
      if(o.response)assert.equal(validate_shape('read_response',o.response),true,o.name);
    } else {
      if(o.response)assert.equal(validate_shape('comparison_response',o.response),true,o.name);
    }
  }
  const partial=rows.operations.find(x=>x.name==='partial').response;
  assert.equal(validate_shape('read_response',{...partial,actual:'11450'}),false);
  assert.equal(validate_shape('comparison_response',{status:'INCOMPLETE',expected:partial.cursor.expected,reason:'budget',actual:'11450'}),false);
  assert.equal(validate_shape('coverage',{gateway:'g0',status:'UNKNOWN_GATEWAY_COVERAGE',cutoff:'0'}),false);
  assert.equal(validate_shape('seal_read_response',rows.seal.partial),true);
  assert.equal(validate_shape('seal_read_response',rows.seal.complete),true);
});

test('bounded reader meters exact current, historical, and partial segment bytes',()=>{
  const t=customer(),rows=file('./read-vectors.json');
  for(const o of rows.operations.filter(x=>x.kind==='read')) {
    if(o.response)assert.equal(canonical(read_prefix(t,o.request)),canonical(o.response),o.name);
    else assert.throws(()=>read_prefix(t,o.request),{code:o.error});
  }
});

test('reader continuation is private to one live session and cancellation drops it',()=>{
  const t=customer(),rows=file('./read-vectors.json');
  const partial=rows.operations.find(x=>x.name==='partial');
  const reader=create_read_session(t),other=create_read_session(t);
  const first=reader.read(partial.request);
  assert.equal(first.status,'INCOMPLETE');
  const resume={...partial.request,budget:rows.operations[0].request.budget,cursor:first.cursor};
  assert.throws(()=>read_prefix(t,resume),{code:'READ_CURSOR_SESSION'});
  assert.throws(()=>other.read(resume),{code:'READ_CURSOR_SESSION'});
  assert.equal(reader.read(resume).status,'COMPLETE');
  assert.throws(()=>reader.read(resume),{code:'READ_CURSOR_SESSION'});
  const once=reader.read(partial.request);
  reader.cancel();
  assert.throws(()=>reader.read({...resume,cursor:once.cursor}),{code:'READ_CURSOR_SESSION'});
});

test('local seal scan pages every canonical fragment and aborts without a root',()=>{
  const row=file('./read-vectors.json').seal;
  const source=file(`./${row.source_trace}`),t=prefix(source,Number(row.through));
  const fresh=()=>create_seal_session(t,row.gateway,row.round);
  assert.equal(canonical(fresh().scan(row.partial_budget)),canonical(row.partial));
  assert.equal(canonical(fresh().scan({bytes:'999999',pages:'999999'})),canonical(row.complete));
  const resumed=fresh();let state=resumed.scan(row.partial_budget),steps=0;
  while(state.status==='INCOMPLETE' && steps++<100) {
    assert.equal(Object.hasOwn(state,'disposition_root'),false);
    state=resumed.scan(row.resume_budget,state.cursor);
  }
  assert.equal(state.status,'COMPLETE');
  assert.equal(state.disposition_root,row.complete.disposition_root);
  assert.equal(state.receipt_root,row.complete.receipt_root);
  const aborted=fresh();aborted.scan(row.partial_budget);aborted.abort();
  assert.throws(()=>aborted.scan(row.resume_budget),{code:row.abort_error});
});

test('comparison folds charged central segments and authenticates certificate coverage',()=>{
  const t=customer(),rows=file('./read-vectors.json');
  for(const o of rows.operations.filter(x=>x.kind==='compare')) {
    if(o.response)assert.equal(canonical(compare_request(t,o.request)),canonical(o.response),o.name);
    else assert.throws(()=>compare_request(t,o.request),{code:o.error});
  }
});

test('comparison continuation keeps policy, coverage, prefix and private fold state fixed',()=>{
  const t=customer(),v=file('./read-vectors.json').comparison_resume;
  const s=create_comparison_session(t);
  const first=s.compare(v.request);
  assert.equal(canonical(first),canonical(v.first));
  const changed=structuredClone(v.resume_request);
  changed.policy.resolution_atoms='1600';
  assert.throws(()=>s.compare(changed),{code:'COMPARISON_SESSION'});
  assert.equal(canonical(s.compare(v.resume_request)),canonical(v.final));
  assert.throws(()=>s.compare(v.resume_request),{code:'READ_CURSOR_SESSION'});
  const canceled=create_comparison_session(t);
  canceled.compare(v.request);canceled.cancel();
  assert.throws(()=>canceled.compare(v.resume_request),{code:'READ_CURSOR_SESSION'});
});

test('alternating gateway rounds retain independent installed predecessors and immutable routing',()=>{
  const a=replay(vector('alternating-subset-rounds'));
  assert.equal(a.summary.round,'3');
  assert.equal(a.summary.refused,0);
  assert.equal(a.snapshot.round_preparations['["2","g1"]'].predecessor,'0');
  const p=replay(vector('permuted-preparation-order'));
  assert.equal(p.summary.refused,0);
  assert.equal(p.summary.cases,4);
});

test('historical supplier releases follow only the last explicit family close',()=>{
  const checkpoints=file('./regression-checkpoints.json').close_prefixes;
  assert.equal(checkpoints[0].suppliers[0].held,'170');
  for(const row of checkpoints) {
    const t=file('./'+row.trace),x=replay(prefix(t,row.through));
    assert.equal(canonical(x.summary.suppliers),canonical(row.suppliers),row.trace);
    assert.equal(digest('closure',x.snapshot.certificates.at(-1)),row.certificate,row.trace);
  }
  const saved=replay(vector('REG02-saved-close-unimported'));
  assert.deepEqual(saved.snapshot.certificates[0].supplier_before,[]);
  assert.deepEqual(saved.snapshot.certificates[0].supplier_after,[]);
  assert.equal(saved.summary.duplicates,1);
});

test('initial writer epoch must match trusted capability and cannot skip a predecessor',()=>{
  const t=minimal();
  t.initial.writer_capabilities.g0.epoch='2';
  assert.throws(()=>replay(t),{code:'WRITER_CAPABILITY'});
  const x=minimal();
  x.initial.initial_counters.g0.writer_epoch={q:'0',R:'0'};
  assert.throws(()=>replay(x),{code:'INITIAL_RESERVATION'});
});
