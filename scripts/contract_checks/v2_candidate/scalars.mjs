// Independent schema/scalar validation. No Python or Rust validators are called.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {pathToFileURL} from 'node:url';

const full=(pattern,value)=>typeof value==='string'&&new RegExp('^(?:'+pattern+')(?![\\s\\S])','u').test(value);
const integer=(v,signed=true)=>full(signed?'0|-?[1-9][0-9]*':'0|[1-9][0-9]*',v);
const gcd=(a,b)=>{a=a<0n?-a:a;while(b){[a,b]=[b,a%b];}return a;};
const bits=n=>(n<0n?-n:n).toString(2).length;
function canonical(v,depth=0) {
  assert(depth<=32,'JSON_DEPTH');
  if(Array.isArray(v))return '['+v.map(x=>canonical(x,depth+1)).join(',')+']';
  if(v!==null&&typeof v==='object')return '{'+Object.keys(v).sort().map(k=>canonical(k,depth+1)+':'+canonical(v[k],depth+1)).join(',')+'}';
  assert(v!==null&&['string','number','boolean'].includes(typeof v),'JSON_SCALAR');
  if(typeof v==='number')assert(Number.isSafeInteger(v)&&!Object.is(v,-0),'JSON_INTEGER');
  if(typeof v==='string')assert(!/[\uD800-\uDFFF]/u.test(v),'UNICODE_SURROGATE');
  return JSON.stringify(v);
}
function scalar(kind,v) {
  if(kind==='text'||kind==='source') {
    assert(typeof v==='string'&&v.length&&!/\p{Cc}/u.test(v),'IDENTIFIER');
    // Rust char::is_whitespace follows Unicode White_Space. JavaScript \s
    // additionally includes U+FEFF, which the approved source rule permits.
    if(kind==='source')assert(/^[A-Za-z][A-Za-z0-9+.-]*:/u.test(v)&&!/\p{White_Space}/u.test(v),'SOURCE');
  } else if(['decimal','positive-decimal','decimal-percent'].includes(kind)) {
    assert(full('(0|[1-9][0-9]*)(\\.[0-9]*[1-9])?',v),'DECIMAL_CANONICAL');
    const [w,f='']=v.split('.');assert(Buffer.byteLength(v)<=64&&f.length<=18&&(w+f).replace(/^0+/u,'').length<=30,'DECIMAL_PRECISION');
    if(kind==='positive-decimal')assert(v!=='0','QUANTITY');
    if(kind==='decimal-percent')assert(BigInt(w+f)<=100n*10n**BigInt(f.length),'POLICY_PERCENT_RANGE');
  } else if(kind==='atoms'||kind==='nonnegative-atoms') {
    assert(integer(v,kind==='atoms'),'CANONICAL_INTEGER');const n=BigInt(v);assert(n>=-(10n**30n-1n)&&n<=10n**30n-1n,'ATOMS');
  } else if(kind==='uint') {
    assert(integer(v,false),'CANONICAL_INTEGER');assert(BigInt(v)<=9223372036854775807n,'COUNTER');
  } else if(kind==='slug')assert(full('[a-z][a-z0-9_.-]{0,63}',v),'SLUG');
  else if(kind==='ratio') {
    assert(v&&typeof v==='object'&&integer(v.numerator)&&integer(v.denominator,false),'RATIO_SPELLING');
    assert(v.numerator.length<=156&&v.denominator.length<=155,'RATIO_LENGTH');
    const n=BigInt(v.numerator),d=BigInt(v.denominator);assert(d>0n&&gcd(n,d)===1n&&bits(n)<=512&&bits(d)<=512,'RATIO');
  } else if(kind==='nonnegative-money') {scalar('nonnegative-atoms',v.atoms);}
  else if(kind==='time') {
    assert(full('[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\\.[0-9]{6}Z',v),'TIME');
    const [y,m,d,h,mi,sec]=[v.slice(0,4),v.slice(5,7),v.slice(8,10),v.slice(11,13),v.slice(14,16),v.slice(17,19)].map(Number);
    const days=[31,(y%4===0&&(y%100!==0||y%400===0))?29:28,31,30,31,30,31,31,30,31,30,31];
    assert(y>=1&&y<=9999&&m>=1&&m<=12&&d>=1&&d<=days[m-1]&&h<24&&mi<60&&sec<60,'TIME');
  } else throw new Error('UNKNOWN_SCALAR:'+kind);
}
export function validate(schema,value,defs=schema.$defs) {
  if(schema===true)return;if(schema===false)throw new Error('SCHEMA_FALSE');
  if(schema.$ref){assert(schema.$ref.startsWith('#/$defs/'));validate(defs[schema.$ref.slice(8)],value,defs);}
  if(schema.const!==undefined)assert.deepEqual(value,schema.const,'CONST');
  if(schema.enum)assert(schema.enum.some(x=>canonical(x)===canonical(value)),'ENUM');
  if(schema.type){
    const types=Array.isArray(schema.type)?schema.type:[schema.type];
    assert(types.some(t=>t==='array'?Array.isArray(value):t==='object'?value!==null&&!Array.isArray(value)&&typeof value==='object':t==='integer'?Number.isSafeInteger(value)&&!Object.is(value,-0):typeof value===t),'TYPE');
  }
  if(schema.oneOf)assert(schema.oneOf.filter(s=>{try{validate(s,value,defs);return true;}catch{return false;}}).length===1,'ONE_OF');
  if(schema.allOf)for(const s of schema.allOf)validate(s,value,defs);
  if(schema.if){let yes=true;try{validate(schema.if,value,defs);}catch{yes=false;}if(yes&&schema.then)validate(schema.then,value,defs);}
  if(typeof value==='string') {
    assert(!/[\uD800-\uDFFF]/u.test(value),'UNICODE_SURROGATE');
    if(schema.minLength!==undefined)assert([...value].length>=schema.minLength,'STRING_LENGTH');
    if(schema.maxLength!==undefined)assert([...value].length<=schema.maxLength,'STRING_LENGTH');
    if(schema['x-utf8-maxBytes']!==undefined)assert(Buffer.byteLength(value)<=schema['x-utf8-maxBytes'],'UTF8_BYTES');
    if(schema.pattern)assert(full(schema.pattern,value),'CANONICAL_SPELLING');
    if(schema['x-canonicalSchema']){const x=JSON.parse(value);assert.equal(canonical(x),value,'EMBEDDED_CANONICAL');validate(defs[schema['x-canonicalSchema']],x,defs);}
  }
  if(typeof value==='number') {
    if(schema.minimum!==undefined)assert(value>=schema.minimum,'MINIMUM');if(schema.maximum!==undefined)assert(value<=schema.maximum,'MAXIMUM');
  }
  if(Array.isArray(value)) {
    if(schema.minItems!==undefined)assert(value.length>=schema.minItems,'MIN_ITEMS');if(schema.maxItems!==undefined)assert(value.length<=schema.maxItems,'MAX_ITEMS');
    if(schema.uniqueItems)assert(new Set(value.map(x=>canonical(x))).size===value.length,'UNIQUE');
    value.forEach((v,i)=>{if(schema.prefixItems&&i<schema.prefixItems.length)validate(schema.prefixItems[i],v,defs);else if(schema.items!==undefined)validate(schema.items,v,defs);});
  } else if(value!==null&&typeof value==='object') {
    if(schema.required)for(const k of schema.required)assert(Object.hasOwn(value,k),'REQUIRED:'+k);
    if(schema.maxProperties!==undefined)assert(Object.keys(value).length<=schema.maxProperties,'MAX_PROPERTIES');
    for(const [k,v] of Object.entries(value)){if(schema.properties?.[k])validate(schema.properties[k],v,defs);else if(schema.additionalProperties!==undefined)validate(schema.additionalProperties,v,defs);}
  }
  if(schema['x-scalar'])scalar(schema['x-scalar'],value);
  if(schema['x-canonical-maxBytes']!==undefined)assert(Buffer.byteLength(canonical(value))<=schema['x-canonical-maxBytes'],'CANONICAL_BYTES');
}
if(import.meta.url===pathToFileURL(process.argv[1]).href) {
  const schema=JSON.parse(readFileSync(process.argv[2]));const cases=JSON.parse(readFileSync(process.argv[3]));
  for(const c of cases){let accepted=true;try{validate({$ref:'#/$defs/'+c.kind},c.value,schema.$defs);}catch{accepted=false;}assert.equal(accepted,c.accepted,JSON.stringify(c));}
  console.log(JSON.stringify({status:'passed',independent:'Node canonical scalar boundaries',cases:cases.length}));
}
