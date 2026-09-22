import { createHash } from 'node:crypto';
import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const SCHEMA = strictParse(readFileSync(new URL('./protocol/schema.json', import.meta.url)));
const DEFS = SCHEMA.$defs;
const ORIGINAL_V1 = strictParse(readFileSync(new URL('../../schemas/v1/canonical-records.schema.json',import.meta.url)));
const ORIGINAL_V2 = strictParse(readFileSync(new URL('../v2/schemas/canonical-records.schema.json',import.meta.url)));
const resourceUrl = new URL('./protocol/resources.json',import.meta.url);
const RESOURCE = existsSync(resourceUrl) ? strictParse(readFileSync(resourceUrl)) : null;
const INDEX_LIMITS = RESOURCE?.key_components??{};
const INDEX_TAGS = Object.fromEntries(Object.keys(INDEX_LIMITS).map(kind=>[kind,kind.toUpperCase().slice(0,8).padEnd(8,'_')]));
if(new Set(Object.values(INDEX_TAGS)).size!==Object.keys(INDEX_TAGS).length) throw new Error('INDEX_TAG_COLLISION');
const ZERO = '0'.repeat(64);
const M = 10n ** 30n - 1n;
const encoder = new TextEncoder();
const domains = new Set(['command','submission','grant','claim','receipt','action','closure','segment','replay','route','namespace','authority','evidence','result','enrollment']);
const denial = (code) => { throw Object.assign(new Error(code), { code }); };
const bigint = (v) => BigInt(v);
const keyOf = (v) => canonical(v);
const same = (a,b) => keyOf(a) === keyOf(b);

// The parser intentionally precedes JSON.parse: duplicate keys and noncanonical
// numeric tokens must not disappear before validation gets to inspect them.
export function strictParse(input) {
  const bytes = Buffer.isBuffer(input) ? input : Buffer.from(input);
  const source = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
  if (source.charCodeAt(0) === 0xfeff) denial('BOM');
  let at = 0;
  const ws = () => { while (/\s/.test(source[at] ?? '') && at < source.length) {
    if (!' \t\r\n'.includes(source[at])) denial('JSON_WHITESPACE'); at++;
  }};
  function str() {
    const start = at++;
    while (at < source.length) {
      const c = source[at++];
      if (c === '"') {
        const raw = source.slice(start, at);
        let value;
        try { value = JSON.parse(raw); } catch { denial('JSON_STRING'); }
        assertUnicode(value);
        return value;
      }
      if (c === '\\') { if (at >= source.length) denial('JSON_STRING'); at++; }
      else if (c.charCodeAt(0) < 32) denial('JSON_STRING');
    }
    denial('JSON_STRING');
  }
  function value() {
    ws();
    const c = source[at];
    if (c === '"') return str();
    if (c === '[') {
      at++; ws(); const out = [];
      if (source[at] === ']') { at++; return out; }
      for (;;) { out.push(value()); ws(); if (source[at] === ']') { at++; return out; }
        if (source[at++] !== ',') denial('JSON_ARRAY'); }
    }
    if (c === '{') {
      at++; ws(); const out = Object.create(null); const seen = new Set();
      if (source[at] === '}') { at++; return out; }
      for (;;) {
        ws(); if (source[at] !== '"') denial('JSON_OBJECT');
        const k = str(); if (seen.has(k)) denial('DUPLICATE_KEY'); seen.add(k);
        ws(); if (source[at++] !== ':') denial('JSON_OBJECT');
        out[k] = value(); ws(); if (source[at] === '}') { at++; return out; }
        if (source[at++] !== ',') denial('JSON_OBJECT');
      }
    }
    for (const [word, val] of [['true',true],['false',false],['null',null]]) {
      if (source.startsWith(word, at)) { at += word.length; return val; }
    }
    const tail = source.slice(at);
    const n = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(tail)?.[0];
    if (!n) denial('JSON_TOKEN');
    if (n === '-0' || n.includes('.') || /[eE]/.test(n)) denial('JSON_NUMBER');
    const v = Number(n); if (!Number.isSafeInteger(v)) denial('JSON_NUMBER');
    at += n.length; return v;
  }
  const out = value(); ws(); if (at !== source.length) denial('JSON_TRAILING');
  return out;
}

function assertUnicode(s) {
  for (let i=0; i<s.length; i++) {
    const c=s.charCodeAt(i);
    if (c>=0xd800 && c<=0xdbff) { if (i+1>=s.length || s.charCodeAt(++i)<0xdc00 || s.charCodeAt(i)>0xdfff) denial('SURROGATE'); }
    else if (c>=0xdc00 && c<=0xdfff) denial('SURROGATE');
  }
}
export function canonical(value) {
  if (value === null || typeof value === 'boolean') return JSON.stringify(value);
  if (typeof value === 'number') { if (!Number.isSafeInteger(value) || Object.is(value,-0)) denial('CANONICAL_NUMBER'); return String(value); }
  if (typeof value === 'string') { assertUnicode(value); return JSON.stringify(value); }
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']';
  if (typeof value === 'object' && value) {
    const keys=Object.keys(value).sort();
    for (const k of keys) { assertUnicode(k); if (['__proto__','prototype','constructor'].includes(k)) denial('UNSAFE_KEY'); }
    return '{' + keys.map(k=>canonical(k)+':'+canonical(value[k])).join(',') + '}';
  }
  denial('CANONICAL_TYPE');
}
export function digest(domain, value) {
  if (!domains.has(domain)) denial('HASH_DOMAIN');
  const h=createHash('sha256'); h.update(`ledgerlab/central-r3/${domain}/1`); h.update(Buffer.from([0])); h.update(canonical(value));
  return h.digest('hex');
}
export function encodeIndexKey(kind,components) {
  const limits=INDEX_LIMITS[kind];
  if(!limits || !Array.isArray(components) || components.length!==limits.length) denial('INDEX_ARITY');
  const parts=[Buffer.from(INDEX_TAGS[kind],'ascii'),Buffer.from([limits.length])];
  for(let i=0;i<limits.length;i++) {
    if(typeof components[i]!=='string') denial('INDEX_COMPONENT');
    assertUnicode(components[i]);
    const bytes=Buffer.from(components[i],'utf8');
    if(bytes.length>limits[i] || bytes.length>65535) denial('INDEX_COMPONENT_LENGTH');
    const length=Buffer.alloc(2);length.writeUInt16BE(bytes.length);
    parts.push(length,bytes);
  }
  const encoded=Buffer.concat(parts);
  if(encoded.length>RESOURCE.maximum_key_bytes) denial('INDEX_MAXIMUM');
  return encoded;
}
export function decodeIndexKey(bytes) {
  const b=Buffer.from(bytes);
  if(b.length<9) denial('INDEX_FRAME');
  const tag=b.subarray(0,8).toString('ascii');
  const kind=Object.keys(INDEX_TAGS).find(k=>INDEX_TAGS[k]===tag),limits=INDEX_LIMITS[kind];
  if(!limits || b[8]!==limits.length) denial('INDEX_TAG_ARITY');
  let at=9;const parts=[];
  for(let i=0;i<limits.length;i++) {
    if(at+2>b.length) denial('INDEX_FRAME');
    const len=b.readUInt16BE(at);at+=2;
    if(len>limits[i] || at+len>b.length) denial('INDEX_COMPONENT_LENGTH');
    const value=new TextDecoder('utf-8',{fatal:true}).decode(b.subarray(at,at+len));at+=len;
    assertUnicode(value);parts.push(value);
  }
  if(at!==b.length || !encodeIndexKey(kind,parts).equals(b)) denial('INDEX_TRAILING');
  return {kind,components:parts};
}
function calendarTime(s) {
  const m=/^(\d{4})-(\d\d)-(\d\d)T(\d\d):(\d\d):(\d\d)\.(\d{6})Z$/.exec(s);
  if (!m) return false;
  const [year,month,day,hour,minute,second]=m.slice(1,7).map(Number);
  if (!year || month<1 || month>12 || hour>23 || minute>59 || second>59) return false;
  const days=[31,(year%4===0 && (year%100!==0 || year%400===0))?29:28,31,30,31,30,31,31,30,31,30,31];
  return day>=1 && day<=days[month-1];
}
function isObject(x) { return x!==null && typeof x==='object' && !Array.isArray(x); }
function scalarValid(kind,value) {
  if(kind==='text' || kind==='source') {
    if(typeof value!=='string' || !value || /\p{Cc}/u.test(value)) return false;
    return kind==='text' || (/^[A-Za-z][A-Za-z0-9+.-]*:/u.test(value) && !/\p{White_Space}/u.test(value));
  }
  if(kind==='slug') return typeof value==='string' && /^[a-z][a-z0-9_.-]{0,63}$/u.test(value);
  if(['decimal','positive-decimal','decimal-percent'].includes(kind)) {
    if(typeof value!=='string' || !/^(?:0|[1-9][0-9]*)(?:\.[0-9]*[1-9])?$/u.test(value)) return false;
    const [whole,fraction='']=value.split('.');
    if(fraction.length>18 || (whole+fraction).replace(/^0+/u,'').length>30) return false;
    if(kind==='positive-decimal') return value!=='0';
    return kind!=='decimal-percent' || BigInt(whole+fraction)<=100n*10n**BigInt(fraction.length);
  }
  if(['atoms','nonnegative-atoms','uint'].includes(kind)) {
    if(typeof value!=='string' || !/^(?:0|-?[1-9][0-9]*)$/u.test(value)) return false;
    const n=BigInt(value);
    if(kind==='uint') return n>=0n && n<=9223372036854775807n;
    return (kind==='atoms' || n>=0n) && n>=-(10n**30n-1n) && n<=10n**30n-1n;
  }
  if(kind==='time') return typeof value==='string' && calendarTime(value);
  if(kind==='ratio') {
    if(!isObject(value) || typeof value.numerator!=='string' || typeof value.denominator!=='string' || !/^(?:0|-?[1-9][0-9]*)$/u.test(value.numerator) || !/^[1-9][0-9]*$/u.test(value.denominator)) return false;
    const n=BigInt(value.numerator),d=BigInt(value.denominator);
    if((n<0n?-n:n).toString(2).length>512 || d.toString(2).length>512) return false;
    let a=n<0n?-n:n,b=d;
    while(b!==0n) [a,b]=[b,a%b];
    return a===1n;
  }
  if(kind==='nonnegative-money') return isObject(value) && scalarValid('nonnegative-atoms',value.atoms);
  denial('SCHEMA_SCALAR');
}
function shape(schema, value, defs=DEFS) {
  if(schema===false) return false;
  if(schema===true) return true;
  if (schema.$ref) {
    const target=defs[schema.$ref.slice('#/$defs/'.length)];
    if(!target) denial('SCHEMA_REFERENCE');
    return shape(target,value,defs);
  }
  if (schema.oneOf && schema.oneOf.filter(s=>shape(s,value,defs)).length!==1) return false;
  if (schema.anyOf && !schema.anyOf.some(s=>shape(s,value,defs))) return false;
  if (schema.allOf && !schema.allOf.every(s=>shape(s,value,defs))) return false;
  if (schema.not && shape(schema.not,value,defs)) return false;
  if (schema.if && !shape(shape(schema.if,value,defs)?schema.then??true:schema.else??true,value,defs)) return false;
  if (Object.hasOwn(schema,'const') && !same(schema.const,value)) return false;
  if (schema.enum && !schema.enum.some(x=>same(x,value))) return false;
  if(schema['x-scalar'] && !scalarValid(schema['x-scalar'],value)) return false;
  if(isObject(value)) {
    const keys=Object.keys(value);
    if(schema.minProperties!==undefined && keys.length<schema.minProperties) return false;
    if(schema.maxProperties!==undefined && keys.length>schema.maxProperties) return false;
    if(schema.required?.some(k=>!Object.hasOwn(value,k))) return false;
    if(schema.additionalProperties===false && keys.some(k=>!Object.hasOwn(schema.properties??{},k))) return false;
    if(schema.dependentRequired && Object.entries(schema.dependentRequired).some(([k,required])=>Object.hasOwn(value,k) && required.some(x=>!Object.hasOwn(value,x)))) return false;
    for(const k of keys) {
      if(['__proto__','prototype','constructor'].includes(k)) return false;
      if(Object.hasOwn(schema.properties??{},k) && !shape(schema.properties[k],value[k],defs)) return false;
      if(!Object.hasOwn(schema.properties??{},k) && typeof schema.additionalProperties==='object' && !shape(schema.additionalProperties,value[k],defs)) return false;
    }
  }
  if(schema.type===undefined && Array.isArray(value)) {
    if(schema.minItems!==undefined && value.length<schema.minItems) return false;
    if(schema.maxItems!==undefined && value.length>schema.maxItems) return false;
    if(schema.prefixItems && value.some((v,i)=>i<schema.prefixItems.length && !shape(schema.prefixItems[i],v,defs))) return false;
    if(schema.items===false && value.length>(schema.prefixItems?.length??0)) return false;
    if(schema.items && value.some((v,i)=>i>=(schema.prefixItems?.length??0) && !shape(schema.items,v,defs))) return false;
    if(schema.uniqueItems && new Set(value.map(keyOf)).size!==value.length) return false;
  }
  if(schema.type===undefined && typeof value==='string') {
    if(schema.minLength!==undefined && [...value].length<schema.minLength) return false;
    if(schema.maxLength!==undefined && [...value].length>schema.maxLength) return false;
    if(schema.pattern && !(new RegExp(schema.pattern,'u')).test(value)) return false;
  }
  if(schema.type===undefined && typeof value==='number' && ((schema.minimum!==undefined && value<schema.minimum) || (schema.maximum!==undefined && value>schema.maximum))) return false;
  if(Array.isArray(schema.type)) return schema.type.some(type=>shape({...schema,type},value,defs));
  if(schema['x-canonical-maxBytes']!==undefined && encoder.encode(canonical(value)).length>schema['x-canonical-maxBytes']) return false;
  if (schema.type==='integer') return Number.isSafeInteger(value) && !Object.is(value,-0) && (schema.minimum===undefined || value>=schema.minimum) && (schema.maximum===undefined || value<=schema.maximum);
  if (schema.type==='boolean') return typeof value==='boolean';
  if (schema.type==='string') {
    if (typeof value!=='string') return false;
    try { assertUnicode(value); } catch { return false; }
    if (schema.minLength!==undefined && [...value].length<schema.minLength) return false;
    if (schema.maxLength!==undefined && [...value].length>schema.maxLength) return false;
    if (schema['x-max-utf8']!==undefined && encoder.encode(value).length>schema['x-max-utf8']) return false;
    if (schema['x-utf8-maxBytes']!==undefined && encoder.encode(value).length>schema['x-utf8-maxBytes']) return false;
    if (schema.pattern && !(new RegExp(schema.pattern,'u')).test(value)) return false;
    if (schema.format==='date-time' && !calendarTime(value)) return false;
    if (schema['x-scalar']==='time' && !calendarTime(value)) return false;
    if(schema['x-canonicalSchema']) {
      let embedded;
      try { embedded=strictParse(Buffer.from(value)); if(canonical(embedded)!==value) return false; } catch { return false; }
      const target=defs[schema['x-canonicalSchema']];
      if(!target) denial('SCHEMA_EMBEDDED_REFERENCE');
      if(!shape(target,embedded,defs)) return false;
    }
    if (schema['x-base64-bytes']!==undefined) {
      if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) return false;
      const b=Buffer.from(value,'base64'); if (b.length>schema['x-base64-bytes'] || b.toString('base64')!==value) return false;
    }
    return true;
  }
  if (schema.type==='array') {
    if (!Array.isArray(value) || (schema.minItems!==undefined && value.length<schema.minItems) || (schema.maxItems!==undefined && value.length>schema.maxItems)) return false;
    if (schema.prefixItems && value.some((v,i)=>i<schema.prefixItems.length && !shape(schema.prefixItems[i],v,defs))) return false;
    if (schema.items===false && value.length>(schema.prefixItems?.length??0)) return false;
    if (schema.items && value.some((v,i)=>i>=(schema.prefixItems?.length??0) && !shape(schema.items,v,defs))) return false;
    if (schema.uniqueItems) { const set=new Set(value.map(keyOf)); if (set.size!==value.length) return false; }
    return true;
  }
  if (schema.type==='object') return isObject(value);
  return true;
}
export function validate_shape(type_name,value) {
  if (!Object.hasOwn(DEFS,type_name)) denial('SHAPE_TYPE');
  if(!shape(DEFS[type_name],value)) return false;
  if(type_name==='expected_prefix' && (value.ordinal==='0') !== (value.segment===ZERO && value.root===ZERO)) return false;
  if(type_name==='read_request' && value.cursor && !same(value.cursor.expected,value.expected)) return false;
  if(type_name==='retry_response') {
    if(value.knowledge==='UNKNOWN' && (Object.hasOwn(value,'prefix') || value.current_lifecycle!=='UNKNOWN' || value.central_admission!=='UNKNOWN' || value.coverage.some(x=>x.status!=='UNKNOWN_GATEWAY_COVERAGE'))) return false;
    if(value.knowledge==='CACHED_VERIFIED_PREFIX' && (!Object.hasOwn(value,'prefix') || value.current_lifecycle!=='UNKNOWN')) return false;
    if(value.knowledge==='AUTHORITATIVE_AT_PREFIX' && !Object.hasOwn(value,'prefix')) return false;
    if(value.coverage.some((x,i)=>i>0 && canonical(value.coverage[i-1])>=canonical(x))) return false;
  }
  if(type_name==='comparison_response' && value.status==='COMPARABLE' && bigint(value.alternative)-bigint(value.actual)!==bigint(value.difference)) return false;
  return true;
}

