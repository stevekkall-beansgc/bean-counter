//! Frozen source-candidate-prepare/1 descriptor recipe. No caller supplies
//! authoritative locators or descriptors; all anchors follow typed source data.
use super::*;
pub(super) fn registry_digest() -> Result<String> {
    let body=canonical::parse_bounded(include_bytes!("../../../../../../../contracts/candidates/full-suite-v1/core/publication-transport-02-registry.json"),65536).map_err(|_|ServiceError::IntegrityFailure)?;
    Ok(format!(
        "sha256:{}",
        records::raw_hash(&records::bytes(&body)?)
    ))
}
fn unique(mut values: Vec<Value>) -> Result<Vec<Value>> {
    let mut map = std::collections::BTreeMap::new();
    for v in values.drain(..) {
        map.insert(records::bytes(&v)?, v);
    }
    let mut values: Vec<_> = map.into_values().collect();
    canonical::sort_set(&mut values).map_err(|_| ServiceError::IntegrityFailure)?;
    Ok(values)
}
pub(super) fn descriptors(resolution: &resolve::Resolution) -> Result<(Value, Vec<Value>)> {
    let mut locators = vec![];
    let mut anchors = vec![];
    let mut descriptors = vec![];
    if let Some(definition) = &resolution.definition {
        let root = definition.document().reference();
        let mut refs = vec![("inline_document", root.clone())];
        refs.extend(
            definition
                .direct_dependency_refs()
                .map_err(transport_error)?
                .into_iter()
                .map(|r| ("policy", r)),
        );
        for (role, r) in refs {
            locators.push(json!({"role":role,"ref":r}));
            let schemas = definition
                .descriptor_refs_for(&r)
                .map_err(transport_error)?;
            descriptors.extend(schemas.iter().map(|r| json!(r)));
            anchors.push(json!({"role":role,"record_ref":r,"relation":if role=="inline_document"{"publication_source"}else{"direct"},"descriptor_refs":schemas}));
        }
    }
    for fragment in &resolution.fragments {
        locators.push(json!({"role":"evidence","ref":fragment.reference}));
        anchors.push(json!({"role":"evidence","record_ref":fragment.reference,"relation":"evidence_input","descriptor_refs":[]}));
    }
    let locators = unique(locators)?;
    let anchors = unique(anchors)?;
    let descriptors = unique(descriptors)?;
    let mut body = json!({"schema":"ledger-operation-descriptors/1","registry_digest":registry_digest()?,"operation_kind":"evidence.retain","payload_schema":resolution.payload["schema"],"locator_digest":records::hash("descriptor-locators",1,&json!(["evidence.retain",resolution.payload["schema"],locators]))?,"anchors":anchors,"descriptors":descriptors,"normalization_profile":"ledger-operation-normalization/2"});
    body["closure_digest"] = json!(records::hash("descriptor-closure", 1, &body)?);
    Ok((body, descriptors))
}
pub(super) fn request_evidence(
    req: &request::Request,
    resolution: &resolve::Resolution,
) -> Result<Value> {
    let (descriptor_resolution, descriptors) = descriptors(resolution)?;
    let normalized = req.normalized_envelope(&resolution.payload);
    Ok(
        json!({"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2","original_request_bytes_base64":super::super::super::publication::base64(req.raw()),"original_request_bytes_sha256":records::raw_hash(req.raw()),"normalized_request":normalized,"request_hash":records::hash("operation-request",2,&json!([descriptor_resolution["closure_digest"],normalized]))?,"schema_bundle_digest":records::hash("schema-bundle",1,&json!(descriptors))?,"descriptor_resolution":descriptor_resolution}),
    )
}
