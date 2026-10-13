"""Independent Astra reviewer oracle; mechanically adapted to read-only verification.
Original SHA256 c4cdd279eb1b23c55543f62c8ea8151a01c8aabd4d06ce6e1a0eb504f217c0e5.
No production code or author comparator is imported. No file is written.
"""
import json
import argparse
import base64
from pathlib import Path
from hashlib import sha256
from copy import deepcopy
def canon(v):
    if isinstance(v,dict): return "{"+",".join(canon(k)+":"+canon(v[k]) for k in sorted(v,key=lambda x:x.encode("utf-16-be")))+"}"
    if isinstance(v,list): return "["+",".join(canon(x) for x in v)+"]"
    if isinstance(v,str): return json.dumps(v,ensure_ascii=False,separators=(",",":"))
    if isinstance(v,bool): return "true" if v else "false"
    if isinstance(v,int) and abs(v)<=9007199254740991:return str(v)
    raise TypeError(type(v))
def ordered(xs): return sorted(xs,key=lambda x:canon(x).encode())
def raw(s):return sha256(s.encode()).hexdigest()
def digest(domain,value):
    return "sha256:"+sha256(("bean-counter/"+domain+"/1").encode()+b"\0"+canon(value).encode()).hexdigest()
def framed(domain,value):return (("bean-counter/"+domain+"/1").encode()+b"\0"+canon(value).encode()).hex()
parser=argparse.ArgumentParser()
parser.add_argument("--input",type=Path,default=Path(__file__).with_name("publication-fixture-input.json"))
parser.add_argument("--registry",type=Path,default=Path(__file__).with_name("registry-contract.json"))
parser.add_argument("--expected",type=Path,default=Path(__file__).with_name("publication-independent-expected.json"))
args=parser.parse_args()
source=args.input
src=json.loads(source.read_bytes())
defs=deepcopy(src["definitions"])
for k,v in src["descriptor_digest_inputs"].items():defs["compiler"][k]="sha256:"+raw(v)
refs={};bodies={};vectors=[]
def resolve(x):
    if isinstance(x,dict):
        if list(x)==["fixture_ref"]:return refs[x["fixture_ref"]]
        return {k:resolve(v) for k,v in x.items()}
    if isinstance(x,list):return [resolve(v) for v in x]
    return x
for name in ["facts","context","template","compiler","product","policy"]:
    body=resolve(defs[name]);bodies[name]=body
    kind=body["schema"].replace("ledger-","").split("/")[0]
    d=digest("workflow-document",[body["schema"],body])
    ref={"kind":kind,"id":"wdoc_"+d[7:],"digest":d};refs[name]=ref
    vectors.append({"name":name,"body":body,"ref":ref,"canonical_body_utf8":canon(body),"canonical_body_hex":canon(body).encode().hex(),"framed_hex":framed("workflow-document",[body["schema"],body])})