// Replay and economics continue below. Every operation is checked against the
// current in-memory prefix; this is a contract simulator, not a store proof.
class Refusal extends Error { constructor(code) { super(code); this.code=code; } }
const refuse=(code)=>{ throw new Refusal(code); };
function commandDigest(c) { return digest('command',c.kind==='RECEIVE'?[c.kind,c.key,c.payload.submission]:[c.kind,c.key,c.payload]); }
function requireShape(name,v) { if (!validate_shape(name,v)) denial(`SHAPE_${name.toUpperCase()}`); }
function requireRecord(v,fields) { if (!isObject(v) || Object.keys(v).some(k=>!fields.includes(k)) || fields.some(k=>!Object.hasOwn(v,k))) denial('TRACE_SHAPE'); }
function sortedSet(xs) { for(let i=1;i<xs.length;i++) if (canonical(xs[i-1])>=canonical(xs[i])) denial('UNSORTED_SET'); }
function checkSetOrder(c) {
  const p=c.payload;
  if(c.kind==='ENROLL') {
    sortedSet(p.preparations);
    sortedSet(p.suppliers); sortedSet(p.pools);
    for(const f of p.families) { sortedSet(f.correction_atoms); sortedSet(f.prerequisites); }
  }
  if(c.kind==='BEGIN') { sortedSet(p.families); sortedSet(p.gateways); sortedSet(p.preparations); }
  if(c.kind==='RECEIVE') sortedSet(p.submission.evidence);
  if(c.kind==='SUPPLEMENT') sortedSet(p.evidence);
}
function scopeKey(v) { return canonical(v); }
function familyKey(v) { return canonical(v); }
function caseKey(v) { return canonical(v); }
function resourceVector(v) {
  requireShape('resource',v);
  return Object.fromEntries(Object.entries(v).map(([k,n])=>[k,bigint(n)]));
}
function checkVectorPositive(v) { for(const n of Object.values(v)) if (n<0n || n>M) denial('RESOURCE_RANGE'); }
const dims=['canonical_bytes','trusted_bytes','records','index_pages','index_values','workspace_bytes'];
const vectorOf=(row,bundle=false)=>Object.fromEntries([
  ['canonical_bytes',BigInt(bundle?row.retained.segment_bytes:row.segment_bytes)],
  ['trusted_bytes',BigInt(bundle?row.retained.new_trusted_bytes:row.new_trusted_bytes)],
  ['records',BigInt(bundle?row.retained.records:row.records)],
  ['index_pages',BigInt(bundle?row.retained.index_path_pages:row.index_path_pages)],
  ['index_values',BigInt(bundle?row.retained.index_value_pages:row.index_value_pages)],
  ['workspace_bytes',BigInt(bundle?row.peak_workspace:row.logical_workspace_bytes)],
]);
function template(kind,bundle=false) {
  if(!RESOURCE || RESOURCE.format!=='r3-resource-worksheet/1') denial('RESOURCE_SHEET');
  const row=(bundle?RESOURCE.bundles:RESOURCE.transitions)[kind];
  if(!row) denial('RESOURCE_TEMPLATE');
  return {vector:vectorOf(row,bundle),counters:Object.fromEntries(Object.entries(row.counters??row.counter_increments).map(([k,v])=>[k,BigInt(v)])),slots:bundle?[...row.slots]:[kind]};
}
function heldTotal(account,k) { let n=0n; for(const o of account.held.values()) n+=o.vector[k]; return n; }
function counterPair(s,host,name) {
  if(!s.counters.has(host)) s.counters.set(host,new Map());
  const c=s.counters.get(host);
  if(!c.has(name)) c.set(name,{q:0n,R:0n});
  return c.get(name);
}
function reserve(s,host,owner,t) {
  const a=s.resources.get(host); if(!a || a.held.has(owner)) refuse('RESOURCE_OWNER');
  for(const k of dims) if(a.used[k]+heldTotal(a,k)+t.vector[k]>a.provisioned[k]) refuse('RESOURCE_SHORT');
  for(const [k,n] of Object.entries(t.counters)) { const c=counterPair(s,host,k); if(c.q+c.R+n>M) refuse('COUNTER_SHORT'); }
  a.held.set(owner,{vector:{...t.vector},counters:{...t.counters},slots:[...t.slots],active:true});
  for(const [k,n] of Object.entries(t.counters)) counterPair(s,host,k).R+=n;
}
function actualIndex(s,c) {
  const p=c.payload,kind=c.kind;
  let extra;
  if(kind==='PREPARE_ENROLL') extra=37;
  else if(kind==='PREPARE_ROUND') extra=5;
  else if(kind==='ENROLL') extra=s.originalObjects.length+p.preparations.length+p.families.length+2*p.gateways.length+p.suppliers.length+p.pools.length+32+2;
  else if(['LOCAL_GRANT','REGISTER_GRANT'].includes(kind)) extra=2;
  else if(kind==='ISSUE') extra=4;
  else if(kind==='ACTIVATE') extra=2;
  else if(kind==='RECEIVE') extra=getToken(s,p.token).disposition==='NEW_CASE'?3:1;
  else if(kind==='RETURN_UNUSED') extra=2;
  else if(kind==='IMPORT') extra=getToken(s,p.token).disposition==='NEW_CASE'?3:2;
  else if(['RECONCILE','ADVANCE','ADVANCE_RECEIPT','LOCAL_TERMINAL','RETIRE_GRANT','SEAL_BEGIN','SEALED','DRAIN','READY','ABORT','INSTALL'].includes(kind)) extra=1;
  else if(kind==='ACK_INSTALL') extra=2;
  else if(kind==='BEGIN') extra=p.mode==='CANCELLABLE'?1+p.preparations.length:1;
  else if(kind==='CLOSE') {
    const r=s.round;
    const transitions=[...s.suppliers.values()].filter(x=>r.families.some(k=>getFamily(s,k).supplier_pool===x.id) && ![...s.family.values()].some(f=>f.supplier_pool===x.id && !f.closed)).length;
    extra=2+r.families.length+transitions;
  }
  else if(['SUPPLEMENT','REPLACE_WRITER','EXTEND_RESOURCES'].includes(kind)) extra=2;
  else if(kind==='DECIDE') extra=p.verdict==='DENY'?2:5+2*s.currentEffects.filter(e=>e.kind==='ACTION').length;
  else if(kind==='CORRECT') extra=3+s.currentEffects.filter(e=>e.kind==='ACTION').length;
  else denial('INDEX_KIND');
  const introductions=s.pendingIntroductions??0;
  if(kind==='ENROLL' || kind==='PREPARE_ENROLL' || kind==='PREPARE_ROUND') return BigInt(4+extra+(s.pendingAuthorityIntroductions??0));
  return BigInt(4+extra+introductions);
}
function spend(s,host,owner,kind,c) {
  const a=s.resources.get(host),o=a?.held.get(owner); if(!o?.active) refuse('RESOURCE_OWNER');
  const t=template(kind);
  const slot=o.slots.indexOf(kind);
  if(slot<0) refuse('RESOURCE_SLOT');
  for(const k of dims) if(k!=='workspace_bytes' && o.vector[k]<t.vector[k]) refuse('RESOURCE_BRANCH');
  if(o.vector.workspace_bytes<t.vector.workspace_bytes) refuse('WORKSPACE_SHORT');
  for(const [k,n] of Object.entries(t.counters)) if((o.counters[k]??0n)<n) refuse('COUNTER_BRANCH');
  for(const k of dims) if(k!=='workspace_bytes') { o.vector[k]-=t.vector[k]; a.used[k]+=t.vector[k]; }
  for(const [k,n] of Object.entries(t.counters)) {
    let actual=n;
    if(kind==='RECEIVE' && k==='receipt' && getToken(s,c.payload.token).disposition==='ALIAS') actual=0n;
    if(kind==='DECIDE' && k==='economic_revision' && c.payload.verdict==='DENY') actual=0n;
    if(k==='index_cardinality') actual=actualIndex(s,c);
    if(actual>n) denial('INDEX_RESERVATION_SHORT');
    o.counters[k]-=n;
    const pair=counterPair(s,host,k); pair.R-=n; pair.q+=actual;
    if(pair.q+pair.R>M) denial('COUNTER_INVARIANT');
  }
  o.slots.splice(slot,1);
}
function release(s,host,owner) {
  const a=s.resources.get(host),o=a?.held.get(owner); if(!o?.active) refuse('RESOURCE_OWNER');
  for(const [k,n] of Object.entries(o.counters)) counterPair(s,host,k).R-=n;
  o.vector=Object.fromEntries(dims.map(k=>[k,0n]));
  o.counters=Object.fromEntries(Object.keys(o.counters).map(k=>[k,0n]));
  o.slots=[]; o.active=false;
}
function charge(s,host,owner,kind,c) {
  owner=`command:${canonical([host,c.key])}`;
  reserve(s,host,owner,template(kind)); spend(s,host,owner,kind,c); release(s,host,owner);
}
function finishRoundResources(s,r) {
  const center=s.enrollment.store;
  const owner=r.mode==='CANCELLABLE'?`round:center:${r.id}`:`protected:center:${r.resourceIndex}`;
  if(s.resources.get(center)?.held.get(owner)?.active) release(s,center,owner);
}
function initialState(trace) {
  requireRecord(trace,['format','initial','commands']);
  if (trace.format!=='r3-trace/1' || !Array.isArray(trace.commands)) denial('TRACE_FORMAT');
  const i=trace.initial;
  const initialFields=['authority_documents','authority_sources','authority_observations','grant_authentications','original_base_receipt','original_base_manifest','initial_resources','initial_counters','writer_capabilities','original_objects','trusted_observations'];
  if(!isObject(i) || Object.keys(i).some(k=>!initialFields.includes(k)) || initialFields.some(k=>!Object.hasOwn(i,k))) denial('TRACE_INITIAL');
  if (!Array.isArray(i.authority_documents) || !Array.isArray(i.authority_sources) || !Array.isArray(i.grant_authentications) || !isObject(i.initial_resources) || !isObject(i.initial_counters) || !isObject(i.writer_capabilities)) denial('TRACE_INITIAL');
  for (const x of [...i.authority_documents,...i.grant_authentications,i.original_base_receipt,i.original_base_manifest]) requireShape('digest',x);
  if(!Array.isArray(i.original_objects) || i.original_objects.length>128) denial('ORIGINAL_OBJECTS');
  if(!Array.isArray(i.trusted_observations)) denial('TRUSTED_OBSERVATIONS');
  let originalBytes=0n;
  for(const o of i.original_objects) { requireShape('object',o); checkObject(o); if(o.kind!=='ORIGINAL_BASE' || o.full_key!==o.body_hash) denial('ORIGINAL_OBJECTS'); originalBytes+=bigint(o.bytes); }
  if(originalBytes>1048576n) denial('ORIGINAL_OBJECTS');
  for(const x of i.trusted_observations) requireShape('digest',x);
  sortedSet(i.trusted_observations);
  sortedSet(i.authority_documents);
  sortedSet(i.authority_sources);
  const authoritySources=new Map(),authorityIdentities=new Map();
  for(const record of i.authority_sources) {
    if(!isObject(record) || typeof record.body!=='string' || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(record.body) || Buffer.from(record.body,'base64').toString('base64')!==record.body) denial('BASE64');
    requireShape('authority_source',record);
    const raw=Buffer.from(record.body,'base64'),body=strictParse(raw),encoded=Buffer.from(canonical(body));
    if(!raw.equals(encoded) || raw.length>16384 || String(raw.length)!==record.bytes || createHash('sha256').update(raw).digest('hex')!==record.body_hash || !validate_shape('authority_source_body',body)) denial('AUTH_SOURCE_BODY');
    const identity=canonical([body.source,body.id,body.revision]);
    if(authorityIdentities.has(identity) || authoritySources.has(record.body_hash)) denial('AUTH_SOURCE_IDENTITY');
    authorityIdentities.set(identity,record.body_hash);
    authoritySources.set(record.body_hash,{record,body,identity});
  }
  if(!same([...authoritySources.keys()].sort(),[...i.authority_documents].sort())) denial('AUTH_SOURCE_MEMBERSHIP');
  sortedSet(i.grant_authentications);
  sortedSet(i.original_objects);
  if(!Array.isArray(i.authority_observations)) denial('AUTHORITY_OBSERVATIONS');
  for(const x of i.authority_observations) requireShape('digest',x);
  sortedSet(i.authority_observations);
  const resources=new Map();
  for (const [host,v] of Object.entries(i.initial_resources)) { requireShape('id',host); const vec=resourceVector(v); checkVectorPositive(vec); resources.set(host,{provisioned:vec,used:Object.fromEntries(Object.keys(vec).map(k=>[k,0n])),held:new Map()}); }
  const counters=new Map();
  for(const [host,obj] of Object.entries(i.initial_counters)) {
    requireShape('id',host); if (!isObject(obj)) denial('TRACE_COUNTERS');
    const cs=new Map();
    for(const [name,pair] of Object.entries(obj)) { if (!SCHEMA['x-counters'].includes(name) || !isObject(pair) || Object.keys(pair).sort().join(',')!=='R,q') denial('TRACE_COUNTERS');
      requireShape('count',pair.q); requireShape('count',pair.R); const q=bigint(pair.q),R=bigint(pair.R); if(R!==0n || (name==='writer_epoch'?q!==1n:q!==0n)) denial('INITIAL_RESERVATION'); if(q+R>M) denial('COUNTER_HEADROOM'); cs.set(name,{q,R}); }
    counters.set(host,cs);
  }
  const writers=new Map();
  for(const [gateway,cap] of Object.entries(i.writer_capabilities)) { requireShape('id',gateway); if(!isObject(cap) || Object.keys(cap).sort().join(',')!=='epoch,fence,journal_head') denial('WRITER_CAPABILITY'); requireShape('count',cap.epoch); requireShape('digest',cap.journal_head); requireShape('digest',cap.fence); if(cap.epoch!=='1' || cap.journal_head!==ZERO || cap.fence===ZERO || counters.get(gateway)?.get('writer_epoch')?.q!==1n) denial('WRITER_CAPABILITY'); writers.set(gateway,{...cap}); }
  return {journals:new Map(),segments:0,duplicates:0,refused:0,authority:new Set(i.authority_documents),authoritySources,authorityRetained:new Map(),authorityObs:new Set(i.authority_observations),grantAuth:new Set(i.grant_authentications),trustedObs:new Set(i.trusted_observations),originalObjects:i.original_objects,baseReceipt:i.original_base_receipt,baseManifest:i.original_base_manifest,
    resources,counters,writers,enrollment:null,preparations:new Map(),roundPreparations:new Map(),objectInventory:new Map(),localClaims:new Map(),localBegins:new Map(),localInstalled:new Map(),gatewayAck:new Map(),localDispositionBytes:new Map(),localReceiptBytes:new Map(),gateways:[],family:new Map(),suppliers:new Map(),pools:new Map(),grants:new Map(),tokens:new Map(),cases:new Map(),deliveries:new Map(),commands:new Map(),round:null,lastRound:0n,customer:0n,premiumUsed:0n,gross:0n,entitlements:new Map(),actions:[],certificates:[],allocationPrefix:new Map(),receiptPrefix:new Map(),allocationNext:new Map(),receiptNext:new Map(),clockFloor:new Map(),coverage:new Map()};
}
function permissionFor(c,s) {
  if (c.kind==='DECIDE') return c.payload.path==='ADJUSTMENT'?'adjust':'decide';
  if (c.kind==='ENROLL') return 'enroll';
  if (['PREPARE_ENROLL','PREPARE_ROUND','LOCAL_GRANT','REGISTER_GRANT','ISSUE','ACTIVATE','RETURN_UNUSED','IMPORT','RECONCILE','ADVANCE','ADVANCE_RECEIPT','RETIRE_GRANT','LOCAL_TERMINAL','SEAL_BEGIN','SEALED','DRAIN','READY','INSTALL','ACK_INSTALL','EXTEND_RESOURCES'].includes(c.kind)) return 'capacity';
  if (['RECEIVE','SUPPLEMENT'].includes(c.kind)) return 'submit';
  if (['BEGIN','CLOSE','ABORT'].includes(c.kind)) return 'close';
  if(c.kind==='CORRECT') return 'correct';
  if(c.kind==='REPLACE_WRITER') return 'replace';
  denial('COMMAND_KIND');
}
const localKinds=new Set(['PREPARE_ENROLL','PREPARE_ROUND','LOCAL_GRANT','ACTIVATE','RECEIVE','RETURN_UNUSED','LOCAL_TERMINAL','SEAL_BEGIN','SEALED','INSTALL','REPLACE_WRITER']);
function hostFor(c,s) {
  if(c.kind==='EXTEND_RESOURCES') return c.payload.host;
  if(!localKinds.has(c.kind)) return c.kind==='ENROLL'?c.payload.store:s.enrollment?.store;
  return c.kind==='LOCAL_GRANT'?c.payload.grant.gateway:c.payload.gateway;
}
function journal(s,host) {
  if(!host) refuse('NOT_ENROLLED');
  if(!s.journals.has(host)) s.journals.set(host,{root:ZERO,previous:ZERO,ordinal:0n,segments:new Map(),bytes:new Map(),byOrdinal:new Map()});
  return s.journals.get(host);
}
function authoritySource(s,hash,kind) {
  const found=s.authoritySources.get(hash);
  if(!found || (kind && found.body.kind!==kind)) denial('AUTH_SOURCE_MEMBERSHIP');
  return found;
}
function sourceContext(body,scope,target) {
  if(!same(body.scope,scope) || body.target!==target) denial('AUTH_SOURCE_SCOPE');
}
function checkAuthority(c,s,duplicate,host) {
  const a=c.authority, h=commandDigest(c);
  if(a.command!==h || !s.authority.has(a.document)) denial('AUTHORITY_INTEGRITY');
  if(!s.authorityObs.has(digest('authority',a))) denial('AUTHORITY_OBSERVATION');
  if(duplicate) { if(a.permission!=='read') denial('RETRY_READ_AUTHORITY'); }
  else if(a.permission!==permissionFor(c,s)) denial('AUTHORITY_PERMISSION');
  const body=authoritySource(s,a.document,'AUTHORIZATION').body;
  const scope=c.kind==='ENROLL'||c.kind==='PREPARE_ENROLL'?c.payload.scope:s.enrollment?.scope;
  if(!scope || !same(c.key[0],scope)) denial('AUTH_SOURCE_SCOPE');
  if(c.kind==='PREPARE_ENROLL') { if(!same(body.scope,scope)) denial('AUTH_SOURCE_SCOPE'); }
  else sourceContext(body,scope,c.kind==='ENROLL'?c.payload.target:s.enrollment?.target);
  if(body.principal!==a.principal || body.revision!==a.revision || !body.permissions.includes(a.permission)) denial('AUTH_SOURCE_BINDING');
  if(a.observed_at<body.starts_at || a.observed_at>=body.ends_at) denial('AUTH_SOURCE_WINDOW');
  if(!duplicate && a.head!==journal(s,host).root) denial('AUTHORITY_HEAD');
}
function getFamily(s,k) { const f=s.family.get(familyKey(k)); if(!f) refuse('UNKNOWN_FAMILY'); return f; }
function getGateway(s,id) { const g=s.gateways.find(x=>x.gateway===id); if(!g) refuse('UNKNOWN_GATEWAY'); return g; }
function writer(s,id) { const w=s.writers.get(id); if(!w || w.fence===ZERO || w.journal_head!==journal(s,id).root) refuse('WRITER_FENCE'); return w; }
function getToken(s,id) { const t=s.tokens.get(id); if(!t) refuse('UNKNOWN_TOKEN'); return t; }
function getGrant(s,id) { const g=s.grants.get(id); if(!g) refuse('UNKNOWN_GRANT'); return g; }
function routeIndex(s,caseTuple) { return Number(bigint('0x'+digest('route',caseTuple)) % BigInt(s.gateways.length)); }
function checkNamespace(g,delivery) {
  if (!same(g.scope,delivery[0])) return false;
  const prefix=`gw1.${g.tag}.`;
  const external=delivery[2], suffix=external.startsWith(prefix)?external.slice(prefix.length):'';
  return suffix.length>0 && encoder.encode(suffix).length<=91;
}
function evidenceGood(e) { for(const x of e) if(createHash('sha256').update(Buffer.from(x.body,'base64')).digest('hex')!==x.sha256) denial('EVIDENCE_HASH'); }
function checkObject(o) {
  const bytes=Buffer.from(o.body,'base64'), text=new TextDecoder('utf-8',{fatal:true,ignoreBOM:true}).decode(bytes);
  if(canonical(strictParse(bytes))!==text || BigInt(o.bytes)!==BigInt(bytes.length) || createHash('sha256').update(bytes).digest('hex')!==o.body_hash) denial('OBJECT_BYTES');
}
function originalBase(s,p) {
  const facts=s.originalObjects.map(o=>({hash:o.body_hash,value:strictParse(Buffer.from(o.body,'base64'))}));
  const receipt=facts.find(x=>x.hash===p.base_receipt)?.value,manifest=facts.find(x=>x.hash===p.base_manifest)?.value;
  const old=receipt?.kind==='receipt' && manifest?.kind==='decision-manifest';
  const newer=receipt?.kind==='base-acceptance' && manifest?.kind==='target-snapshot';
  if(!old && !newer) denial('BASE_COMPANIONS');
  const profile=newer?'2-candidate.4':'1';
  const refs=new Map();
  function v2Identity(x) {
    const b=x.body,scope=x.scope;
    const fixed={evidence:'ed2_', 'policy-snapshot':'po2_', 'target-basis':'tb2_', 'binding-snapshot':'bs2_', 'base-evaluation':'be2_', 'target-snapshot':'ts2_'};
    let prefix=fixed[x.kind],input;
    if(prefix) input=[scope,b];
    else {
      const identities={
        'base-identity':['bi2_',[scope,b.target,b.original_kind,b.original_id]],
        event:['ev2_',[scope,b.data?.source,b.data?.external_id]],
        'base-posting':['bp2_',[scope,b.event_id,b.agreement_id,b.book,b.ordinal]],
        obligation:['ob2_',[scope,b.agreement_id,b.book,b.currency,b.scale,b.roles]],
        'base-acceptance':['ba2_',[b.target]],
      };
      [prefix,input]=identities[x.kind]??[];
    }
    if(!prefix || input.some(v=>v===undefined)) denial('BASE_KIND');
    const id=prefix+createHash('sha256').update(`ledgerlab/${x.kind}/2-candidate.4\0`).update(canonical(input)).digest('hex');
    if(x.id!==id) denial('BASE_ID');
  }
  function embedded(text) {
    if(typeof text!=='string') denial('BASE_EMBEDDED');
    if(canonical(strictParse(Buffer.from(text)))!==text) denial('BASE_EMBEDDED');
  }
  function total(rows) {
    let retail=0n,supplier=0n;
    for(const x of rows) {
      const amount=x.body?.amount;
      if(!isObject(amount) || amount.currency!=='USD' || amount.scale!==2 || typeof amount.atoms!=='string' || !/^(?:0|-?[1-9][0-9]*)$/.test(amount.atoms)) denial('BASE_ACTION_AMOUNT');
      if(x.body.book==='retail') retail+=bigint(amount.atoms);
      else if(x.body.book==='supplier') supplier+=bigint(amount.atoms);
      else denial('BASE_ACTION_BOOK');
    }
    if(retail!==bigint(p.base_atoms) || supplier!==bigint(p.supplier_booked)) denial('BASE_BOOKS');
  }
  for(const {value:x} of facts) {
    const originalSchema=newer?ORIGINAL_V2:ORIGINAL_V1;
    if(!shape(originalSchema,x,originalSchema.$defs)) denial('BASE_SCHEMA_SHAPE');
    if(!isObject(x) || !same(x.scope,p.scope) || typeof x.kind!=='string' || !isObject(x.body) || typeof x.content_hash!=='string' || !Object.keys(x).every(k=>['kind','scope','id','body','content_hash',...(x.kind==='document'?['document_type']:[])].includes(k))) denial('BASE_SCOPE');
    if(x.kind!=='document' && x.body.schema!==`ledger-${x.kind}/${profile}`) denial('BASE_SCHEMA');
    const domain=x.kind==='decision-manifest'?'decision-content':x.kind==='document'?'document':x.kind==='event' && old?'event-content':'record-content';
    const input=x.kind==='decision-manifest' || (x.kind==='event' && old)?x.body:x.kind==='document'?[x.document_type,1,x.body]:[x.kind,newer?2:1,x.body];
    const expected='sha256:'+createHash('sha256').update(`ledgerlab/${domain}/${profile}\0`).update(canonical(input)).digest('hex');
    if(x.content_hash!==expected) denial('BASE_CONTENT_HASH');
    if(newer) v2Identity(x);
    const key=canonical([x.kind,x.id]);
    if(refs.has(key)) denial('BASE_DUPLICATE_RECORD');
    refs.set(key,x);
    for(const [k,v] of Object.entries(x.body)) if(k.endsWith('_utf8')) embedded(v);
  }
  if(newer) {
    if(!Array.isArray(receipt.body.members) || receipt.body.members.length!==facts.length-1) denial('BASE_MEMBERSHIP');
    const memberKeys=new Set();
    for(const m of receipt.body.members) {
      const key=canonical([m.kind,m.id]);
      if(memberKeys.has(key) || refs.get(key)?.content_hash!==m.content_hash) denial('BASE_MEMBERSHIP');
      memberKeys.add(key);
    }
    for(const x of refs.values()) if(x!==receipt && !memberKeys.has(canonical([x.kind,x.id]))) denial('BASE_MEMBERSHIP');
    function checkRefs(v) {
      if(!v || typeof v!=='object') return;
      if(!Array.isArray(v) && typeof v.kind==='string' && Object.hasOwn(v,'id') && typeof v.content_hash==='string' && refs.get(canonical([v.kind,v.id]))?.content_hash!==v.content_hash) denial('BASE_REFERENCE');
      for(const child of Object.values(v)) checkRefs(child);
    }
    for(const x of refs.values()) checkRefs(x.body);
    if(!same(receipt.body.target_snapshot,{kind:manifest.kind,id:manifest.id,content_hash:manifest.content_hash})) denial('BASE_MANIFEST_REF');
    const embeddedReceipt=strictParse(Buffer.from(receipt.body.original_receipt_utf8));
    const memberDigest='sha256:'+createHash('sha256').update('ledgerlab/base-membership/2-candidate.4\0').update(canonical(receipt.body.members)).digest('hex');
    if(embeddedReceipt.membership_hash!==memberDigest || embeddedReceipt.target!==receipt.body.target || embeddedReceipt.target!==p.target || !same(embeddedReceipt.target_snapshot,receipt.body.target_snapshot) || !same(embeddedReceipt.base_evaluation,receipt.body.base_evaluation) || embeddedReceipt.accepted_at!==receipt.body.accepted_at || manifest.body.target!==p.target || manifest.body.rated_final!==true) denial('BASE_RECEIPT');
    const postings=[...refs.values()].filter(x=>x.kind==='base-posting');
    const bases=[...refs.values()].filter(x=>x.kind==='target-basis');
    total(postings); total(bases);
    const evaluation=[...refs.values()].find(x=>x.kind==='base-evaluation');
    if(!evaluation || evaluation.body.postings.length!==postings.length || evaluation.body.postings.some(m=>refs.get(canonical([m.kind,m.id]))?.content_hash!==m.content_hash)) denial('BASE_EVALUATION');
    if(!same(manifest.body.base_evaluation,{kind:evaluation.kind,id:evaluation.id,content_hash:evaluation.content_hash})) denial('BASE_EVALUATION');
    const covered=new Set();
    for(const basis of bases) {
      let sum=0n;
      for(const m of basis.body.postings) {
        const posting=refs.get(canonical([m.kind,m.id]));
        if(!posting || covered.has(posting.id) || posting.body.book!==basis.body.book || posting.body.agreement_id!==basis.body.agreement_id || posting.body.event_id!==p.target) denial('BASE_POSTING_BIJECTION');
        covered.add(posting.id); sum+=bigint(posting.body.amount.atoms);
      }
      if(sum!==bigint(basis.body.amount.atoms) || basis.body.target!==p.target) denial('BASE_POSTING_BIJECTION');
    }
    if(covered.size!==postings.length || !same(manifest.body.retail_basis,{kind:'target-basis',id:bases.find(x=>x.body.book==='retail')?.id,content_hash:bases.find(x=>x.body.book==='retail')?.content_hash})) denial('BASE_POSTING_BIJECTION');
    const evaluated=strictParse(Buffer.from(evaluation.body.evaluation_utf8));
    if(!Array.isArray(evaluated.actions) || !Array.isArray(evaluated.invocations) || !Array.isArray(evaluated.consumptions) || evaluated.source_authority?.active!==true) denial('BASE_EVALUATION');
    let retailEvaluated=0n,supplierEvaluated=0n;
    for(const action of evaluated.actions) {
      if(action.book==='retail') retailEvaluated+=bigint(action.amount.atoms);
      else if(action.book==='supplier') supplierEvaluated+=bigint(action.amount.atoms);
      else denial('BASE_EVALUATION_BOOK');
    }
    if(retailEvaluated!==bigint(p.base_atoms) || supplierEvaluated!==bigint(p.supplier_booked)) denial('BASE_EVALUATION_BOOK');
    let consumed=0n;
    const invocationById=new Map(evaluated.invocations.map(x=>[x.id,x]));
    for(const x of evaluated.consumptions) {
      const inv=invocationById.get(x.invocation_id);
      if(!inv || bigint(inv.held.atoms)!==bigint(x.consume.atoms)+bigint(x.release.atoms) || bigint(inv.maximum_exposure.atoms)!==bigint(inv.held.atoms)) denial('BASE_CONSUMPTION');
      consumed+=bigint(x.consume.atoms);
    }
    if(consumed!==bigint(p.supplier_booked)) denial('BASE_CONSUMPTION');
    const mappings=evaluation.body.identity_mappings,projection=new Set();
    if(!Array.isArray(mappings)) denial('BASE_MAPPING');
    for(const m of mappings) {
      if(m.target!==p.target || refs.get(canonical([m.projection.kind,m.projection.id]))?.content_hash!==m.projection.content_hash) denial('BASE_MAPPING');
      if(m.original_kind==='action') projection.add(m.projection.id);
    }
    if(projection.size!==postings.length || postings.some(x=>!projection.has(x.id))) denial('BASE_MAPPING');
    const bindings=[...refs.values()].filter(x=>x.kind==='binding-snapshot');
    if(bindings.some(x=>!manifest.body.verified_assents.includes(x.body.assent))) denial('BASE_ASSENT');
  } else {
    const actions=[...refs.values()].filter(x=>x.kind==='action');
    total(actions);
    const ids=new Set(actions.map(x=>x.id));
    if(!Array.isArray(receipt.body.action_ids) || receipt.body.action_ids.length!==ids.size || receipt.body.action_ids.some(id=>!ids.has(id))) denial('BASE_RECEIPT_ACTIONS');
    for(const m of manifest.body.members??[]) if(refs.get(canonical([m.kind,m.id]))?.content_hash!==m.content_hash) denial('BASE_MEMBERSHIP');
  }
}
function object(kind,full_key,body,origin) {
  const bytes=Buffer.from(canonical(body));
  return {origin,kind,full_key,body:bytes.toString('base64'),body_hash:createHash('sha256').update(bytes).digest('hex'),bytes:String(bytes.length)};
}
function objectIdentity(o) { return canonical([o.origin,o.kind,o.full_key,o.body_hash,o.bytes]); }
function originFor(s,c,host) {
  const e=s.enrollment??c.payload;
  return {store:e.store,scope:e.scope,registration:e.registration,host,ordinal:String(journal(s,host).ordinal+1n)};
}
function proveSource(s,proof) {
  requireShape('proof',proof);
  const untrusted={...proof}; delete untrusted.trusted_observation_ref;
  if(digest('authority',untrusted)!==proof.trusted_observation_ref || !s.trustedObs.has(proof.trusted_observation_ref)) denial('PROOF_OBSERVATION');
  const e=s.enrollment??s.pendingEnrollment;
  if(!e || proof.store!==e.store || !same(proof.scope,e.scope) || proof.registration!==e.registration || (s.enrollment && proof.host!==e.store && !s.gateways.some(g=>g.gateway===proof.host))) denial('PROOF_BINDING');
  const j=s.journals.get(proof.host), source=j?.segments.get(proof.segment);
  if(!source || source.host!==proof.host || source.ordinal!==proof.ordinal || source.result.root!==proof.root || digest('segment',source)!==proof.segment) denial('PROOF_SOURCE');
  const found=source.objects.find(o=>o.kind===proof.fact_kind && same(o.full_key,proof.full_key) && o.body_hash===proof.body_hash && o.bytes===proof.bytes);
  if(!found || !same(found.origin,{store:proof.store,scope:proof.scope,registration:proof.registration,host:proof.host,ordinal:proof.ordinal})) denial('PROOF_MEMBERSHIP');
  checkObject(found);
  return {object:found,segment:source};
}
function sourceFor(s,proof,kind,key,host) {
  if(proof.fact_kind!==kind || !same(proof.full_key,key)) denial('PROOF_KIND_KEY');
  const found=proveSource(s,proof);
  if(proof.host!==host) denial('PROOF_TARGET');
  return found;
}
function sourceFact(c,s,effects,origin) {
  const p=c.payload;
  const choice={
    PREPARE_ENROLL:['ENROLL_PREPARATION',p.gateway], PREPARE_ROUND:['ROUND_PREPARATION',c.kind==='PREPARE_ROUND'?digest('namespace',[p.gateway,p.round]):null],
    ENROLL:['ENROLLMENT',p.registration], LOCAL_GRANT:['GRANT',p.grant?.id], ISSUE:['CLAIM',p.token?.id],
    RECEIVE:[s.tokens.get(p.token)?.disposition==='ALIAS'?'ALIAS':'RECEIPT',p.token],
    RETURN_UNUSED:['RETURNED_UNUSED',p.token], RECONCILE:['RECONCILIATION',p.token],
    RETIRE_GRANT:['RETIREMENT',p.grant], BEGIN:['BEGIN',p.round],
    SEALED:['SEAL',p.round], CLOSE:['TERMINAL',p.round], ABORT:['TERMINAL',p.round],
    INSTALL:['INSTALLATION',p.round],
  }[c.kind];
  return choice?object(choice[0],choice[1],{payload:p,effects},origin):null;
}
function authorizationReferences(s,c) {
  const p=c.payload,hashes=new Set([c.authority.document]);
  if(c.kind==='ENROLL') {
    for(const f of p.families) { hashes.add(f.assent);if(f.roles.payer_delegation) hashes.add(f.roles.payer_delegation); }
    for(const pool of p.pools) for(const a of pool.authorizations) { hashes.add(a.assent);if(a.roles.payer_delegation) hashes.add(a.roles.payer_delegation); }
  }
  if(c.kind==='DECIDE'||c.kind==='CORRECT') { hashes.add(p.assent);if(p.roles.payer_delegation) hashes.add(p.roles.payer_delegation); }
  const limit=c.kind==='ENROLL'?83:['DECIDE','CORRECT'].includes(c.kind)?3:1;
  const resolved=[...hashes].map(hash=>authoritySource(s,hash));
  const bytes=resolved.reduce((n,x)=>n+bigint(x.record.bytes),0n);
  if(resolved.length>limit || bytes>BigInt(c.kind==='ENROLL'?524288:limit*16384)) denial('AUTH_SOURCE_LIMIT');
  return resolved.sort((a,b)=>a.identity<b.identity?-1:a.identity>b.identity?1:0);
}
function checkAssent(s,hash,scope,target,roles,terms) {
  const body=authoritySource(s,hash,'ASSENT').body;
  sourceContext(body,scope,target);
  if(!same(body.roles,roles) || body.terms!==digest('authority',terms)) denial('ASSENT_BINDING');
}
function delegation(s,roles,scope,target,agreement,amount,at,windowEnd=null) {
  const hash=roles.payer_delegation;
  if(!hash) { if(roles.payer!==roles.bearer) refuse('PAYER_DELEGATION');return null; }
  const body=authoritySource(s,hash,'DELEGATION').body;
  sourceContext(body,scope,target);
  const stripped={...roles};delete stripped.payer_delegation;
  const terms={...body};delete terms.assent;
  if(!same(body.roles,stripped) || body.acceptor!==roles.payer || body.assent.terms!==digest('authority',terms) || !body.agreement_ids.includes(agreement) || body.assent.accepted_at>at || at<body.starts_at || at>=body.ends_at || bigint(amount)>bigint(body.maximum_exposure)) denial('DELEGATION_BINDING');
  if(windowEnd!==null && windowEnd>=body.ends_at) denial('DELEGATION_WINDOW');
  return body;
}
function validateEnrollmentSources(s,p,observedAt) {
  const exposures=new Map();
  const addExposure=(hash,n)=>{if(hash) exposures.set(hash,(exposures.get(hash)??0n)+n);};
  for(const f of p.families) {
    const terms={...f};delete terms.assent;
    checkAssent(s,f.assent,p.scope,p.target,f.roles,terms);
    const amounts=[f.ordinary_atoms,...f.correction_atoms].map(x=>{const n=bigint(x);return n<0n?-n:n;});
    const max=amounts.reduce((a,b)=>a>b?a:b,0n);
    delegation(s,f.roles,p.scope,p.target,f.key[1],max,observedAt,f.correction_by);
    addExposure(f.roles.payer_delegation,max);
  }
  const agreements=[...new Set(p.families.map(f=>f.key[1]))];
  for(const pool of p.pools) for(const a of pool.authorizations) {
    checkAssent(s,a.assent,p.scope,p.target,a.roles,{id:pool.id,funding:pool.funding,positive:pool.positive,negative:pool.negative,gross:pool.gross,direction:a.direction,roles:a.roles});
    const cap=a.direction==='POSITIVE'?bigint(pool.positive):a.direction==='NEGATIVE'?bigint(pool.negative):0n;
    const bound=[bigint(pool.funding),bigint(pool.gross),cap].reduce((v,n)=>v<n?v:n);
    for(const agreement of agreements) delegation(s,a.roles,p.scope,p.target,agreement,bound,observedAt);
    addExposure(a.roles.payer_delegation,bound);
  }
  for(const [hash,n] of exposures) if(n>bigint(authoritySource(s,hash,'DELEGATION').body.maximum_exposure)) denial('DELEGATION_EXPOSURE');
}
function effect(kind,body) { return {kind,body}; }
function cacheLocalRow(s,target,gateway,position,row) {
  s.pendingRows.push({target,gateway,position,bytes:Buffer.from(canonical(row))});
}
function sealCoverage(s,gateway,cutoff,high) {
  const claims=s.localClaims.get(gateway)??new Map(),dispositions=s.localDispositionBytes.get(gateway)??new Map(),receipts=s.localReceiptBytes.get(gateway)??new Map();
  for(let allocation=1n;allocation<=cutoff;allocation++) {
    const claim=claims.get(String(allocation));
    if(!claim) refuse('LOCAL_CLAIM_GAP');
    if(claim.gateway!==gateway || bigint(claim.allocation)!==allocation) denial('LOCAL_CLAIM_BINDING');
    if(!dispositions.has(String(allocation))) refuse('TOKEN_NOT_TERMINAL');
  }
  for(let position=1n;position<=high;position++) if(!receipts.has(String(position))) refuse('LOCAL_RECEIPT_GAP');
  return {dispositions,receipts};
}
function hashLocalArray(domain,rows,count) {
  const h=createHash('sha256').update(`ledgerlab/central-r3/${domain}/1`).update(Buffer.from([0])).update('[');
  for(let n=1n;n<=count;n++) {if(n>1n) h.update(',');h.update(rows.get(String(n)));}
  return h.update(']').digest('hex');
}
function sealFacts(s,gateway,cutoff,high) {
  const {dispositions,receipts}=sealCoverage(s,gateway,cutoff,high);
  return {disposition_root:hashLocalArray('result',dispositions,cutoff),receipt_root:hashLocalArray('receipt',receipts,high)};
}
function allowedWindow(f,submission,at) { return submission.occurred_at>=f.starts_at && submission.occurred_at<f.occurs_before && at<=f.received_by; }
function demandConservation(s) {
  for(const p of s.suppliers.values()) if(p.maximum!==p.consumed+p.held+p.released) denial('SUPPLIER_CONSERVATION');
  for(const p of s.pools.values()) if(p.grossUsed>p.gross || p.positiveUsed>p.positive || p.negativeUsed>p.negative || p.fundingUsed>p.funding) denial('POOL_CONSERVATION');
}
function demandResourceConservation(s,host) {
  const account=s.resources.get(host);
  if(!account) denial('RESOURCE_HOST');
  const held=Object.fromEntries(dims.map(k=>[k,0n])),reserved=new Map();
  for(const owner of account.held.values()) {
    for(const dimension of dims) held[dimension]+=owner.vector[dimension];
    for(const [name,n] of Object.entries(owner.counters)) reserved.set(name,(reserved.get(name)??0n)+n);
  }
  for(const dimension of dims) {
    if(account.used[dimension]+held[dimension]>account.provisioned[dimension]) denial('RESOURCE_CONSERVATION');
  }
  for(const [name,pair] of s.counters.get(host)??[]) {
    if(pair.R!==(reserved.get(name)??0n) || pair.q+pair.R>M) denial('COUNTER_CONSERVATION');
  }
}
function resourceFlow(s,c,host) {
  const p=c.payload;
  if(c.kind==='PREPARE_ENROLL') {
    charge(s,host,`single:prepare-enroll:${host}`,'PREPARE_ENROLL',c);
    for(let n=0;n<32;n++) reserve(s,host,`protected:gateway:${host}:${n}`,template('finish_gateway',true));
    return;
  }
  if(c.kind==='ENROLL') {
    charge(s,host,'single:enroll','ENROLL',c);
    for(let n=0;n<32;n++) reserve(s,host,`protected:center:${n}`,template('finish_central',true));
    return;
  }
  if(c.kind==='PREPARE_ROUND') {
    charge(s,host,`single:prepare-round:${p.round}`,'PREPARE_ROUND',c);
    reserve(s,host,`round:gateway:${host}:${p.round}`,template('cancel_gateway',true));
    return;
  }
  if(c.kind==='LOCAL_GRANT') {
    const id=p.grant.id, bundle=template('local_grant',true), grantResources=resourceVector(p.grant.resources);
    for(const k of dims) if(grantResources[k]<bundle.vector[k]) refuse('GRANT_RESOURCE_TEMPLATE');
    for(const [k,n] of Object.entries(bundle.counters)) if(bigint(p.grant.counters[k])<n) refuse('GRANT_COUNTER_TEMPLATE');
    reserve(s,host,`local-grant:${id}`,bundle); spend(s,host,`local-grant:${id}`,'LOCAL_GRANT',c); return;
  }
  if(c.kind==='REGISTER_GRANT') { reserve(s,host,`central-grant:${p.grant.id}`,template('central_grant',true)); spend(s,host,`central-grant:${p.grant.id}`,'REGISTER_GRANT',c); return; }
  if(c.kind==='ISSUE') { reserve(s,host,`central-token:${p.token.id}`,template('central_token',true)); spend(s,host,`central-token:${p.token.id}`,'ISSUE',c); release(s,host,`central-grant:${p.grant}`); return; }
  if(['ACTIVATE','RECEIVE','RETURN_UNUSED'].includes(c.kind)) { const t=getToken(s,p.token); spend(s,host,`local-grant:${t.body.grant}`,c.kind,c); return; }
  if(c.kind==='LOCAL_TERMINAL') { spend(s,host,`local-grant:${p.grant}`,'LOCAL_TERMINAL',c); release(s,host,`local-grant:${p.grant}`); return; }
  if(c.kind==='RETIRE_GRANT') { spend(s,host,`central-grant:${p.grant}`,'RETIRE_GRANT',c); release(s,host,`central-grant:${p.grant}`); return; }
  if(['IMPORT','RECONCILE','ADVANCE','ADVANCE_RECEIPT'].includes(c.kind)) {
    const token=c.kind==='ADVANCE'?[...s.tokens.values()].find(t=>t.body.gateway===p.gateway && t.body.allocation===p.through)?.body.id:
      c.kind==='ADVANCE_RECEIPT'?[...s.tokens.values()].find(t=>t.disposition==='NEW_CASE' && t.receipt.gateway===p.gateway && t.receipt.position===p.through)?.body.id:p.token;
    if(!token) refuse('TOKEN_RESOURCE');
    spend(s,host,`central-token:${token}`,c.kind,c);
    const t=getToken(s,token);
    if((c.kind==='ADVANCE' && t.disposition!=='NEW_CASE') || (t.disposition==='NEW_CASE' && t.allocationAdvanced && t.receiptAdvanced)) {
      release(s,host,`central-token:${token}`);
    }
    return;
  }
  if(c.kind==='BEGIN') {
    const r=s.round,idx=Math.min(...r.families.map(k=>getFamily(s,k).index));
    r.resourceIndex=idx;
    if(r.mode==='CANCELLABLE') {
      reserve(s,host,`round:center:${r.id}`,template('cancel_central',true));
    } else {
      const central=s.resources.get(host)?.held.get(`protected:center:${idx}`);
      if(!central?.active) refuse('PROTECTED_BUNDLE');
    }
    spend(s,host,r.mode==='CANCELLABLE'?`round:center:${r.id}`:`protected:center:${idx}`,'BEGIN',c); return;
  }
  if(['SEAL_BEGIN','SEALED','INSTALL'].includes(c.kind)) {
    const r=s.round,owner=r.mode==='CANCELLABLE'?`round:gateway:${p.gateway}:${r.id}`:`protected:gateway:${p.gateway}:${r.resourceIndex}`;
    spend(s,host,owner,c.kind,c);
    if(c.kind==='INSTALL') release(s,host,owner);
    return;
  }
  if(['DRAIN','READY','CLOSE','ABORT','ACK_INSTALL'].includes(c.kind)) {
    const r=s.round,owner=r.mode==='CANCELLABLE'?`round:center:${r.id}`:`protected:center:${r.resourceIndex}`;
    spend(s,host,owner,c.kind,c);
    if(r.ackedAll) finishRoundResources(s,r);
    return;
  }
  charge(s,host,`single:${c.kind}:${commandDigest(c)}`,c.kind,c);
}
function execute(c,s) {
  const p=c.payload, out=[];
  switch(c.kind) {
    case 'PREPARE_ENROLL': {
      if(s.enrollment || s.preparations.has(p.gateway) || p.namespace.gateway!==p.gateway || !same(p.namespace.scope,p.scope)) refuse('PREPARATION_STATE');
      if(!s.writers.has(p.gateway) || p.store===p.gateway) refuse('PREPARATION_HOST');
      s.preparations.set(p.gateway,p);
      break;
    }
    case 'ENROLL': {
      validateEnrollmentSources(s,p,c.authority.observed_at);
      if(s.enrollment) refuse('ALREADY_ENROLLED');
      const hostNames=[p.store,...p.gateways.map(g=>g.gateway)].sort();
      if(!same([...s.resources.keys()].sort(),hostNames) || !same([...s.writers.keys()].sort(),p.gateways.map(g=>g.gateway).sort())) denial('INITIAL_HOSTS');
      const origin={store:p.store,scope:p.scope,registration:p.registration,host:p.store,ordinal:'1'};
      if(s.originalObjects.some(o=>!same(o.origin,origin))) denial('ORIGINAL_ORIGIN');
      s.pendingEnrollment=p;
      const expectedIntent=digest('enrollment',Object.fromEntries(Object.entries(p).filter(([k])=>k!=='preparations')));
      if(p.preparations.length!==p.gateways.length) refuse('ENROLL_PREPARATIONS');
      const prepSeen=new Set();
      for(const proof of p.preparations) {
        const prep=s.preparations.get(proof.host),g=p.gateways.find(x=>x.gateway===proof.host);
        if(!prep || !g || prepSeen.has(proof.host) || prep.intent!==expectedIntent || prep.store!==p.store || prep.registration!==p.registration || !same(prep.scope,p.scope) || !same(prep.namespace,g)) refuse('ENROLL_PREPARATIONS');
        const preparation=sourceFor(s,proof,'ENROLL_PREPARATION',proof.host,proof.host);
        if(authoritySource(s,preparation.segment.command.authority.document,'AUTHORIZATION').body.target!==p.target) denial('AUTH_SOURCE_SCOPE');
        prepSeen.add(proof.host);
      }
      if(p.base_receipt!==s.baseReceipt || p.base_manifest!==s.baseManifest) denial('BASE_ANCHOR');
      originalBase(s,p);
      if(p.gateways.length!==new Set(p.gateways.map(g=>g.gateway)).size || p.gateways.length!==new Set(p.gateways.map(g=>g.tag)).size) refuse('GATEWAY_DUPLICATE');
      const families=new Map();
      for(const f of p.families) {
        if(!same(f.key[0],p.scope) || f.key[3]!==p.target || families.has(familyKey(f.key))) refuse('FAMILY_TOPOLOGY');
        if(f.prerequisites.some(n=>bigint(n)>=BigInt(p.families.length))) refuse('FAMILY_TOPOLOGY');
        families.set(familyKey(f.key),{...f,index:families.size,closed:false,unavailable:false,accepted:false});
      }
      for(const f of families.values()) if(f.prerequisites.some(n=>bigint(n)>=BigInt(f.index))) refuse('FAMILY_CYCLE');
      const suppliers=new Map();
      for(const x of p.suppliers) { if(suppliers.has(x.id)) refuse('SUPPLIER_DUPLICATE'); const q=Object.fromEntries(Object.entries(x).map(([k,v])=>[k,k==='id'?v:bigint(v)])); if(q.maximum!==q.consumed+q.held+q.released) refuse('SUPPLIER_CONSERVATION'); suppliers.set(x.id,q); }
      for(const f of families.values()) {
        if(f.book==='RETAIL' && f.supplier_pool!=='none') refuse('RETAIL_SUPPLIER_POOL');
        if(f.book==='SUPPLIER' && !suppliers.has(f.supplier_pool)) refuse('UNKNOWN_SUPPLIER');
        if(f.book==='SUPPLIER' && bigint(f.ordinary_atoms)<0n) refuse('SUPPLIER_NEGATIVE_ORDINARY');
      }
      const pools=new Map();
      for(const x of p.pools) {
        if(pools.has(x.id)) refuse('POOL_DUPLICATE');
        pools.set(x.id,{...x,funding:bigint(x.funding),positive:bigint(x.positive),negative:bigint(x.negative),gross:bigint(x.gross),fundingUsed:0n,positiveUsed:0n,negativeUsed:0n,grossUsed:0n});
      }
      s.enrollment={...p}; delete s.pendingEnrollment; s.gateways=p.gateways; s.family=families; s.suppliers=suppliers; s.pools=pools; s.customer=bigint(p.base_atoms);
      for(const g of p.gateways) { s.allocationPrefix.set(g.gateway,0n); s.receiptPrefix.set(g.gateway,0n); s.allocationNext.set(g.gateway,0n); s.receiptNext.set(g.gateway,0n); s.localInstalled.set(g.gateway,0n); s.gatewayAck.set(g.gateway,0n); s.clockFloor.set(g.gateway,'0001-01-01T00:00:00.000000Z'); }
      break;
    }
    case 'PREPARE_ROUND': {
      if(!s.enrollment || p.mode!=='CANCELLABLE' || p.enrollment!==digest('enrollment',s.enrollment) || bigint(p.round)<=bigint(p.predecessor) || bigint(p.predecessor)!==s.localInstalled.get(p.gateway)) refuse('ROUND_PREPARATION');
      getGateway(s,p.gateway);
      sourceFor(s,p.proof,'ENROLLMENT',s.enrollment.registration,s.enrollment.store);
      const k=canonical([p.round,p.gateway]);
      if(s.roundPreparations.has(k)) refuse('ROUND_PREPARATION');
      s.roundPreparations.set(k,p);
      break;
    }
    case 'LOCAL_GRANT': {
      if(!s.enrollment) refuse('NOT_ENROLLED');
      const x=p.grant,g=getGateway(s,x.gateway); writer(s,x.gateway);
      sourceFor(s,p.proof,'ENROLLMENT',s.enrollment.registration,s.enrollment.store);
      const prefix=`gr1.${g.tag}.`,suffix=x.id.startsWith(prefix)?x.id.slice(prefix.length):'';
      if(!suffix || encoder.encode(suffix).length>91 || encoder.encode(x.id).length>128) refuse('GRANT_NAMESPACE');
      if(s.grants.has(x.id) || x.store!==s.enrollment.store || x.registration!==s.enrollment.registration || !same(x.namespace,g) || x.journal_head!==journal(s,x.gateway).root || !s.grantAuth.has(x.authentication)) refuse('GRANT_BINDING');
      const omitted={...x}; delete omitted.authentication;
      if(digest('grant',omitted)!==x.authentication) denial('GRANT_AUTH_HASH');
      s.grants.set(x.id,{body:x,status:'LOCAL_HELD',token:null,localTerminal:false}); break;
    }
    case 'REGISTER_GRANT': {
      const x=p.grant,g=getGrant(s,x.id);
      if(!same(g.body,x) || g.status!=='LOCAL_HELD') refuse('GRANT_STATE');
      const source=sourceFor(s,p.proof,'GRANT',x.id,x.gateway);
      if(source.segment.command.kind!=='LOCAL_GRANT' || !same(source.segment.command.payload.grant,x)) denial('PROOF_TARGET');
      g.status='REGISTERED_UNCLAIMED'; break;
    }
    case 'ISSUE': {
      const g=getGrant(s,p.grant),t=p.token;
      if(g.status!=='REGISTERED_UNCLAIMED' || t.grant!==p.grant || t.gateway!==g.body.gateway || s.tokens.has(t.id)) refuse('GRANT_CLAIM');
      if(s.round && !s.round.ackedAll && t.category==='ORDINARY') refuse('ROUND_ORDINARY_FROZEN');
      const next=s.allocationNext.get(t.gateway)+1n;
      if(bigint(t.allocation)!==next || t.claim!==digest('claim',[t.grant,t.id,t.gateway,t.allocation,t.category])) refuse('CLAIM');
      s.allocationNext.set(t.gateway,next); g.status='CLAIMED'; g.token=t.id;
      s.tokens.set(t.id,{body:t,status:'ISSUED',disposition:null,receipt:null,imported:false,reconciled:false,allocationAdvanced:false,receiptAdvanced:false}); break;
    }
    case 'ACTIVATE': {
      const t=getToken(s,p.token); writer(s,p.gateway);
      if(t.body.gateway!==p.gateway || t.status!=='ISSUED' || s.round?.sealed?.has(p.gateway)) refuse('ACTIVATE_STATE');
      const source=sourceFor(s,p.proof,'CLAIM',p.token,s.enrollment.store);
      if(source.segment.command.kind!=='ISSUE' || !same(source.segment.command.payload.token,t.body)) denial('PROOF_TARGET');
      if(!s.localClaims.has(p.gateway)) s.localClaims.set(p.gateway,new Map());
      s.localClaims.get(p.gateway).set(t.body.allocation,{...t.body});
      t.status='ACTIVE'; break;
    }
    case 'RECEIVE': {
      const t=getToken(s,p.token),w=writer(s,p.gateway);
      if(!same(c.key,p.delivery)) denial('RECEIVE_IDENTITY');
      const gw=getGateway(s,p.gateway);
      if(!checkNamespace(gw,p.delivery)) refuse('WRONG_OWNER');
      if(t.body.gateway!==p.gateway || t.status!=='ACTIVE' || bigint(p.epoch)!==bigint(w.epoch) || s.round?.sealed?.has(p.gateway)) refuse('RECEIPT_STATE');
      if(p.received_at!==c.authority.observed_at || p.received_at<p.submission.occurred_at) denial('RECEIPT_TIME');
      if(p.received_at<s.clockFloor.get(p.gateway)) refuse('CLOCK_FLOOR');
      evidenceGood(p.submission.evidence);
      const dk=keyOf(p.delivery), prior=s.deliveries.get(dk);
      if(prior) refuse('DELIVERY_OCCUPIED');
      if(s.gateways[routeIndex(s,p.submission.case)].gateway!==p.gateway) refuse('WRONG_OWNER');
      const ck=caseKey(p.submission.case), existing=s.cases.get(ck), sub=digest('submission',p.submission);
      if(existing && existing.submission!==sub) refuse('CASE_CONFLICT');
      if(existing) {
        t.status='CONSUMED'; t.disposition='ALIAS'; t.aliasOf=existing.receipt.token; t.receipt=existing.receipt;
        s.deliveries.set(dk,{submission:sub,receipt:existing.receipt,token:t.body.id,alias:true});
        out.push(effect('RECEIPT',existing.receipt));
      } else {
        const f=getFamily(s,p.submission.case[0]);
        if(!allowedWindow(f,p.submission,p.received_at)) refuse('OCCURRENCE_WINDOW');
        const pos=s.receiptNext.get(p.gateway)+1n; s.receiptNext.set(p.gateway,pos);
        const receipt={case:p.submission.case,delivery:p.delivery,submission:sub,token:t.body.id,gateway:p.gateway,epoch:p.epoch,position:String(pos),received_at:p.received_at,journal_head:w.journal_head};
        const item={submission:sub,body:p.submission,receipt,status:'LOCAL',path:null,decision:null,transfer:null,revision:0n,evidence:new Set(p.submission.evidence.map(e=>e.sha256))};
        s.cases.set(ck,item); s.deliveries.set(dk,{submission:sub,receipt,token:t.body.id,alias:false});
        t.status='CONSUMED'; t.disposition='NEW_CASE'; t.receipt=receipt; out.push(effect('RECEIPT',receipt));
        cacheLocalRow(s,'localReceiptBytes',p.gateway,receipt.position,[receipt.position,receipt]);
      }
      cacheLocalRow(s,'localDispositionBytes',p.gateway,t.body.allocation,[t.body.allocation,t.body.id,t.disposition]);
      break;
    }
    case 'RETURN_UNUSED': {
      const t=getToken(s,p.token),g=getGrant(s,t.body.grant); writer(s,p.gateway);
      if(t.body.gateway!==p.gateway || t.body.claim!==p.claim || !['ISSUED','ACTIVE'].includes(t.status)) refuse('TOKEN_TERMINAL');
      const source=sourceFor(s,p.proof,'CLAIM',p.token,s.enrollment.store);
      if(source.segment.command.kind!=='ISSUE' || !same(source.segment.command.payload.token,t.body)) denial('PROOF_TARGET');
      if(!s.localClaims.has(p.gateway)) s.localClaims.set(p.gateway,new Map());
      s.localClaims.get(p.gateway).set(t.body.allocation,{...t.body});
      t.status='RETURNED'; t.disposition='RETURNED_UNUSED';
      cacheLocalRow(s,'localDispositionBytes',p.gateway,t.body.allocation,[t.body.allocation,t.body.id,t.disposition]);
      break;
    }
    case 'IMPORT': {
      const t=getToken(s,p.token);
      const source=sourceFor(s,p.proof,t.disposition==='NEW_CASE'?'RECEIPT':'ALIAS',p.token,t.body.gateway);
      if(t.status!=='CONSUMED' || t.imported) refuse('IMPORT_STATE');
      if(p.proof.host!==t.body.gateway || !same(p.proof.full_key,t.body.id) || !['RECEIPT','ALIAS'].includes(p.proof.fact_kind) || p.proof.fact_kind!==t.disposition.replace('NEW_CASE','RECEIPT')) denial('IMPORT_PROOF');
      if(source.segment.command.kind!=='RECEIVE' || source.segment.command.payload.token!==p.token) denial('IMPORT_PROOF');
      t.sourceObject=source.object;
      if(t.disposition==='ALIAS') { const original=getToken(s,t.aliasOf); if(!original.imported) refuse('ALIAS_ORIGINAL_NOT_IMPORTED'); }
      if(t.disposition==='NEW_CASE') {
        const q=s.cases.get(caseKey(t.receipt.case));
        if(!q || q.status!=='LOCAL') refuse('IMPORT_STATE');
        const f=getFamily(s,q.body.case[0]);
        q.status='PENDING'; q.path=(f.closed || f.unavailable)?'ADJUSTMENT':'ORDINARY';
      }
      t.imported=true; break;
    }
    case 'RECONCILE': {
      const t=getToken(s,p.token);
      if(!t.disposition || (t.status==='CONSUMED' && !t.imported) || t.reconciled) refuse('RECONCILE_STATE');
      const source=sourceFor(s,p.proof,t.disposition==='NEW_CASE'?'RECEIPT':t.disposition==='ALIAS'?'ALIAS':'RETURNED_UNUSED',p.token,t.body.gateway);
      if(p.proof.host!==t.body.gateway || !same(p.proof.full_key,t.body.id) || !['RECEIPT','ALIAS','RETURNED_UNUSED'].includes(p.proof.fact_kind)) denial('RECONCILE_PROOF');
      if(!['RECEIVE','RETURN_UNUSED'].includes(source.segment.command.kind) || source.segment.command.payload.token!==p.token) denial('RECONCILE_PROOF');
      t.sourceObject=source.object;
      t.reconciled=true; break;
    }
    case 'ADVANCE': {
      getGateway(s,p.gateway);
      const want=s.allocationPrefix.get(p.gateway)+1n;
      if(bigint(p.through)!==want) refuse('ALLOCATION_PREFIX');
      const t=[...s.tokens.values()].find(x=>x.body.gateway===p.gateway && bigint(x.body.allocation)===want);
      if(!t || !t.reconciled) refuse('UNRECONCILED_TOKEN');
      s.allocationPrefix.set(p.gateway,want);
      t.allocationAdvanced=true;
      break;
    }
    case 'ADVANCE_RECEIPT': {
      getGateway(s,p.gateway);
      const want=s.receiptPrefix.get(p.gateway)+1n;
      if(bigint(p.through)!==want) refuse('RECEIPT_PREFIX');
      const t=[...s.tokens.values()].find(x=>x.disposition==='NEW_CASE' && x.receipt.gateway===p.gateway && bigint(x.receipt.position)===want);
      if(!t || !t.imported || t.receiptAdvanced) refuse('UNIMPORTED_RECEIPT');
      s.receiptPrefix.set(p.gateway,want);
      t.receiptAdvanced=true;
      break;
    }
    case 'RETIRE_GRANT': {
      const g=getGrant(s,p.grant);
      if(g.status!=='REGISTERED_UNCLAIMED') refuse('GRANT_CLAIMED_OR_RETIRED');
      g.status='RETIRED_UNCLAIMED'; break;
    }
    case 'LOCAL_TERMINAL': {
      const g=getGrant(s,p.grant); writer(s,p.gateway);
      if(g.body.gateway!==p.gateway || g.localTerminal) refuse('LOCAL_TERMINAL_STATE');
      if(g.status==='RETIRED_UNCLAIMED') {
        const source=sourceFor(s,p.proof,'RETIREMENT',p.grant,s.enrollment.store);
        if(source.segment.command.kind!=='RETIRE_GRANT') denial('RETIREMENT_PROOF');
      } else if(g.status==='CLAIMED') {
        const t=getToken(s,g.token);
        if(!t.reconciled) refuse('TOKEN_PROOF');
        const source=sourceFor(s,p.proof,'RECONCILIATION',t.body.id,s.enrollment.store);
        if(source.segment.command.kind!=='RECONCILE') denial('TOKEN_PROOF');
      } else refuse('LOCAL_TERMINAL_STATE');
      g.localTerminal=true; break;
    }
    case 'BEGIN': {
      if(!s.enrollment || (s.round && !s.round.ackedAll)) refuse('ROUND_ACTIVE');
      const n=bigint(p.round);
      if(n!==s.lastRound+1n || bigint(p.predecessor)!==s.lastRound) refuse('ROUND_PREDECESSOR');
      if(p.families.some(k=>getFamily(s,k).closed)) refuse('FAMILY_CLOSED');
      for(const g of p.gateways) getGateway(s,g);
      if(new Set(p.gateways).size!==p.gateways.length || p.gateways.length===0) refuse('GATEWAY_COVERAGE');
      if(p.mode==='FINISH_ONLY' && p.preparations.length) refuse('ROUND_PREPARATIONS');
      if(p.mode==='CANCELLABLE') {
        if(p.preparations.length!==p.gateways.length) refuse('ROUND_PREPARATIONS');
        const seen=new Set();
        for(const proof of p.preparations) {
          const prep=s.roundPreparations.get(canonical([p.round,proof.host]));
          if(!prep || seen.has(proof.host) || !p.gateways.includes(proof.host) || bigint(prep.predecessor)!==s.gatewayAck.get(proof.host)) refuse('ROUND_PREPARATIONS');
          sourceFor(s,proof,'ROUND_PREPARATION',digest('namespace',[proof.host,p.round]),proof.host);
          seen.add(proof.host);
        }
      }
      s.round={id:n,mode:p.mode,families:p.families,gateways:p.gateways,cutoffs:Object.fromEntries(p.gateways.map(g=>[g,s.allocationNext.get(g)])),state:'DRAINING',sealed:new Set(),drained:new Set(),installed:new Set(),acked:new Set(),terminal:null,ackedAll:false};
      out.push(effect('ROUND_BEGIN',{round:p.round,predecessor:p.predecessor,mode:p.mode,cutoffs:p.gateways.map(g=>({gateway:g,cutoff:String(s.round.cutoffs[g]),gateway_predecessor:String(s.gatewayAck.get(g))})).sort((a,b)=>canonical(a)<canonical(b)?-1:1)}));
      break;
    }
    case 'SEAL_BEGIN': {
      const r=s.round; writer(s,p.gateway);
      if(!r || r.id!==bigint(p.round) || bigint(p.predecessor)!==s.localInstalled.get(p.gateway) || !r.gateways.includes(p.gateway) || r.sealed.has(p.gateway)) refuse('SEAL_ROUND');
      const source=sourceFor(s,p.proof,'BEGIN',p.round,s.enrollment.store);
      if(source.segment.command.kind!=='BEGIN' || !source.segment.command.payload.gateways.includes(p.gateway)) denial('SEAL_PROOF');
      const begin=source.segment.result.effects.find(e=>e.kind==='ROUND_BEGIN')?.body;
      if(!begin || begin.round!==p.round || begin.cutoffs.find(x=>x.gateway===p.gateway)?.gateway_predecessor!==p.predecessor) denial('SEAL_PROOF');
      s.localBegins.set(p.gateway,begin);
      r.sealed.add(p.gateway); break;
    }
    case 'SEALED': {
      const r=s.round;
      if(!r || r.id!==bigint(p.round) || !r.sealed.has(p.gateway)) refuse('SEAL_STATE');
      const begin=s.localBegins.get(p.gateway),entry=begin?.cutoffs.find(x=>x.gateway===p.gateway);
      if(!entry) denial('SEAL_BEGIN_MISSING');
      const cutoff=bigint(entry.cutoff);
      r[`sealed_${p.gateway}`]=true;
      r[`high_${p.gateway}`]=s.receiptNext.get(p.gateway);
      const roots=sealFacts(s,p.gateway,cutoff,r[`high_${p.gateway}`]);
      out.push(effect('SEAL',{round:p.round,gateway:p.gateway,cutoff:String(cutoff),receipt_high:String(r[`high_${p.gateway}`]),...roots}));
      break;
    }
    case 'DRAIN': {
      const r=s.round;
      if(!r || r.id!==bigint(p.round) || !r[`sealed_${p.gateway}`] || r.drained.has(p.gateway) || s.allocationPrefix.get(p.gateway)<r.cutoffs[p.gateway] || s.receiptPrefix.get(p.gateway)<r[`high_${p.gateway}`]) refuse('DRAIN_INCOMPLETE');
      const source=sourceFor(s,p.proof,'SEAL',p.round,p.gateway);
      if(source.segment.command.kind!=='SEALED') denial('SEAL_PROOF');
      const sealed=source.segment.result.effects.find(e=>e.kind==='SEAL')?.body;
      if(!sealed || sealed.gateway!==p.gateway || sealed.round!==p.round || bigint(sealed.cutoff)!==r.cutoffs[p.gateway] || bigint(sealed.receipt_high)!==r[`high_${p.gateway}`]) denial('SEAL_PROOF');
      s.coverage.set(p.gateway,{...sealed,observation:p.proof.trusted_observation_ref});
      r.drained.add(p.gateway); break;
    }
    case 'READY': {
      const r=s.round;
      if(!r || r.id!==bigint(p.round) || r.state!=='DRAINING' || r.drained.size!==r.gateways.length) refuse('NOT_READY');
      r.state='READY'; break;
    }
    case 'CLOSE': {
      const r=s.round;
      if(!r || r.id!==bigint(p.round) || r.state!=='READY') refuse('CLOSE_STATE');
      if(p.closed_at!==c.authority.observed_at) denial('CLOSE_TIME');
      const predecessor=journal(s,s.enrollment.store).root;
      const enrollment=digest('enrollment',s.enrollment);
      const family_heads=[...s.family.values()].map(f=>{
        const consumer=s.entitlements.get(familyKey(f.key));
        let entitlement={status:'UNCONSUMED'};
        if(consumer) {
          const q=s.cases.get(consumer);
          const record={case:q.body.case,state:q.status==='ALLOWED'?'FINAL_ALLOW':q.status==='DENIED'?'FINAL_DENY':q.status,revision:String(q.revision),signed:String(q.currentAmount??0n),receipt:digest('receipt',q.receipt)};
          entitlement={status:'CONSUMED',case:q.body.case,revision:String(q.revision),head:digest('result',record)};
        }
        return {family:f.key,terms:digest('enrollment',s.enrollment.families[f.index]),closed:f.closed,unavailable:f.unavailable,entitlement};
      }).sort((a,b)=>canonical(a)<canonical(b)?-1:canonical(a)>canonical(b)?1:0);
      const before=[...s.suppliers.values()].map(q=>({...q}));
      for(const k of r.families) { const family=getFamily(s,k); family.closed=true; family.unavailable=true; }
      // Unavailability is a bounded graph projection from original terms.
      for(const f of s.family.values()) if(!f.accepted && f.prerequisites.some(x=>{ const dep=[...s.family.values()][Number(x)]; return !dep.accepted && (dep.closed || dep.unavailable); })) f.unavailable=true;
      const transitioned=[];
      for(const x of s.suppliers.values()) {
        const open=[...s.family.values()].some(f=>f.supplier_pool===x.id && !f.closed);
        if(!open && r.families.some(k=>getFamily(s,k).supplier_pool===x.id)) { x.released+=x.held; x.held=0n; transitioned.push(x.id); }
      }
      const transferred=[];
      for(const q of s.cases.values()) if(q.status==='PENDING' && q.path==='ORDINARY') {
        const f=getFamily(s,q.body.case[0]);
        if(f.closed || f.unavailable) { q.path='ADJUSTMENT'; transferred.push(q); }
      }
      r.state='COMMITTED'; r.terminal='COMMITTED'; r.closedAt=p.closed_at;
      if(!r.gateways.length) { r.ackedAll=true; s.lastRound=r.id; }
      const cutoffs=r.gateways.map(g=>{const seal=s.coverage.get(g);if(!seal) denial('COVERAGE_MISSING');return {gateway:g,status:'COMPLETE_GATEWAY_CUTOFF',cutoff:seal.cutoff,allocation_prefix:String(s.allocationPrefix.get(g)),receipt_high:seal.receipt_high,receipt_prefix:String(s.receiptPrefix.get(g)),disposition_root:seal.disposition_root,receipt_root:seal.receipt_root,observation:seal.observation};}).sort((a,b)=>canonical(a)<canonical(b)?-1:1);
      const certificate={predecessor,enrollment,family_heads,families:r.families,unavailable:[...s.family.values()].filter(f=>f.unavailable).map(f=>f.key).sort((a,b)=>canonical(a)<canonical(b)?-1:1),supplier_before:before.filter(x=>transitioned.includes(x.id)).map(x=>({id:x.id,maximum:String(x.maximum),consumed:String(x.consumed),held:String(x.held),released:String(x.released)})),supplier_after:[...s.suppliers.values()].filter(x=>transitioned.includes(x.id)).map(x=>({id:x.id,maximum:String(x.maximum),consumed:String(x.consumed),held:String(x.held),released:String(x.released)})),round:String(r.id),cutoffs,closed_at:p.closed_at};
      const closureId=digest('closure',certificate);
      for(const q of transferred) q.transfer=closureId;
      out.push(effect('CLOSURE',certificate));
      break;
    }
    case 'ABORT': {
      const r=s.round;
      if(!r || r.id!==bigint(p.round) || r.mode!=='CANCELLABLE' || r.terminal) refuse('ABORT_UNAVAILABLE');
      r.terminal='ABORTED'; r.state='ABORTED';
      if(!r.gateways.length) { r.ackedAll=true; s.lastRound=r.id; }
      break;
    }
    case 'INSTALL': {
      const r=s.round; writer(s,p.gateway);
      if(!r || r.id!==bigint(p.round) || r.terminal!==p.outcome || !r.gateways.includes(p.gateway) || r.installed.has(p.gateway)) refuse('INSTALL_STATE');
      const beginSource=sourceFor(s,p.begin,'BEGIN',p.round,s.enrollment.store);
      const begin=beginSource.segment.result.effects.find(e=>e.kind==='ROUND_BEGIN')?.body;
      if(beginSource.segment.command.kind!=='BEGIN' || begin?.cutoffs.find(x=>x.gateway===p.gateway)?.gateway_predecessor!==String(s.localInstalled.get(p.gateway))) denial('INSTALL_BEGIN_PROOF');
      const source=sourceFor(s,p.proof,'TERMINAL',p.round,s.enrollment.store);
      if(!['CLOSE','ABORT'].includes(source.segment.command.kind)) denial('TERMINAL_PROOF');
      r.installed.add(p.gateway);
      s.localInstalled.set(p.gateway,r.id);
      r.sealed.delete(p.gateway);
      if(r.terminal==='COMMITTED') s.clockFloor.set(p.gateway,r.closedAt);
      break;
    }
    case 'ACK_INSTALL': {
      const r=s.round;
      if(!r || r.id!==bigint(p.round) || !r.installed.has(p.gateway) || r.acked.has(p.gateway)) refuse('ACK_INSTALL_STATE');
      const source=sourceFor(s,p.proof,'INSTALLATION',p.round,p.gateway);
      if(source.segment.command.kind!=='INSTALL' || source.segment.command.payload.outcome!==r.terminal) denial('INSTALL_PROOF');
      r.acked.add(p.gateway);
      s.gatewayAck.set(p.gateway,r.id);
      if(r.acked.size===r.gateways.length) { r.ackedAll=true; s.lastRound=r.id; }
      break;
    }
    case 'SUPPLEMENT': {
      const q=s.cases.get(caseKey(p.case)); if(!q || q.status!=='PENDING') refuse('CASE_NOT_PENDING');
      evidenceGood(p.evidence);
      for(const x of p.evidence) q.evidence.add(x.sha256);
      if(q.evidence.size>16) refuse('EVIDENCE_LIMIT'); break;
    }
    case 'DECIDE': {
      const q=s.cases.get(caseKey(p.case));
      if(!q || q.status!=='PENDING' || q.path!==p.path) refuse('CASE_DECISION_STATE');
      const f=getFamily(s,p.case[0]);
      if(p.path==='ORDINARY') {
        const terms={...s.enrollment.families[f.index]};delete terms.assent;
        checkAssent(s,p.assent,s.enrollment.scope,s.enrollment.target,p.roles,terms);
      } else {
        const pool=s.pools.get(p.pool),direction=bigint(p.signed_atoms)===0n?'ZERO':bigint(p.signed_atoms)>0n?'POSITIVE':'NEGATIVE';
        if(!pool) refuse('ADJUSTMENT_AUTHORITY');
        checkAssent(s,p.assent,s.enrollment.scope,s.enrollment.target,p.roles,{id:pool.id,funding:String(pool.funding),positive:String(pool.positive),negative:String(pool.negative),gross:String(pool.gross),direction,roles:p.roles});
      }
      const signed=bigint(p.signed_atoms),proposedMagnitude=signed<0n?-signed:signed;
      delegation(s,p.roles,s.enrollment.scope,s.enrollment.target,f.key[1],proposedMagnitude,c.authority.observed_at);
      if(p.verdict==='DENY') { if(bigint(p.signed_atoms)!==0n) refuse('DENY_AMOUNT'); q.status='DENIED'; q.decision='DENY'; break; }
      const fk=familyKey(f.key);
      if(s.entitlements.has(fk)) refuse('ENTITLEMENT_USED');
      const amount=bigint(p.signed_atoms), magnitude=amount<0n?-amount:amount;
      if(p.path==='ORDINARY') {
        if(f.closed || f.unavailable || q.body.occurred_at>=f.occurs_before || c.authority.observed_at>f.accepted_by || !same(p.roles,f.roles) || p.assent!==f.assent || amount!==bigint(f.ordinary_atoms)) refuse('ORDINARY_TERMS');
        if(f.book==='RETAIL' && amount>0n && s.premiumUsed+amount>bigint(s.enrollment.premium_cap)) refuse('PREMIUM_CAP');
        for(const n of f.prerequisites) if(![...s.family.values()][Number(n)].accepted) refuse('PREREQUISITE');
        const supplier=f.book==='SUPPLIER'?s.suppliers.get(f.supplier_pool):null;
        if(supplier && supplier.held<magnitude) refuse('SUPPLIER_HOLD');
        if(supplier) { supplier.held-=magnitude; supplier.consumed+=magnitude; }
        if(f.book==='RETAIL' && amount>0n) s.premiumUsed+=amount;
      } else {
        if(!(f.closed || f.unavailable) || !q.transfer && q.path==='ORDINARY') refuse('ADJUSTMENT_PATH');
        const pool=s.pools.get(p.pool);
        const direction=amount===0n?'ZERO':amount>0n?'POSITIVE':'NEGATIVE';
        const authorized=pool?.authorizations.find(a=>a.direction===direction && same(a.roles,p.roles) && a.assent===p.assent);
        if(!pool || !authorized) refuse('ADJUSTMENT_AUTHORITY');
        if(pool.grossUsed+magnitude>pool.gross || pool.fundingUsed+magnitude>pool.funding || (amount>=0n && pool.positiveUsed+magnitude>pool.positive) || (amount<0n && pool.negativeUsed+magnitude>pool.negative)) refuse('ADJUSTMENT_CAP');
        pool.grossUsed+=magnitude; pool.fundingUsed+=magnitude;
        if(amount>=0n) pool.positiveUsed+=magnitude; else pool.negativeUsed+=magnitude;
        s.gross+=magnitude;
      }
      s.entitlements.set(fk,caseKey(p.case)); f.accepted=true; q.status='ALLOWED'; q.decision='ALLOW'; q.revision=1n; q.currentAmount=amount; q.path=p.path;
      if(f.book==='RETAIL') s.customer+=amount;
      if(amount!==0n) out.push(effect('ACTION',{book:f.book,case:p.case,revision:'1',kind:p.path,signed_atoms:String(amount),magnitude:String(magnitude),roles:p.roles,assent:p.assent}));
      break;
    }
    case 'CORRECT': {
      const q=s.cases.get(caseKey(p.case));
      if(!q || q.status!=='ALLOWED' || q.revision!==bigint(p.expected_revision)) refuse('CORRECTION_REVISION');
      const f=getFamily(s,p.case[0]),amount=bigint(p.replacement);
      const terms={...s.enrollment.families[f.index]};delete terms.assent;
      checkAssent(s,p.assent,s.enrollment.scope,s.enrollment.target,p.roles,terms);
      delegation(s,p.roles,s.enrollment.scope,s.enrollment.target,f.key[1],amount<0n?-amount:amount,c.authority.observed_at);
      if(c.authority.observed_at>f.correction_by || !f.correction_atoms.includes(p.replacement) || !same(p.roles,f.roles) || p.assent!==f.assent) refuse('CORRECTION_AUTHORITY');
      const old=q.currentAmount;
      if(f.book==='RETAIL' && q.path==='ORDINARY') {
        const next=s.premiumUsed-(old>0n?old:0n)+(amount>0n?amount:0n);
        if(next>bigint(s.enrollment.premium_cap)) refuse('PREMIUM_CAP');
        s.premiumUsed=next;
      }
      q.revision++; q.currentAmount=amount; if(f.book==='RETAIL') s.customer+=amount-old;
      if(old!==0n) out.push(effect('ACTION',{book:f.book,case:p.case,revision:String(q.revision),kind:'INVERSE',signed_atoms:String(-old),magnitude:String(old<0n?-old:old),roles:p.roles,assent:p.assent}));
      if(amount!==0n) out.push(effect('ACTION',{book:f.book,case:p.case,revision:String(q.revision),kind:'REPLACEMENT',signed_atoms:String(amount),magnitude:String(amount<0n?-amount:amount),roles:p.roles,assent:p.assent}));
      break;
    }
    case 'REPLACE_WRITER': {
      const w=writer(s,p.gateway);
      if(bigint(p.old_epoch)!==bigint(w.epoch) || bigint(p.new_epoch)!==bigint(w.epoch)+1n || p.journal_head!==w.journal_head || p.fence===ZERO) refuse('WRITER_REPLACEMENT');
      w.epoch=p.new_epoch; w.journal_head=p.journal_head; w.fence=p.fence; break;
    }
    case 'EXTEND_RESOURCES': {
      const r=s.resources.get(p.host); if(!r) refuse('RESOURCE_HOST');
      for(const [k,v] of Object.entries(p.resources)) { const n=bigint(v); if(r.provisioned[k]+n>M) refuse('RESOURCE_OVERFLOW'); r.provisioned[k]+=n; }
      break;
    }
    default: denial('COMMAND_KIND');
  }
  demandConservation(s);
  return out;
}

