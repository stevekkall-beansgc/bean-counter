// Independent exhaustive schema validation for Unicode text/source classes.
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {join} from 'node:path';
import {validate} from './scalars.mjs';

const root=process.argv[2], expected=JSON.parse(readFileSync(process.argv[3]));
const schema=JSON.parse(readFileSync(join(root,'contracts/candidates/v2/schemas/canonical-records.schema.json')));
const rejected={text:[],source:[]};let count=0;
for(let cp=0;cp<=0x10ffff;cp++){
  if(cp>=0xd800&&cp<=0xdfff)continue;
  count++;const character=String.fromCodePoint(cp);
  for(const [kind,value] of [['text','a'+character],['source','urn:synthetic:'+character+'outcome']]){
    try{validate(schema.$defs[kind],value,schema.$defs);}catch{rejected[kind].push(cp);}
  }
}
assert.equal(count,expected.scalar_values);
assert.deepEqual(rejected,expected.rejected,'NODE_UNICODE_CLASSIFICATION');
console.log(JSON.stringify({status:'passed',independent:'Node exhaustive Unicode text/source schema parity',scalar_values:count,checks:count*2}));