sources=ordered(list(refs.values()))
policy=bodies["policy"];rule=policy["rules"][0]
activity=bodies["product"]["activities"][0]
plan={
"schema":"ledger-workflow-compiled-plan/1","compiler_ref":refs["compiler"],"compiler_semantics":"ledger-policy-compiler/1","evaluator":"ledger-workflow-evaluator/1","grammar":"ledger-policy-grammar/1","limits_profile":"workflow-limits/1","source_refs":sources,
"activity_table":[{"activity":activity["name"],"primitive":activity["primitive"],"fact_schema_ref":refs["facts"],"source_slot":activity["source_slot"],"funding_leg":activity["funding_leg"]}],
"fact_slots":[{"index":"0","schema_ref":refs["facts"],"field":bodies["facts"]["fields"][0]}],
"link_paths":[],"component_slots":bodies["template"]["components"],"rule_order":[{"policy_ref":refs["policy"],"rule_id":rule["rule_id"]}],
"typed_instructions":[{"opcode":"evaluate_rule","ordinal":"0","policy_ref":refs["policy"],"rule":rule,"fact_slots":["0"],"binding_fact_slots":[],"basis_instructions":[]}],
"dependency_edges":[],
"worst_case":{"fields":"1088","nodes":"256","hops":"16","loaded_refs":"4096","actions":"128","explanations":"256","bytes":"4194304","compile_work":"1048576","runtime_work":"65536"},
"explanation_map":[{"ordinal":"0","policy_ref":refs["policy"],"rule_or_family":rule["rule_id"],"fields":["fixture.report:pages"],"primitive":"unit"}]}
pd=digest("workflow-plan",plan);pr={"kind":"workflow-compiled-plan","id":"wplan_"+pd[7:],"digest":pd}
vectors.append({"name":"unit-plan","body":plan,"ref":pr,"canonical_body_utf8":canon(plan),"canonical_body_hex":canon(plan).encode().hex(),"framed_hex":framed("workflow-plan",plan)})
prov=src["provenance_inputs"]
pub={"schema":"ledger-workflow-publication/1","root_policy_ref":refs["policy"],"source_refs":sources,"compiled_plan_ref":pr,"compiler_ref":refs["compiler"],"provenance":{"source_sha":prov["source_sha"],"binary_sha256":raw(prov["binary_sha256_utf8"]),"build_manifest_digest":"sha256:"+raw(prov["build_manifest_digest_utf8"])}}
pubid="wpub_"+digest("workflow-publication",[src["store"],src["scope"],refs["policy"]])[7:]
pubd=digest("workflow-record",["workflow-publication",src["scope"],pub])
pubref={"kind":"workflow-publication","id":pubid,"digest":pubd}
vectors.append({"name":"publication","body":pub,"ref":pubref,"canonical_body_utf8":canon(pub),"canonical_body_hex":canon(pub).encode().hex(),"framed_hex":framed("workflow-record",["workflow-publication",src["scope"],pub])})
def literal_digest(frame,value):
    return "sha256:"+sha256(frame.encode()+b"\0"+canon(value).encode()).hexdigest()
def add_vector(name,body,ref,domain=None,hash_value=None):
    entry={"name":name,"body":body,"ref":ref,"canonical_body_utf8":canon(body),"canonical_body_hex":canon(body).encode().hex()}
    if domain:entry["framed_hex"]=framed(domain,hash_value)
    vectors.append(entry)
    return ref