function describe(s,total) {
  const journals=Object.fromEntries([...s.journals].map(([k,v])=>[k,{ordinal:String(v.ordinal),segment:v.previous,root:v.root}]));
  const root=digest('replay',Object.entries(journals).sort(([a],[b])=>a<b?-1:a>b?1:0).map(([k,v])=>[k,v.root]));
  return {commands:total,segments:s.segments,duplicates:s.duplicates,refused:s.refused,
    grants:s.grants.size,tokens:s.tokens.size,receipts:s.cases.size,cases:s.cases.size,
    aliases:[...s.deliveries.values()].filter(x=>x.alias).length,
    customer_atoms:String(s.customer),adjustment_gross:String(s.gross),entitlements:s.entitlements.size,
    suppliers:[...s.suppliers.values()].map(x=>({id:x.id,maximum:String(x.maximum),consumed:String(x.consumed),held:String(x.held),released:String(x.released)})),
    round:String(s.lastRound),allocation_prefix:Object.fromEntries([...s.allocationPrefix].map(([k,v])=>[k,String(v)])),receipt_prefix:Object.fromEntries([...s.receiptPrefix].map(([k,v])=>[k,String(v)])),journals,root};
}

function snapshot(s) {
  const decimalVector=v=>Object.fromEntries(dims.map(k=>[k,String(v[k])]));
  const byKey=entries=>Object.fromEntries([...entries].sort(([a],[b])=>a<b?-1:a>b?1:0));
  const resources=byKey([...s.resources].map(([host,a])=>[host,{
    provisioned:decimalVector(a.provisioned),used:decimalVector(a.used),
    held:Object.fromEntries(dims.map(k=>[k,String(heldTotal(a,k))])),
  }]));
  const counters=byKey([...s.resources.keys()].map(host=>[host,Object.fromEntries(SCHEMA['x-counters'].map(name=>{
    const pair=s.counters.get(host)?.get(name)??{q:0n,R:0n};
    return [name,{q:String(pair.q),R:String(pair.R)}];
  }))]));
  const ownerName=owner=>owner.replace(/^protected:center:(\d+)$/,(_,n)=>`close:${n}`)
    .replace(/^protected:gateway:([^:]+):(\d+)$/,(_,g,n)=>`close:${n}:${g}`)
    .replace(/^local-grant:/,'grant-local:').replace(/^central-grant:/,'grant-central:')
    .replace(/^central-token:/,'token:').replace(/^round:center:/,'optional-round:')
    .replace(/^round:gateway:([^:]+):(.+)$/,(_,g,n)=>`optional-round:${n}:${g}`);
  const allocations=byKey([...s.resources].flatMap(([host,a])=>[...a.held].map(([owner,o])=>[ownerName(owner),{
    host,slots:[...o.slots],held:decimalVector(o.vector),
  }])));
  const grants=byKey([...s.grants].map(([id,g])=>[id,{state:g.status,local_terminal:g.localTerminal,token:g.token}]));
  const tokens=byKey([...s.tokens].map(([id,t])=>[id,{
    state:t.disposition??t.status,imported:t.imported,reconciled:t.reconciled,
    advanced:t.allocationAdvanced,receipt_advanced:t.receiptAdvanced,
  }]));
  const cases=byKey([...s.cases].map(([key,q])=>[key,{
    state:q.status==='ALLOWED'?'FINAL_ALLOW':q.status==='DENIED'?'FINAL_DENY':q.status==='PENDING'?`${q.path}_PENDING`:q.status,transfer:q.transfer??null,revision:String(q.revision),signed:String(q.currentAmount??0n),
  }]));
  const pools=byKey([...s.pools].map(([id,p])=>[id,{
    used:String(p.fundingUsed),positive:String(p.positiveUsed),negative:String(p.negativeUsed),gross:String(p.grossUsed),
  }]));
  const suppliers=[...s.suppliers.values()].map(x=>({id:x.id,maximum:String(x.maximum),consumed:String(x.consumed),held:String(x.held),released:String(x.released)})).sort((a,b)=>a.id<b.id?-1:a.id>b.id?1:0);
  const authority_retained=byKey([...s.authorityRetained].map(([host,records])=>[host,byKey([...records].map(([identity,v])=>[identity,{hash:v.hash,bytes:v.bytes,segment:v.segment}]))]));
  return {resources,counters,allocations,grants,tokens,cases,entitlements:byKey(s.entitlements),pools,suppliers,
    preparations:byKey(s.preparations),round_preparations:byKey(s.roundPreparations),
    object_inventory:byKey([...s.objectInventory].map(([host,ids])=>[host,[...ids].sort()])),
    authority_retained,actions:s.actions,certificates:s.certificates};
}

