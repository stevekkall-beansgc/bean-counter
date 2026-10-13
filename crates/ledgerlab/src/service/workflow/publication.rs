use super::*;
use crate::service::billing::Setup;
use crate::store::sqlite::workflow::{
    Document, PublicationAppend, RecordRef, RegistryEntry, StoredOperation,
};
use ledgerlab_core::policy::workflow::{
    self as core, NormalizedPublicationSources, PreparedPublication,
};
use serde_json::json;

fn reference(r: &core::DefinitionRef) -> RecordRef {
    RecordRef {
        kind: r.kind().into(),
        id: r.id().into(),
        digest: r.digest().into(),
    }
}
fn document(d: &core::DefinitionDocument) -> Document {
    Document {
        reference: reference(d.reference()),
        schema: d.canonical_value()["schema"]
            .as_str()
            .expect("validated document")
            .into(),
        bytes: d.canonical_bytes().to_vec(),
    }
}
pub(super) fn normalize_request(
    req: &request::PublicationRequest,
    sources: &NormalizedPublicationSources,
) -> Result<Value> {
    let mut v = req.value()?;
    if req.inline_documents().is_some() {
        v["payload"]["documents"] = json!(
            sources
                .source_documents()
                .iter()
                .map(|d| d.canonical_value())
                .collect::<Vec<_>>()
        );
    } else {
        let mut refs = req
            .retained_refs()
            .ok_or(ServiceError::IntegrityFailure)?
            .to_vec();
        records::sort_refs(&mut refs)?;
        v["payload"]["document_refs"] = json!(refs);
    }
    Ok(v)
}
pub(super) fn normalized(
    req: &request::PublicationRequest,
    raw_documents: &[Vec<u8>],
) -> Result<NormalizedPublicationSources> {
    let root = core::DefinitionRef::new(&req.root().kind, &req.root().id, &req.root().digest)
        .map_err(|e| reject(e.code))?;
    core::normalize_publication_sources(raw_documents, &root).map_err(|e| reject(e.code))
}
fn base64(raw: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(raw.len().div_ceil(3) * 4);
    for c in raw.chunks(3) {
        s.push(A[(c[0] >> 2) as usize] as char);
        s.push(A[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char);
        s.push(if c.len() > 1 {
            A[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            A[(c[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}
pub(super) fn evidence(
    req: &request::PublicationRequest,
    sources: &NormalizedPublicationSources,
) -> Result<Value> {
    let normalized = normalize_request(req, sources)?;
    let mut locators: Vec<Value> = if req.inline_documents().is_some() {
        sources
            .source_documents()
            .iter()
            .map(|d| json!({"role":"inline_document","inline_document":d.canonical_value()}))
            .collect()
    } else {
        req.retained_refs()
            .ok_or(ServiceError::IntegrityFailure)?
            .iter()
            .map(|r| json!({"role":"inline_document","ref":r}))
            .collect()
    };
    ledgerlab_core::canonical::sort_set(&mut locators)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let descriptors = sources.descriptor_refs().map_err(|e| reject(e.code))?;
    let mut anchors = vec![];
    for d in sources.source_documents() {
        let mut refs: Vec<Value> = vec![];
        if d.kind() == "workflow-policy" {
            let policy = d.canonical_value();
            let product = sources
                .source_documents()
                .iter()
                .find(|p| json!(p.reference()) == policy["product_ref"])
                .ok_or(ServiceError::IntegrityFailure)?;
            refs = product.canonical_value()["activities"]
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
                .iter()
                .map(|a| a["fact_schema"].clone())
                .collect();
            refs.push(policy["context_schema_ref"].clone());
            let mut unique = std::collections::BTreeMap::new();
            for r in refs {
                unique.insert(records::bytes(&r)?, r);
            }
            refs = unique.into_values().collect();
            ledgerlab_core::canonical::sort_set(&mut refs)
                .map_err(|_| ServiceError::IntegrityFailure)?;
        }
        anchors.push(json!({"role":"inline_document","record_ref":d.reference(),"relation":"publication_source","descriptor_refs":refs}));
    }
    ledgerlab_core::canonical::sort_set(&mut anchors)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let registry = ledgerlab_core::canonical::parse_bounded(
        include_bytes!(
            "../../../../../contracts/candidates/full-suite-v1/core/registry-contract.json"
        ),
        1024 * 1024,
    )
    .map_err(|_| ServiceError::IntegrityFailure)?;
    let mut resolution = json!({"schema":"ledger-operation-descriptors/1","registry_digest":format!("sha256:{}",records::raw_hash(&records::bytes(&registry)?)),"operation_kind":"policy.publish","payload_schema":"ledger-workflow-policy-publication/1","locator_digest":records::hash("descriptor-locators",1,&json!(["policy.publish","ledger-workflow-policy-publication/1",locators]))?,"anchors":anchors,"descriptors":descriptors,"normalization_profile":"ledger-operation-normalization/2"});
    resolution["closure_digest"] = json!(records::hash("descriptor-closure", 1, &resolution)?);
    Ok(
        json!({"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2","original_request_bytes_base64":base64(req.raw()),"original_request_bytes_sha256":records::raw_hash(req.raw()),"normalized_request":normalized,"request_hash":records::hash("operation-request",2,&json!([resolution["closure_digest"],normalized]))?,"schema_bundle_digest":records::hash("schema-bundle",1,&json!(descriptors))?,"descriptor_resolution":resolution}),
    )
}
/// Code-owned metadata from the actual local artifact, not a wire value.
pub(super) struct BuildProvenance(Value);
impl BuildProvenance {
    pub(super) fn capture() -> Result<Self> {
        // Fresh publication remains held until PUB-BUILD-01's reviewed embedded
        // BuildInputs and stable executable observation produce a retained manifest.
        Err(reject("BUILD_IDENTITY_REQUIRED"))
    }
}
pub(super) fn assemble(
    req: &request::PublicationRequest,
    prepared: &PreparedPublication,
    evidence: Value,
    setup: &Setup,
    at: &ledgerlab_core::domain::Timestamp,
    sequence: i64,
    provenance: &BuildProvenance,
    existing: Option<Document>,
) -> Result<PublicationAppend> {
    let key = req.key();
    let mut docs: Vec<Document> = prepared.source_documents().iter().map(document).collect();
    let mut source_refs: Vec<RecordRef> = docs.iter().map(|d| d.reference.clone()).collect();
    records::sort_refs(&mut source_refs)?;
    let plan = document(prepared.compiled_plan());
    let plan_ref = plan.reference.clone();
    docs.push(plan);
    let publication_body = json!({"schema":"ledger-workflow-publication/1","root_policy_ref":prepared.root_policy_ref(),"source_refs":source_refs,"compiled_plan_ref":plan_ref,"compiler_ref":prepared.compiler_descriptor().reference(),"provenance":provenance.0});
    let is_alias = existing.is_some();
    let publication = if let Some(old) = existing {
        let body = ledgerlab_core::canonical::parse_bounded(&old.bytes, 1024 * 1024)
            .map_err(|_| ServiceError::IntegrityFailure)?;
        require(
            body["root_policy_ref"] == publication_body["root_policy_ref"]
                && body["source_refs"] == publication_body["source_refs"]
                && body["compiled_plan_ref"] == publication_body["compiled_plan_ref"]
                && body["compiler_ref"] == publication_body["compiler_ref"],
            "SEMANTIC_CONFLICT",
        )?;
        old
    } else {
        Document {
            reference: records::publication_ref(
                &key.logical_store_id,
                &key.scope,
                &publication_body,
            )?,
            schema: "ledger-workflow-publication/1".into(),
            bytes: records::bytes(&publication_body)?,
        }
    };
    let pub_ref = publication.reference.clone();
    docs.push(publication);
    let basis = json!({"schema":"ledger-workflow-local-authority-basis/1","logical_store_id":setup.store_id,"scope":setup.scope,"principal_id":setup.operator,"source":setup.source,"canonical_economic_namespace":setup.source,"rights":["admin","read"],"source_grant_revision":setup.grant_revision.to_string()});
    let authority = records::source_document(
        "workflow-authority",
        json!({"schema":"ledger-workflow-local-authority-snapshot/1","basis":basis,"basis_digest":records::hash("local-authority-basis",1,&basis)?,"verification_profile":"local-private-directory-owner/1","verified_at":at}),
    )?;
    let auth_ref = authority.reference.clone();
    docs.push(authority);
    let mut results = vec![plan_ref.clone(), pub_ref.clone()];
    records::sort_refs(&mut results)?;
    let operation = json!({"schema":"ledger-workflow-operation-record/1","key":key,"kind":"policy.publish","contract":"workflow/1","request_evidence":evidence,"authority_snapshot":{"schema":"ledger-workflow-publication-authority/1","principal_id":setup.operator,"source":setup.source,"authority_refs":[auth_ref],"rights":["admin"],"validated_at":at},"accepted_at":at,"commit_sequence":sequence.to_string(),"result_refs":results});
    let operation_ref = records::record_reference("workflow-operation", &key.scope, &operation)?;
    let mut members: Vec<RecordRef> = docs.iter().map(|d| d.reference.clone()).collect();
    members.push(operation_ref.clone());
    records::sort_refs(&mut members)?;
    let manifest =
        json!({"schema":"ledger-workflow-publication-manifest/1","key":key,"members":members});
    let manifest_ref = records::record_reference("workflow-manifest", &key.scope, &manifest)?;
    let mut receipt = json!({"schema":"ledger-operation-receipt/2","key":key,"request_hash":evidence["request_hash"],"request_hash_profile":"bean-counter/operation-request/2","normalization_profile":"ledger-operation-normalization/2","schema_bundle_digest":evidence["schema_bundle_digest"],"descriptor_closure_digest":evidence["descriptor_resolution"]["closure_digest"],"original_request_bytes_sha256":evidence["original_request_bytes_sha256"],"result_kind":if is_alias{"semantic_alias"}else{"administrative"},"result_refs":results,"accepted_at":at,"commit_sequence":sequence.to_string(),"manifest_hash":manifest_ref.digest});
    receipt["receipt_hash"] = json!(records::hash("operation-receipt", 2, &receipt)?);
    let receipt_ref = records::record_reference("workflow-receipt", &key.scope, &receipt)?;
    let registry = prepared
        .source_documents()
        .iter()
        .filter_map(|d| {
            let body = d.canonical_value();
            let field = match d.kind() {
                "workflow-product" => "product_id",
                "workflow-policy" => "policy_id",
                "workflow-slot-template" => "template_id",
                "workflow-fact-schema" => "schema_id",
                _ => return None,
            };
            Some(RegistryEntry {
                kind: d.kind().into(),
                semantic_id: body[field].as_str().expect("validated identity").into(),
                version: body["version"].as_str().expect("validated version").into(),
                document_id: d.id().into(),
            })
        })
        .collect();
    Ok(PublicationAppend {
        store_id: key.logical_store_id.clone(),
        tenant: key.scope[0].clone(),
        environment: key.scope[1].clone(),
        source: key.source.clone(),
        operation_id: key.operation_id.clone(),
        documents: docs,
        registry,
        publication_key: pub_ref.id.as_bytes().to_vec(),
        root_policy_id: req.root().id.clone(),
        operation: StoredOperation {
            identity: key.bytes()?,
            operation_ref,
            operation_bytes: records::bytes(&operation)?,
            request_hash: evidence["request_hash"]
                .as_str()
                .ok_or(ServiceError::IntegrityFailure)?
                .into(),
            manifest_ref,
            manifest_bytes: records::bytes(&manifest)?,
            receipt_ref,
            receipt_bytes: records::bytes(&receipt)?,
            publication_id: pub_ref.id,
            plan_id: plan_ref.id,
            sequence,
            accepted_at_us: at.micros(),
            members,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtin_prepared_publication_assembles_all_independent_envelope_bytes() {
        let expected:Value=serde_json::from_str(include_str!("../../../../../contracts/candidates/full-suite-v1/core/runtime-support/publication-independent-expected.json")).unwrap();
        let vectors = expected["vectors"].as_array().unwrap();
        let vector = |name: &str| vectors.iter().find(|v| v["name"] == name).unwrap();
        let req = request::PublicationRequest::parse(
            &records::bytes(&expected["normalized_request"]).unwrap(),
        )
        .unwrap();
        let raw: Vec<Vec<u8>> = vectors[..6]
            .iter()
            .map(|v| {
                v["canonical_body_utf8"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .to_vec()
            })
            .collect();
        let root =
            core::DefinitionRef::parse(&records::bytes(&vector("policy")["ref"]).unwrap()).unwrap();
        let prepared = core::compile_publication(
            &raw,
            &root,
            vector("unit-plan")["ref"]["digest"].as_str().unwrap(),
            &core::VerifiedCompilerSupport::builtin().unwrap(),
        )
        .unwrap();
        let sources = normalized(&req, &raw).unwrap();
        let evidence = evidence(&req, &sources).unwrap();
        assert_eq!(evidence, expected["request_evidence"]);
        let setup_raw = include_bytes!("../../../../../examples/billing/setup.json");
        let mut setup = Setup::parse(setup_raw).unwrap();
        setup.store_id = req.key().logical_store_id.clone();
        setup.scope =
            ledgerlab_core::domain::Scope::new(&req.key().scope[0], &req.key().scope[1]).unwrap();
        setup.source = req.key().source.clone();
        setup.operator = vector("authority-snapshot")["body"]["basis"]["principal_id"]
            .as_str()
            .unwrap()
            .into();
        let at = ledgerlab_core::domain::Timestamp::parse(
            vector("operation")["body"]["accepted_at"].as_str().unwrap(),
        )
        .unwrap();
        // Known synthetic provenance only within this private encoding test.
        let provenance = BuildProvenance(vector("publication")["body"]["provenance"].clone());
        let append =
            assemble(&req, &prepared, evidence, &setup, &at, 1, &provenance, None).unwrap();
        for d in &append.documents {
            let v = vectors
                .iter()
                .find(|v| v["ref"] == json!(d.reference))
                .unwrap();
            assert_eq!(
                d.bytes,
                v["canonical_body_utf8"].as_str().unwrap().as_bytes()
            );
        }
        for (name, r, b) in [
            (
                "operation",
                &append.operation.operation_ref,
                &append.operation.operation_bytes,
            ),
            (
                "manifest",
                &append.operation.manifest_ref,
                &append.operation.manifest_bytes,
            ),
            (
                "receipt",
                &append.operation.receipt_ref,
                &append.operation.receipt_bytes,
            ),
        ] {
            assert_eq!(json!(r), vector(name)["ref"]);
            assert_eq!(
                b,
                vector(name)["canonical_body_utf8"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
            );
        }
    }
}
