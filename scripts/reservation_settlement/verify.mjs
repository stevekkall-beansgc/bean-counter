// Independent Node/BigInt byte and numeric oracle. No Python or production core imports.
import fs from 'node:fs';
import crypto from 'node:crypto';
import {fileURLToPath} from 'node:url';
const root=fileURLToPath(new URL('../../',import.meta.url));
const dir=root+'contracts/candidates/reservation-settlement-v1/';
const kinds=['reservation-observation','reservation-transition','reservation-receipt'];
const prefixes=['rso1_','rst1_','rsr1_'];
const MAX=10n**30n-1n, REV=2n**63n-1n;
function must(ok,why){if(!ok)throw Error(why);}
function jcs(x,depth=0){
 must(depth<=32,'depth');
 if(typeof x==='string') {must(!/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(x),'surrogate');return JSON.stringify(x);}
 if(typeof x==='boolean')return JSON.stringify(x);
 if(typeof x==='number'){must(Number.isSafeInteger(x)&&!Object.is(x,-0),'number');return String(x);}
 if(Array.isArray(x))return '['+x.map(y=>jcs(y,depth+1)).join(',')+']';
 must(x!==null&&typeof x==='object','scalar');
 return '{'+Object.keys(x).sort().map(k=>jcs(k,depth+1)+':'+jcs(x[k],depth+1)).join(',')+'}';
}
const eq=(a,b)=>jcs(a)===jcs(b);
const sort=a=>a.toSorted((a,b)=>Buffer.compare(Buffer.from(jcs(a)),Buffer.from(jcs(b))));
const hash=(domain,x)=>'sha256:'+crypto.createHash('sha256').update('ledgerlab/'+domain+'/reservation-settlement/1\0').update(jcs(x)).digest('hex');
const ref=r=>({kind:r.kind,id:r.id,content_hash:r.content_hash});
const uint=(s,max=MAX)=>{must(typeof s==='string'&&/^(0|[1-9][0-9]*)$/u.test(s)&&!s.endsWith('\n'),'uint spelling');let n=BigInt(s);must(n<=max,'uint bound');return n;};
function fields(o,required,optional=[]){must(o&&typeof o==='object'&&!Array.isArray(o),'object');must(required.every(k=>Object.hasOwn(o,k))&&Object.keys(o).every(k=>required.includes(k)||optional.includes(k)),'closed fields');}
function timestamp(s){must(typeof s==='string'&&/^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{6}Z$/u.test(s)&&!s.endsWith('\n'),'time');const d=new Date(s);must(Number.isFinite(d.getTime())&&d.toISOString().slice(0,19)===s.slice(0,19)&&s.slice(0,4)!=='0000','date');return s;}
function txt(s,n=128,source=false){must(typeof s==='string'&&Buffer.byteLength(s)>0&&Buffer.byteLength(s)<=n&&!/\p{Cc}/u.test(s),'text');if(source)must(/^[A-Za-z][A-Za-z0-9+.-]*:/u.test(s)&&!(/\p{White_Space}/u.test(s)),'source');}
function synthetic(kind,label){const p={'base-acceptance':'ba2_','binding-snapshot':'bs2_','target-snapshot':'ts2_','evidence':'ed2_','receipt':'rc2_'};let h=hash('synthetic-input',[kind,label]).slice(7);return {kind,id:p[kind]+h,content_hash:'sha256:'+h};}
function state(s){
 fields(s,['revision','maximum','consumed','held','released','families']);uint(s.revision,REV);
 must(uint(s.maximum)===uint(s.consumed)+uint(s.held)+uint(s.released),'conservation');
 must(Array.isArray(s.families)&&s.families.length>0&&s.families.length<=32&&eq(s.families,sort(s.families)),'members');
 must(new Set(s.families.map(f=>jcs(f.key))).size===s.families.length,'duplicate family');
 for(const f of s.families){fields(f,['key','accepted_by','status'],['ordinary_receipt']);fields(f.key,['agreement_id','family_id','target']);txt(f.key.agreement_id);txt(f.key.family_id);timestamp(f.accepted_by);must(['open','claimed','closed'].includes(f.status),'status');must((f.status==='claimed')===Object.hasOwn(f,'ordinary_receipt'),'ordinary receipt');}
}
function rowIntegrity(r){
 fields(r,['kind','scope','id','body','content_hash']);const index=kinds.indexOf(r.kind);must(index>=0,'kind');
 must(Array.isArray(r.scope)&&r.scope.length===2,'scope');r.scope.forEach(s=>txt(s));
 const b=r.body;must(b.schema==='ledger-'+r.kind+'/reservation-settlement/1','schema');
 let key;if(index===0)key=[r.scope,b.command.source,b.command.external_id];else if(index===1)key=[r.scope,b.invocation_id,b.after.revision];else key=[r.scope,b.source,b.external_id];
 must(r.id===prefixes[index]+hash(r.kind,key).slice(7),'identity');must(r.content_hash===hash('record-content',[r.kind,1,b]),'content hash');must(Buffer.byteLength(jcs(b))<=256*1024,'body bytes');
}
const recordKey=(scope,r)=>jcs([scope,r.kind,r.id]);
function retainReference(registry,scope,r){const key=recordKey(scope,r);must(!registry.has(key)||eq(registry.get(key),r),'reference identity conflict');registry.set(key,r);}
function* referencesIn(value){if(Array.isArray(value)){for(const x of value)yield* referencesIn(x);}else if(value&&typeof value==='object'){if(eq(Object.keys(value).sort(),['content_hash','id','kind']))yield value;else for(const x of Object.values(value))yield* referencesIn(x);}}
function originalIngress(input,c){if(c.kind==='close')return jcs(c);const x={};for(const key of ['kind','id','family','amount'])if(Object.hasOwn(input,key))x[key]=input[key];return jcs(x);}
function lookupDelivery(deliveries,scope,c,ingress,mayRead,absenceConfirmed){if(!mayRead)return {status:'unauthorized'};const stored=deliveries.get(jcs([scope,c.source,c.external_id]));if(!stored)return {status:absenceConfirmed?'absent':'outcome_unknown'};if(stored.command!==jcs(c)||stored.ingress!==ingress)return {status:'identity_conflict'};return {status:'duplicate',composite:structuredClone(stored.composite)};}
function validate(h,v,deliveriesOut=new Map()){
 must(h.name===v.name&&h.steps.length===v.steps.length,'history');
 const anchor={};for(const [key,kind] of [['base_acceptance','base-acceptance'],['binding_snapshot','binding-snapshot'],['target_snapshot','target-snapshot'],['invocation_authorization','evidence']])anchor[key]=synthetic(kind,h.name+':'+key);
 const grant=synthetic('evidence',h.name+':grant'),evidence=synthetic('evidence',h.name+':authority-evidence');
 const target='ev2_'+hash('synthetic-target',h.name).slice(7);
 const key=f=>({agreement_id:'supplier-agreement',family_id:f,target});
 let current,previous,registration,lastTime;const prior=new Map(),live=new Map(),original=new Map(),references=new Map();const deliveries=deliveriesOut,journalIdentities=new Set();let count=0;
 h.steps.forEach((input,i)=>{
  const r=v.steps[i].records;count+=r.length;must(r.length===2||r.length===3,'record set');must(eq(r.map(x=>x.kind),r.length===2?[kinds[0],kinds[2]]:kinds),'record order');
  r.forEach(rowIntegrity);must(eq(v.steps[i].canonical_utf8,r.map(x=>jcs(x))),'canonical bytes');must(r.reduce((n,x)=>n+Buffer.byteLength(jcs(x)),0)<=4*1024*1024,'decision bytes');
  r.forEach(x=>must(eq(x.scope,h.scope??['synthetic','sandbox']),'scope'));
  const o=r[0].body,c=o.command,rc=r.at(-1).body;
  fields(o,['schema','command','request_hash','anchor','unit','before','authority','received_at','accepted_at'],['registration','previous','economic_receipt']);
  fields(c,['kind','source','external_id','invocation_id',...(c.kind==='close'?['reason','expected_revision']:['economic_ingress_hash']),...(['ordinary','post_hoc'].includes(c.kind)?['family']:[])]);
  fields(rc,['schema','source','external_id','invocation_id','request_hash','observation','result','replay'],['transition','economic_receipt']);
  const scope=r[0].scope,delivery=jcs([scope,c.source,c.external_id]);must(!deliveries.has(delivery),'delivery identity reused');
  for(const item of r){const identity=recordKey(scope,item);must(!journalIdentities.has(identity),'record identity reused');journalIdentities.add(identity);retainReference(references,scope,ref(item));for(const dependency of referencesIn(item.body))retainReference(references,scope,dependency);}
  must(c.kind===input.kind&&c.external_id===input.id&&c.invocation_id==='invocation-one'&&c.source==='urn:synthetic:settlement','command');txt(c.source,256,true);txt(c.external_id);txt(c.invocation_id);
  must(o.request_hash===hash('request',c),'request hash');must(eq(o.anchor,anchor),'anchor');must(eq(o.unit,h.unit??{currency:'USD',scale:2}),'unit');
  const now=timestamp(o.accepted_at);must(timestamp(o.received_at)<=now&&(!lastTime||lastTime<=now),'time order');lastTime=now;
  const a=o.authority;fields(a,['principal','grant','grant_revision','active','permissions','evidence']);txt(a.principal);must(eq(a.grant,grant)&&eq(a.evidence,[evidence]),'authority evidence');must(a.active===true&&uint(a.grant_revision,REV)>0n,'authority');must(a.permissions.includes('read')&&a.permissions.includes({register:'submit',ordinary:'submit',post_hoc:'correct',close:'close'}[c.kind]),'permission');must(eq(a.permissions,[...new Set(a.permissions)].sort()),'permissions set');
  state(o.before);
  if(i===0){must(c.kind==='register'&&!o.registration&&!o.previous,'registration');let max=uint(h.maximum),base=uint(h.base_consumed),release=uint(h.base_released);must(base+release<=max&&uint(h.premium_ceiling)<=max-base-release,'base capacity');current={revision:'0',maximum:h.maximum,consumed:h.base_consumed,held:String(max-base-release),released:h.base_released,families:sort(h.families.map(f=>({key:key(f.id),accepted_by:f.accepted_by,status:'open'})))};must(eq(current,o.before),'base checkpoint');}
  else{must(c.kind!=='register'&&eq(o.registration,registration)&&eq(o.previous,previous),'prefix');must(eq(current,o.before),'current head');}
  const result=structuredClone(current);let consume=0n,release=0n;
  const economic=c.kind==='close'?undefined:synthetic('receipt',h.name+':'+input.id);
  if(['ordinary','post_hoc'].includes(c.kind)){
   must(eq(c.family,key(input.family)),'family');const f=result.families.find(x=>eq(x.key,c.family));must(f,'member');
   must(typeof input.amount==='string'&&/^(0|-?[1-9][0-9]*)$/u.test(input.amount)&&!input.amount.endsWith('\n'),'amount');let amt=BigInt(input.amount);must(amt<=MAX&&amt>=-MAX,'amount bound');
   const proposed=new Map(live);proposed.set(input.family,amt);let positive=0n,negative=0n;for(const amount of proposed.values()){if(amount>0n)positive+=amount;else negative-=amount;}must(positive<=uint(h.premium_ceiling)&&negative<=uint(h.base_consumed),'economic ceiling');
   if(c.kind==='ordinary'){must(f.status==='open','ordinary closed');must(now<=timestamp(f.accepted_by),'ordinary deadline');consume=amt>0n?amt:0n;must(consume<=uint(current.held),'held');f.status='claimed';f.ordinary_receipt=economic;original.set(input.family,economic);}
   else must(f.status==='claimed'&&eq(f.ordinary_receipt,original.get(input.family)),'post-hoc original');
   live.clear();for(const [k,v] of proposed)live.set(k,v);
  }else if(c.kind==='close'){
   must(uint(c.expected_revision,REV)===uint(current.revision,REV),'expected revision');
   if(c.reason==='deadline')must(now>current.families.map(f=>timestamp(f.accepted_by)).sort().at(-1),'closure deadline');else must(c.reason==='authorized'&&input.early_authorized===true,'early closure authority');
   for(const f of result.families)if(f.status==='open')f.status='closed';release=uint(current.held);
  }
  result.consumed=String(uint(current.consumed)+consume);result.held=String(uint(current.held)-consume-release);result.released=String(uint(current.released)+release);result.families=sort(result.families);
  const changed=!eq(result,current);if(changed){must(uint(current.revision,REV)<REV,'revision exhausted');result.revision=String(uint(current.revision,REV)+1n);}
  must((r.length===3)===changed,'transition presence');
  if(changed){const t=r[1].body;fields(t,['schema','invocation_id','observation','before','after','consume','release']);must(t.invocation_id===c.invocation_id&&eq(t.observation,ref(r[0])),'transition reference');must(eq(t.before,current)&&eq(t.after,result)&&t.consume===String(consume)&&t.release===String(release),'transition');must(eq(rc.transition,ref(r[1])),'receipt transition');}else must(!Object.hasOwn(rc,'transition'),'no-op transition');
  state(rc.result);must(eq(rc.result,result),'result');must(eq(rc.observation,ref(r[0])),'receipt observation');for(const k of ['source','external_id','invocation_id'])must(rc[k]===c[k],'receipt command');must(rc.request_hash===o.request_hash,'receipt hash');
  if(c.kind==='close')must(!Object.hasOwn(rc,'economic_receipt')&&!Object.hasOwn(o,'economic_receipt'),'close receipt');else {const ingress={};for(const k of ['kind','id','family','amount'])if(Object.hasOwn(input,k))ingress[k]=input[k];must(c.economic_ingress_hash===hash('synthetic-ingress',ingress),'economic ingress');must(eq(o.economic_receipt,economic)&&eq(rc.economic_receipt,economic),'economic receipt');}
  const external=[...Object.values(anchor),grant,evidence,...(economic?[economic]:[])];const replay=new Map(prior);for(const x of [...external,...r.slice(0,-1).map(ref)])retainReference(replay,scope,x);must(rc.replay.length<=1024&&eq(rc.replay,sort([...replay.values()])),'replay closure');
  prior.clear();for(const [k,v] of replay)prior.set(k,v);retainReference(prior,scope,ref(r.at(-1)));const composite={settlement_receipt_utf8:jcs(r.at(-1))};if(Object.hasOwn(rc,'economic_receipt'))composite.economic_receipt=rc.economic_receipt;deliveries.set(delivery,{command:jcs(c),ingress:originalIngress(input,c),composite});previous=ref(r.at(-1));registration??=previous;current=result;
 });return count;
}
function validateLookups(histories,vectors,cases){for(const item of cases){const i=histories.findIndex(h=>h.name===item.history),h=histories[i],v=vectors[i],end=item.after_step+1,deliveries=new Map();if(end)validate({...h,steps:h.steps.slice(0,end)},{...v,steps:v.steps.slice(0,end)},deliveries);const before=jcs([...deliveries]);const c=structuredClone(v.steps[item.request_step].records[0].body.command);let ingress=originalIngress(h.steps[item.request_step],c);Object.assign(c,item.command_patch??{});if(Object.hasOwn(item,'ingress_override'))ingress=item.ingress_override;const reply=lookupDelivery(deliveries,h.scope??['synthetic','sandbox'],c,ingress,item.may_read,item.absence_confirmed);must(reply.status===item.expected,'lookup status '+item.name);if(item.expected==='duplicate'){const original=v.steps[item.original_step].records.at(-1),composite={settlement_receipt_utf8:jcs(original)};if(Object.hasOwn(original.body,'economic_receipt'))composite.economic_receipt=original.body.economic_receipt;must(eq(reply,{status:'duplicate',composite}),'original composite '+item.name);}else must(eq(Object.keys(reply),['status']),'disclosure');must(jcs([...deliveries])===before,'lookup appended');}return cases.length;}
if(process.argv.includes('--attacks')){
 const cases=JSON.parse(fs.readFileSync(0,'utf8'));let count=0;
 for(const item of cases){for(const step of item.vector.steps)step.records.forEach(rowIntegrity);let rejected=false;try{validate(item.history,item.vector);}catch{rejected=true;}must(rejected,'accepted attack '+item.name);count++;}
 console.log(JSON.stringify({status:'passed',independent:'Node rehashed adversarial semantics',cases:count}));
}else{
 const histories=JSON.parse(fs.readFileSync(dir+'histories.json','utf8')).histories,vectors=JSON.parse(fs.readFileSync(dir+'vectors.json','utf8'));
 const lookups=validateLookups(histories,vectors,JSON.parse(fs.readFileSync(dir+'lookup-cases.json','utf8')));
 let records=0;histories.forEach((h,i)=>records+=validate(h,vectors[i]));
 must(jcs({'\uE000':1,'😀':2})==='{"😀":2,"":1}','UTF16 key order');
 console.log(JSON.stringify({status:'passed',independent:'Node BigInt arithmetic and canonical hashes',histories:histories.length,steps:histories.reduce((n,h)=>n+h.steps.length,0),records,non_appending_lookup_cases:lookups}));
}