function inventoryFor(s,c,host,effects) {
  const proposed=[];
  const retained=s.authorityRetained.get(host)??new Map(),dependencies=new Set();
  for(const source of s.currentAuthoritySources) {
    const prior=retained.get(source.identity);
    if(prior) {
      const segment=journal(s,host).segments.get(prior.segment);
      if(prior.hash!==source.record.body_hash || !segment?.objects.some(o=>o.kind==='AUTHORITY' && same(o.full_key,[source.body.source,source.body.id,source.body.revision]) && o.body_hash===source.record.body_hash && o.bytes===source.record.bytes)) denial('AUTH_SOURCE_RETAINED');
      dependencies.add(prior.segment);
    } else proposed.push(object('AUTHORITY',[source.body.source,source.body.id,source.body.revision],source.body,originFor(s,c,host)));
  }
  s.pendingAuthorityDependencies=dependencies;
  if(c.kind==='ENROLL') proposed.push(...s.originalObjects);
  if(c.payload.proof) proposed.push(proveSource(s,c.payload.proof).object);
  if(c.payload.begin) proposed.push(proveSource(s,c.payload.begin).object);
  for(const proof of c.payload.preparations??[]) proposed.push(proveSource(s,proof).object);
  const fact=sourceFact(c,s,effects,originFor(s,c,host));
  if(fact) proposed.push(fact);
  const known=s.objectInventory.get(host)??new Set(),pending=new Set(),objects=[];
  for(const o of proposed) {
    requireShape('object',o);checkObject(o);
    const id=objectIdentity(o);
    if(!known.has(id) && !pending.has(id)) { pending.add(id);objects.push(o); }
  }
  objects.sort((a,b)=>canonical(a)<canonical(b)?-1:canonical(a)>canonical(b)?1:0);
  return objects;
}