basis=src["trusted_authority_fixture"]
authority={"schema":"ledger-workflow-local-authority-snapshot/1","basis":basis,"basis_digest":digest("local-authority-basis",basis),"verification_profile":"local-private-directory-owner/1","verified_at":src["accepted_at"]}
ad=digest("workflow-document",[authority["schema"],authority]); ar={"kind":"workflow-authority","id":"wdoc_"+ad[7:],"digest":ad}
add_vector("authority-snapshot",authority,ar,"workflow-document",[authority["schema"],authority])
registry_path=args.registry; registry=json.loads(registry_path.read_bytes())
registry_digest="sha256:"+raw(canon(registry))
key={"logical_store_id":src["store"],"scope":src["scope"],"source":src["source"],"operation_id":src["operation_id"]}
payload={"schema":"ledger-workflow-policy-publication/1","mode":"inline","documents":ordered(list(bodies.values())),"root_policy_ref":refs["policy"],"expected_plan_digest":pd}
request={"schema":"ledger-operation/1","key":key,"kind":"policy.publish","contract":"workflow/1","payload":payload}
locators=ordered([{"role":"inline_document","inline_document":b} for b in bodies.values()])
anchors=ordered([{"role":"inline_document","record_ref":r,"relation":"publication_source","descriptor_refs":ordered([refs["facts"],refs["context"]]) if name=="policy" else []} for name,r in refs.items()])
descriptors=ordered([refs["facts"],refs["context"],refs["compiler"]])
resolution={"schema":"ledger-operation-descriptors/1","registry_digest":registry_digest,"operation_kind":"policy.publish","payload_schema":payload["schema"],"locator_digest":digest("descriptor-locators",["policy.publish",payload["schema"],locators]),"anchors":anchors,"descriptors":descriptors,"normalization_profile":"ledger-operation-normalization/2"}
resolution["closure_digest"]=digest("descriptor-closure",resolution)
request_hash=literal_digest("bean-counter/operation-request/2",[resolution["closure_digest"],request])
schema_bundle=digest("schema-bundle",descriptors)
raw_request=canon(request).encode()
evidence={"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2","original_request_bytes_base64":base64.b64encode(raw_request).decode(),"original_request_bytes_sha256":sha256(raw_request).hexdigest(),"normalized_request":request,"request_hash":request_hash,"schema_bundle_digest":schema_bundle,"descriptor_resolution":resolution}
results=ordered([pubref,pr])
op={"schema":"ledger-workflow-operation-record/1","key":key,"kind":"policy.publish","contract":"workflow/1","request_evidence":evidence,"authority_snapshot":{"schema":"ledger-workflow-publication-authority/1","principal_id":src["principal"],"source":src["source"],"authority_refs":[ar],"rights":["admin"],"validated_at":src["accepted_at"]},"accepted_at":src["accepted_at"],"commit_sequence":src["commit_sequence"],"result_refs":results}
od=digest("workflow-record",["workflow-operation",src["scope"],op]);oref={"kind":"workflow-operation","id":"wop_"+digest("workflow-operation",key)[7:],"digest":od}
add_vector("operation",op,oref,"workflow-record",["workflow-operation",src["scope"],op])
manifest={"schema":"ledger-workflow-publication-manifest/1","key":key,"members":ordered(sources+[ar,pr,pubref,oref])}
md=digest("workflow-manifest",manifest);mref={"kind":"workflow-manifest","id":"wmanifest_"+md[7:],"digest":md}
add_vector("manifest",manifest,mref,"workflow-manifest",manifest)
receipt={"schema":"ledger-operation-receipt/2","key":key,"request_hash":request_hash,"request_hash_profile":"bean-counter/operation-request/2","normalization_profile":"ledger-operation-normalization/2","schema_bundle_digest":schema_bundle,"descriptor_closure_digest":resolution["closure_digest"],"original_request_bytes_sha256":sha256(raw_request).hexdigest(),"result_kind":"administrative","result_refs":results,"accepted_at":src["accepted_at"],"commit_sequence":src["commit_sequence"],"manifest_hash":md}
rd=literal_digest("bean-counter/operation-receipt/2",receipt);receipt_frame=(b"bean-counter/operation-receipt/2\0"+canon(receipt).encode()).hex()
receipt["receipt_hash"]=rd;rref={"kind":"workflow-receipt","id":"wreceipt_"+rd[7:],"digest":rd}
add_vector("receipt",receipt,rref);vectors[-1]["framed_hex"]=receipt_frame
records=ordered([{"kind":x["ref"]["kind"],"logical_store_id":src["store"],"scope":src["scope"],"id":x["ref"]["id"],"digest":x["ref"]["digest"],"body":x["body"]} for x in vectors])
status={"schema":"ledger-operation-status/1","status":"committed","receipt":receipt,"request_evidence":evidence}
out={"status":"INDEPENDENT_PUBLICATION_EXPECTATIONS_NOT_PRODUCT_PASS_OR_FREEZE","input_path":str(source),"input_sha256":sha256(source.read_bytes()).hexdigest(),"registry_path":str(registry_path),"registry_input_sha256":sha256(registry_path.read_bytes()).hexdigest(),"registry_digest":registry_digest,"vectors":vectors,"locators":locators,"descriptor_resolution":resolution,"normalized_request":request,"original_request_hex":raw_request.hex(),"request_evidence":evidence,"envelopes":records,"status_result":status,"member_count":len(manifest["members"]),"envelope_count":len(records),"pending":["independent author comparator agreement","semantic negative/authority/reopen qualification; no product tests run"],"economic_sanity":{"quantity":"3","rate":"0.37","exact_atoms":"1110000000000000000","note":"manual unit math only; publication posts no charge"}}
expected=json.loads(args.expected.read_bytes())
# These two fields identify original review workspace paths, not canonical values.
for field in out:
    if field not in {"input_path","registry_path"}:
        assert out[field] == expected[field], field
print("independent read-only oracle: 12 envelope bodies/refs/bytes, descriptors, request, evidence and status reproduce exactly; runtime histories UNRUN")
