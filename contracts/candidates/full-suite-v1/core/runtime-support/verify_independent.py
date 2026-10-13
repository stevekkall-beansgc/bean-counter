"""Independent support descriptor derivation. Reads three catalog inputs only.
No author expected descriptor, generator or production code is imported.
"""
import json
from pathlib import Path
from hashlib import sha256
root=Path(__file__).resolve().parent
def canon(v):
 if isinstance(v,dict):return '{'+','.join(canon(k)+':'+canon(v[k]) for k in sorted(v,key=lambda s:s.encode('utf-16-be')))+'}'
 if isinstance(v,list):return '['+','.join(canon(x) for x in v)+']'
 if isinstance(v,str):return json.dumps(v,ensure_ascii=False,separators=(',',':'))
 if v is True:return 'true'
 if v is False:return 'false'
 if isinstance(v,int) and abs(v)<=9007199254740991:return str(v)
 raise TypeError(type(v))
catalogs={name:json.loads((root/(name+'.json')).read_bytes()) for name in ('limits','mode-matrix','contract-profile')}
catalog_evidence={name:{'file_sha256':sha256((root/(name+'.json')).read_bytes()).hexdigest(),'canonical_utf8':canon(body),'canonical_hex':canon(body).encode().hex(),'digest':'sha256:'+sha256(canon(body).encode()).hexdigest()} for name,body in catalogs.items()}
m=catalogs['mode-matrix']
d={'schema':'ledger-workflow-compiler-descriptor/1','compiler_semantics':'ledger-policy-compiler/1','evaluator':'ledger-workflow-evaluator/1','grammar':'ledger-policy-grammar/1','facts_normalization':'ledger-facts-normalization/1','operation_normalization':'ledger-operation-normalization/2','limits_profile':'workflow-limits/1','limits_digest':catalog_evidence['limits']['digest'],'primitive_versions':sorted(m['primitives'],key=canon),'fact_types':sorted(m['fact_types'],key=canon),'predicate_ops':sorted(m['predicate_ops'],key=canon),'formula_kinds':sorted(m['formula_kinds'],key=canon),'mode_matrix_digest':catalog_evidence['mode-matrix']['digest'],'plan_schema':'ledger-workflow-compiled-plan/1','contract_manifest_digest':catalog_evidence['contract-profile']['digest']}
body=canon(d);framed=b'bean-counter/workflow-document/1\0'+canon([d['schema'],d]).encode();digest=sha256(framed).hexdigest()
out={'status':'INDEPENDENT_RUNTIME_CATALOG_EXPECTATIONS_NOT_BINARY_OR_RUNTIME_QUALIFICATION','catalogs':catalog_evidence,'descriptor':d,'canonical_utf8':body,'canonical_hex':body.encode().hex(),'framed_hex':framed.hex(),'ref':{'kind':'workflow-compiler-descriptor','id':'wdoc_'+digest,'digest':'sha256:'+digest}}
expected=json.loads((root/'independent-expected.json').read_text())
assert out==expected, 'Independent catalog descriptor reproduction differs'
print('Independent catalog inputs reproduce exact descriptor/ref/bytes; runtime support UNRUN.')