function runReplay(trace) {
  const s=initialState(trace);
  for(const c of trace.commands) {
    requireShape('command',c);
    checkSetOrder(c);
    const dg=commandDigest(c), host=hostFor(c,s), k=canonical([host,c.key]), previous=s.commands.get(k);
    if(previous) {
      checkAuthority(c,s,true,host);
      if(previous.digest===dg) { s.duplicates++; continue; }
      s.refused++; continue;
    }
    if(c.authority.permission==='read' && c.kind==='RECEIVE') {
      checkAuthority(c,s,true,host);
      s.refused++;continue;
    }
    checkAuthority(c,s,false,host);
    if(encoder.encode(canonical(c)).length>SCHEMA['x-operation-limit']) denial('OPERATION_LIMIT');
    // Committed journal segments and trust anchors are immutable during a
    // tentative operation. Excluding them keeps a long refused-operation
    // schedule from repeatedly cloning the entire authenticated history.
    const stable=['journals','commands','authority','authoritySources','authorityObs','grantAuth','trustedObs','originalObjects','baseReceipt','baseManifest','localDispositionBytes','localReceiptBytes'];
    const mutable={...s};
    for(const name of stable) mutable[name]=null;
    const backup=structuredClone(mutable);
    for(const name of stable) backup[name]=s[name];
    let effects;
    let objects;
    try { s.pendingRows=[];s.currentAuthoritySources=authorizationReferences(s,c);if(c.payload.proof && c.kind!=='IMPORT') proveSource(s,c.payload.proof);effects=execute(c,s); s.currentEffects=effects; objects=inventoryFor(s,c,host,effects); s.pendingIntroductions=objects.length;s.pendingAuthorityIntroductions=objects.filter(o=>o.kind==='AUTHORITY').length; resourceFlow(s,c,host); demandResourceConservation(s,host); delete s.currentEffects;delete s.pendingIntroductions;delete s.pendingAuthorityIntroductions; }
    catch(e) { if(e instanceof Refusal) { if(process.env.R3_DEBUG) process.stderr.write(`${c.kind}: ${e.code}\n`);Object.assign(s,backup);delete s.pendingRows;delete s.currentEffects;delete s.currentAuthoritySources;delete s.pendingAuthorityDependencies;delete s.pendingIntroductions;delete s.pendingAuthorityIntroductions;delete s.pendingEnrollment;s.refused++; continue; } throw e; }
    for(const row of s.pendingRows) {
      const target=s[row.target];if(!target.has(row.gateway)) target.set(row.gateway,new Map());
      target.get(row.gateway).set(row.position,row.bytes);
    }
    delete s.pendingRows;
    for(const e of effects) { if(e.kind==='ACTION') s.actions.push(e); if(e.kind==='CLOSURE') s.certificates.push(e.body); }
    // Local and central journal segmentation is refined by the pinned schema.
    const j=journal(s,host),root=digest('replay',[j.root,dg,effects]);
    if(!s.objectInventory.has(host)) s.objectInventory.set(host,new Set());
    for(const o of objects) s.objectInventory.get(host).add(objectIdentity(o));
    const dependencies=[...new Set([...(c.payload.proof?[c.payload.proof.segment]:[]),...(c.payload.begin?[c.payload.begin.segment]:[]),...(c.payload.preparations??[]).map(x=>x.segment),...s.pendingAuthorityDependencies])].sort();
    delete s.pendingAuthorityDependencies;
    delete s.currentAuthoritySources;
    const segment={profile:'central-adjudication-r3/1',host,ordinal:String(j.ordinal+1n),previous:j.previous,previous_root:j.root,command:c,result:{status:'COMMITTED',code:c.kind,effects,root},dependencies,objects};
    requireShape('result',segment.result);
    requireShape('segment',segment);
    const bodyHashes=new Set(),trust=encoder.encode(canonical(c)).length+encoder.encode(canonical(segment.result)).length+objects.reduce((n,o)=>{if(bodyHashes.has(o.body_hash))return n;bodyHashes.add(o.body_hash);return n+Number(o.bytes);},0);
    if(trust>SCHEMA['x-trusted-limit']) denial('TRUST_LIMIT');
    const encodedSegment=Buffer.from(canonical(segment));
    if(encodedSegment.length>SCHEMA['x-segment-limit']) denial('SEGMENT_LIMIT');
    j.ordinal++; j.previous=digest('segment',segment); j.root=root; j.segments.set(j.previous,segment);j.byOrdinal.set(String(j.ordinal),j.previous);j.bytes.set(String(j.ordinal),encodedSegment);s.segments++;
    if(!s.authorityRetained.has(host)) s.authorityRetained.set(host,new Map());
    for(const o of objects.filter(o=>o.kind==='AUTHORITY')) s.authorityRetained.get(host).set(canonical(o.full_key),{hash:o.body_hash,bytes:o.bytes,segment:j.previous});
    if(s.writers.has(host)) s.writers.get(host).journal_head=root;
    s.commands.set(k,{digest:dg,result:segment.result});
  }
  return {output:{accepted:true,summary:describe(s,trace.commands.length),snapshot:snapshot(s)},state:s};
}
export function replay(trace) { return runReplay(trace).output; }

export function create_seal_session(trace,gateway,round) {
  const {state:s}=runReplay(trace),j=s.journals.get(gateway),begin=s.localBegins.get(gateway);
  const cutoff=begin?.cutoffs.find(x=>x.gateway===gateway)?.cutoff;
  if(!j || !s.round || s.round.id!==bigint(round) || !s.round.sealed.has(gateway) || cutoff===undefined) denial('SEAL_SESSION');
  const counts={DISPOSITIONS:bigint(cutoff),RECEIPTS:s.receiptNext.get(gateway)};
  const source={dispositions:s.localDispositionBytes.get(gateway)??new Map(),receipts:s.localReceiptBytes.get(gateway)??new Map()};
  const currentFragment=(phase,entry)=>{
    if(entry===0n) return Buffer.from('[');
    if(entry===counts[phase]+1n) return Buffer.from(']');
    const rows=phase==='DISPOSITIONS'?source.dispositions:source.receipts;
    if(phase==='DISPOSITIONS') {
      const claim=s.localClaims.get(gateway)?.get(String(entry));
      if(!claim || claim.gateway!==gateway || bigint(claim.allocation)!==entry) denial('SEAL_SOURCE');
    }
    const row=rows.get(String(entry));
    if(!row) denial('SEAL_SOURCE');
    return entry===1n?Buffer.from(row):Buffer.concat([Buffer.from(','),Buffer.from(row)]);
  };
  const expected={store:s.enrollment.store,scope:s.enrollment.scope,target:s.enrollment.target,profile:'central-adjudication-r3/1',enrollment:digest('enrollment',s.enrollment),registration:s.enrollment.registration,host:gateway,ordinal:String(j.ordinal),segment:j.previous,root:j.root};
  const hashes={DISPOSITIONS:createHash('sha256').update('ledgerlab/central-r3/result/1').update(Buffer.from([0])),RECEIPTS:createHash('sha256').update('ledgerlab/central-r3/receipt/1').update(Buffer.from([0]))};
  let phase='DISPOSITIONS',entry=0n,offset=0n,live=null,aborted=false,completed=false;
  const cursor=()=>({expected,round,phase,entry:String(entry),byte_offset:String(offset)});
  return {
    scan(budget,resume=null) {
      if(aborted) denial('SCAN_ABORTED');
      if(completed) denial('SCAN_COMPLETE');
      if(!validate_shape('seal_budget',budget)) denial('SEAL_BUDGET');
      if(live && !same(resume,live)) denial('SEAL_CURSOR');
      if(!live && resume) denial('SEAL_CURSOR');
      let bytes=0n,pages=0n;
      while(phase!=='COMPLETE' && bytes<bigint(budget.bytes) && pages<bigint(budget.pages)) {
        const fragment=currentFragment(phase,entry);
        if(!fragment || offset>=BigInt(fragment.length)) denial('SEAL_CURSOR');
        const take=[4032n,BigInt(fragment.length)-offset,bigint(budget.bytes)-bytes].reduce((a,v)=>a<v?a:v);
        if(take<=0n) break;
        hashes[phase].update(fragment.subarray(Number(offset),Number(offset+take)));
        bytes+=take;pages++;offset+=take;
        if(offset===BigInt(fragment.length)) {
          entry++;offset=0n;
          if(entry===counts[phase]+2n) {phase=phase==='DISPOSITIONS'?'RECEIPTS':'COMPLETE';entry=0n;}
        }
      }
      const measured={bytes:String(bytes),pages:String(pages)};
      if(phase!=='COMPLETE') {live=cursor();return {status:'INCOMPLETE',cursor:live,measured};}
      live=null;completed=true;
      return {status:'COMPLETE',cursor:cursor(),measured,disposition_root:hashes.DISPOSITIONS.digest('hex'),receipt_root:hashes.RECEIPTS.digest('hex')};
    },
    abort(){aborted=true;live=null;},
  };
}

// This model reads already verified immutable segment bytes. A store adapter
// must supply the same ordinal lookup and bounded pages from its own storage.
const readerSessions=new WeakSet();
export function create_read_session(trace) {
  const state={live:null};readerSessions.add(state);
  return {read(request){return read_prefix(trace,request,()=>{},state);},cancel(){state.live=null;}};
}
export function read_prefix(trace,request,visitor=()=>{},session=null) {
  if(!validate_shape('read_request',request)) denial('READ_SHAPE');
  if(request.cursor && (!readerSessions.has(session) || !same(request.cursor,session.live))) denial('READ_CURSOR_SESSION');
  if(!request.cursor && readerSessions.has(session) && session.live) denial('READ_SESSION_BUSY');
  const {state:s}=runReplay(trace),e=request.expected,b=request.budget;
  if(!s.enrollment || e.store!==s.enrollment.store || !same(e.scope,s.enrollment.scope) || e.registration!==s.enrollment.registration ||
      e.target!==s.enrollment.target || e.profile!=='central-adjudication-r3/1' ||
      e.enrollment!==digest('enrollment',s.enrollment)) denial('EXPECTED_PREFIX');
  const j=s.journals.get(e.host);
  const ordinal=bigint(e.ordinal);
  if(!j || ordinal<1n || ordinal>j.ordinal) denial('EXPECTED_PREFIX');
  const terminal=j.segments.get(j.byOrdinal.get(e.ordinal));
  if(j.byOrdinal.get(e.ordinal)!==e.segment || terminal.result.root!==e.root) denial('EXPECTED_PREFIX');
  if(request.cursor) {
    const c=request.cursor,raw={expected:c.expected,ordinal:c.ordinal,byte_offset:c.byte_offset,verified_root:c.verified_root};
    if(!same(c.expected,e) || c.continuation!==digest('authority',raw)) denial('READ_CURSOR');
  }
  let complete=bigint(request.cursor?.ordinal??'0'),offset=bigint(request.cursor?.byte_offset??'0');
  let verifiedRoot=request.cursor?.verified_root??ZERO;
  if(complete>ordinal || (complete>0n && j.segments.get(j.byOrdinal.get(String(complete)))?.result.root!==verifiedRoot)) denial('READ_CURSOR');
  let measuredBytes=0n,measuredPages=0n,measuredSegments=0n;
  const maxBytes=bigint(b.bytes),maxPages=bigint(b.pages),maxSegments=bigint(b.segments);
  while(complete<ordinal && measuredBytes<maxBytes && measuredPages<maxPages && measuredSegments<maxSegments) {
    const seg=j.segments.get(j.byOrdinal.get(String(complete+1n))),bytes=j.bytes.get(String(complete+1n));
    if(!bytes) denial('READ_STORAGE');
    if(offset>=BigInt(bytes.length)) denial('READ_CURSOR');
    const pageRemainder=4096n-(offset%4096n);
    const take=[BigInt(bytes.length)-offset,pageRemainder,maxBytes-measuredBytes].reduce((a,v)=>a<v?a:v);
    if(take<=0n) break;
    offset+=take;measuredBytes+=take;measuredPages++;
    if(offset===BigInt(bytes.length)) {complete++;measuredSegments++;offset=0n;verifiedRoot=seg.result.root;visitor(seg);}
  }
  const measured={bytes:String(measuredBytes),pages:String(measuredPages),segments:String(measuredSegments)};
  if(complete<ordinal) {
    const raw={expected:e,ordinal:String(complete),byte_offset:String(offset),verified_root:verifiedRoot};
    const response={status:'INCOMPLETE',cursor:{...raw,continuation:digest('authority',raw)},measured};
    if(!validate_shape('read_response',response)) denial('READ_RESULT');
    if(readerSessions.has(session)) session.live=response.cursor;
    return response;
  }
  const current=j.ordinal===ordinal;
  const coverage=s.gateways.map(g=>({gateway:g.gateway,status:'UNKNOWN_GATEWAY_COVERAGE'})).sort((a,b)=>canonical(a)<canonical(b)?-1:canonical(a)>canonical(b)?1:0);
  const response={status:'COMPLETE',expected:e,measured,selection:current?'CURRENT_AT_READ':'HISTORICAL_PREFIX',scope:e.host===s.enrollment.store?'CENTRAL_PREFIX':'GATEWAY_PREFIX',coverage};
  if(!validate_shape('read_response',response)) denial('READ_RESULT');
  if(readerSessions.has(session)) session.live=null;
  return response;
}
export function create_comparison_session(trace) {
  const state={live:null,first:null,fold:null}; readerSessions.add(state);
  return {compare(request){return compareRequestInternal(trace,request,state);},cancel(){state.live=null;state.first=null;state.fold=null;}};
}
export function compare_request(trace,request) { return compareRequestInternal(trace,request,null); }
function compareRequestInternal(trace,request,session) {
  if(!validate_shape('comparison_request',request)) {
    if(isObject(request.policy) && Object.keys(request.policy).some(k=>k!=='resolution_atoms')) denial('FIELDS');
    denial('COMPARISON_SHAPE');
  }
  if(request.cursor && (!session || !session.first || !same(request.cursor,session.live))) denial('READ_CURSOR_SESSION');
  if(session?.first && (!request.cursor || !same(session.first.expected,request.expected) || !same(session.first.policy,request.policy) || !same(session.first.coverage,request.coverage))) denial('COMPARISON_SESSION');
  if(session && !session.first) {session.first={expected:request.expected,policy:request.policy,coverage:request.coverage};session.fold={enrollment:null,actual:0n,ordinaryPositive:0n,resolution:null,resolutionCase:null,resolutionAmount:null,corrected:false,policyFailure:null,known:[],seenGateways:new Set()};}
  const f=session?.fold??{enrollment:null,actual:0n,ordinaryPositive:0n,resolution:null,resolutionCase:null,resolutionAmount:null,corrected:false,policyFailure:null,known:[],seenGateways:new Set()};
  const proposed=bigint(request.policy.resolution_atoms);
  const read=read_prefix(trace,{expected:request.expected,budget:request.budget,...(request.cursor?{cursor:request.cursor}:{})},seg=>{
    const c=seg.command,p=c.payload;
    if(c.kind==='ENROLL') {
      f.enrollment=p;f.actual=bigint(p.base_atoms);
      const candidates=p.families.filter(f=>f.book==='RETAIL' && f.key[2]==='resolution');
      if(candidates.length===1) f.resolution=candidates[0];
      for(const g of p.gateways) f.seenGateways.add(g.gateway);
    }
    if(!f.enrollment) return;
    for(const effect of seg.result.effects) {
      if(effect.kind==='ACTION' && effect.body.book==='RETAIL') f.actual+=bigint(effect.body.signed_atoms);
      if(effect.kind==='CLOSURE') f.known.push(...effect.body.cutoffs);
    }
    if(c.kind==='CORRECT' && f.resolution && same(p.case[0],f.resolution.key)) f.corrected=true;
    if(c.kind==='DECIDE' && p.verdict==='ALLOW' && p.path==='ORDINARY') {
      const family=f.enrollment.families.find(x=>same(x.key,p.case[0]));
      if(family?.book==='RETAIL') {
        const amount=f.resolution && same(family.key,f.resolution.key)?proposed:bigint(p.signed_atoms);
        if(f.resolution && same(family.key,f.resolution.key)) {f.resolutionCase=p.case;f.resolutionAmount=bigint(p.signed_atoms);}
        if(amount>0n) f.ordinaryPositive+=amount;
        if(!f.policyFailure && f.ordinaryPositive>bigint(f.enrollment.premium_cap)) f.policyFailure=p.case;
      }
    }
  },session);
  if(read.status==='INCOMPLETE') return {status:'INCOMPLETE',expected:request.expected,cursor:read.cursor,measured:read.measured,reason:'verification and policy fold incomplete; no successful total'};
  const seen=new Set();
  if(request.coverage.length!==f.seenGateways.size) denial('COVERAGE_EXACT_PREFIX');
  for(const row of request.coverage) {
    if(!f.seenGateways.has(row.gateway) || seen.has(row.gateway)) denial('COVERAGE_EXACT_PREFIX');
    seen.add(row.gateway);
    if(row.status==='UNKNOWN_GATEWAY_COVERAGE') continue;
    if(!f.known.some(k=>same(k,row))) denial('COVERAGE_EXACT_PREFIX');
  }
  if(session) {session.first=null;session.fold=null;}
  if(!f.resolution || !f.resolutionCase || f.corrected) return {status:'UNSUPPORTED',reason:'No fixed first resolution allowance',measured:read.measured};
  if(f.policyFailure) return {status:'POLICY_FAILURE',at_case:f.policyFailure,reason:'original premium cap exceeded at this actual acceptance',measured:read.measured};
  const alternative=f.actual-f.resolutionAmount+proposed;
  const response={status:'COMPARABLE',expected:request.expected,actual:String(f.actual),alternative:String(alternative),difference:String(alternative-f.actual),supplier_booked:f.enrollment.supplier_booked,coverage:request.coverage,measured:read.measured};
  if(!validate_shape('comparison_response',response)) denial('COMPARISON_RESULT');
  return response;
}

export function percentAtoms(basis,numerator,denominator) {
  const b=bigint(basis),n=bigint(numerator),d=bigint(denominator);
  if(d<=0n) denial('PERCENT_DENOMINATOR');
  const value=b*n,den=d*100n,absolute=value<0n?-value:value;
  const rounded=(absolute+den/2n)/den;
  return String(value<0n?-rounded:rounded);
}

export function compare_policy(trace,policy) {
  if(!validate_shape('comparison_policy',policy)) return {status:'UNSUPPORTED',reason:'Only a resolution amount is supported'};
  const resultAtPrefix=replay(trace),summary=resultAtPrefix.summary,enroll=trace.commands.find(c=>c.kind==='ENROLL')?.payload;
  const resolution=enroll?.families.filter(f=>f.book==='RETAIL' && f.key[2]==='resolution');
  if(!enroll || resolution?.length!==1) return {status:'UNSUPPORTED',reason:'No unique retail resolution family'};
  const family=resolution[0],decisions=trace.commands.filter(c=>c.kind==='DECIDE' && c.payload.verdict==='ALLOW' && c.payload.path==='ORDINARY');
  const resolutionDecision=decisions.find(c=>same(c.payload.case[0],family.key));
  if(!resolutionDecision || trace.commands.some(c=>c.kind==='CORRECT' && same(c.payload.case[0],family.key))) return {status:'UNSUPPORTED',reason:'Resolution outcome is not a fixed first allowance'};
  let premium=0n;
  for(const c of decisions) {
    const f=enroll.families.find(x=>same(x.key,c.payload.case[0]));
    if(f?.book!=='RETAIL') continue;
    const amount=c===resolutionDecision?bigint(policy.resolution_atoms):bigint(c.payload.signed_atoms);
    if(amount>0n) premium+=amount;
    if(premium>bigint(enroll.premium_cap)) return {status:'POLICY_FAILURE',at_case:c.payload.case,reason:'Original premium cap exceeded'};
  }
  const actual=bigint(summary.customer_atoms),alternative=actual-bigint(resolutionDecision.payload.signed_atoms)+bigint(policy.resolution_atoms);
  const center=summary.journals[enroll.store];
  const expected={store:enroll.store,scope:enroll.scope,target:enroll.target,profile:'central-adjudication-r3/1',enrollment:digest('enrollment',enroll),registration:enroll.registration,host:enroll.store,ordinal:center.ordinal,segment:center.segment,root:center.root};
  const verified=new Map();
  for(const certificate of resultAtPrefix.snapshot.certificates) for(const row of certificate.cutoffs) verified.set(row.gateway,row);
  const coverage=enroll.gateways.map(g=>verified.get(g.gateway)??{gateway:g.gateway,status:'UNKNOWN_GATEWAY_COVERAGE'})
    .sort((a,b)=>canonical(a)<canonical(b)?-1:canonical(a)>canonical(b)?1:0);
  const result={status:'COMPARABLE',expected,actual:String(actual),alternative:String(alternative),difference:String(alternative-actual),supplier_booked:enroll.supplier_booked,coverage,measured:read_prefix(trace,{expected,budget:{bytes:String(M),pages:String(M),segments:String(M)}}).measured};
  if(!validate_shape('comparison_response',result)) denial('COMPARISON_RESULT');
  return result;
}

if (process.argv[1] && fileURLToPath(import.meta.url)===process.argv[1]) {
  try {
    if(process.argv.length!==3 || process.argv[2]==='--self-test') denial('ARGUMENT');
    const result=replay(strictParse(readFileSync(process.argv[2])));
    process.stdout.write(canonical(result)+'\n');
  } catch(e) {
    process.stdout.write(canonical({accepted:false,error:e.code??'INVALID'})+'\n');
    process.exitCode=1;
  }
}
