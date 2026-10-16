//! Saved agreement descriptor recipe. Its inputs are data, not authority.
use super::*;

fn reference(v: &Value) -> Result<RecordRef> {
    ledgerlab_core::policy::workflow::AgreementRef::parse(&records::bytes(v)?)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    serde_json::from_value(v.clone()).map_err(|_| ServiceError::IntegrityFailure)
}
pub(super) fn locators(kind: &str, payload: &Value) -> Result<Vec<Value>> {
    let mut out = std::collections::BTreeMap::new();
    let mut add = |role: &str, v: &Value| -> Result<()> {
        let r = reference(v)?;
        let locator = json!({"role":role,"ref":r});
        out.insert(records::bytes(&locator)?, locator);
        Ok(())
    };
    match kind {
        "agreement.accept" => {
            add("publication", &payload["publication_ref"])?;
            let input = &payload["binding_inputs"];
            for field in ["slot_catalog_ref", "slot_instantiation_ref"] {
                add("private_document", &payload[field])?;
                add("private_document", &input[field])?;
            }
            for values in [
                &payload["assent_refs"],
                &payload["authority_refs"],
                &input["assent_refs"],
                &input["authority_refs"],
                &input["roles_refs"],
            ] {
                for r in values.as_array().ok_or(ServiceError::IntegrityFailure)? {
                    add("private_document", r)?;
                }
            }
            add(
                "private_document",
                &input["accepted_context"]["fact_schema_ref"],
            )?;
            for b in input["bindings"]
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                add("private_document", &b["binding_ref"])?;
            }
            for leg in input["funding_legs"]
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                for r in leg["evidence_refs"]
                    .as_array()
                    .ok_or(ServiceError::IntegrityFailure)?
                {
                    add("private_document", r)?;
                }
            }
        }
        "agreement.activate" | "agreement.revoke" => {
            add("accepted_binding", &payload["accepted_binding_ref"])?;
            for field in ["transition_ref", "replaces_transition_ref"] {
                if let Some(r) = payload.get(field) {
                    add("activation_target", r)?;
                }
            }
            for r in payload["evidence_refs"]
                .as_array()
                .ok_or(ServiceError::IntegrityFailure)?
            {
                add("private_document", r)?;
            }
        }
        _ => return Err(ServiceError::IntegrityFailure),
    }
    let mut out: Vec<_> = out.into_values().collect();
    canonical::sort_set(&mut out).map_err(|_| ServiceError::IntegrityFailure)?;
    Ok(out)
}
/// `descriptors` must come from the original origin-authenticated publication
/// (or accepted binding). This encoder cannot turn supplied refs into proof.
pub(super) fn evidence(
    req: &request::Request,
    payload: &Value,
    descriptors: &[RecordRef],
) -> Result<Value> {
    let locators = locators(req.kind(), payload)?;
    let mut descriptors = descriptors.to_vec();
    records::sort_refs(&mut descriptors)?;
    if descriptors.iter().any(|r| {
        !matches!(
            r.kind.as_str(),
            "workflow-fact-schema" | "workflow-compiler-descriptor"
        )
    }) {
        return Err(ServiceError::IntegrityFailure);
    }
    let mut anchors = vec![];
    for l in &locators {
        let r = reference(&l["ref"])?;
        let (relation, refs) = match l["role"].as_str() {
            Some("publication") => ("publication_source", descriptors.clone()),
            Some("accepted_binding") => ("accepted_binding", descriptors.clone()),
            Some("activation_target") => ("activation_target", vec![]),
            Some("private_document")
                if req.kind() == "agreement.accept"
                    && json!(r)
                        == payload["binding_inputs"]["accepted_context"]["fact_schema_ref"] =>
            {
                if !descriptors.contains(&r) {
                    return Err(ServiceError::IntegrityFailure);
                }
                ("context_schema", vec![r.clone()])
            }
            Some("private_document") => ("agreement_input", vec![]),
            _ => return Err(ServiceError::IntegrityFailure),
        };
        anchors.push(
            json!({"role":l["role"],"record_ref":r,"relation":relation,"descriptor_refs":refs}),
        );
    }
    canonical::sort_set(&mut anchors).map_err(|_| ServiceError::IntegrityFailure)?;
    let registry_raw: &[u8] = if req.kind() == "agreement.accept"
        && payload["schema"] == "ledger-workflow-agreement-accept/4"
    {
        include_bytes!(
            "../../../../../../contracts/candidates/full-suite-v1/core/exposure-scope-01/registry.json"
        )
    } else if req.kind() == "agreement.accept"
        && payload["schema"] == "ledger-workflow-agreement-accept/3"
    {
        include_bytes!(
            "../../../../../../contracts/candidates/full-suite-v1/core/causal-reversal-02-registry.json"
        )
    } else {
        include_bytes!(
            "../../../../../../contracts/candidates/full-suite-v1/core/agreement-ready-registry.json"
        )
    };
    let registry = canonical::parse_bounded(registry_raw, 65536)
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let mut resolution = json!({"schema":"ledger-operation-descriptors/1","registry_digest":format!("sha256:{}",records::raw_hash(&records::bytes(&registry)?)),"operation_kind":req.kind(),"payload_schema":payload["schema"],"locator_digest":records::hash("descriptor-locators",1,&json!([req.kind(),payload["schema"],locators]))?,"anchors":anchors,"descriptors":descriptors,"normalization_profile":"ledger-operation-normalization/2"});
    resolution["closure_digest"] = json!(records::hash("descriptor-closure", 1, &resolution)?);
    let normalized = req.normalized_envelope(payload);
    Ok(
        json!({"schema":"ledger-operation-request-evidence/2","normalization_profile":"ledger-operation-normalization/2","original_request_bytes_base64":super::super::publication::base64(req.raw()),"original_request_bytes_sha256":records::raw_hash(req.raw()),"normalized_request":normalized,"request_hash":records::hash("operation-request",2,&json!([resolution["closure_digest"],normalized]))?,"schema_bundle_digest":records::hash("schema-bundle",1,&json!(descriptors))?,"descriptor_resolution":resolution}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn causal_acceptance_uses_its_exact_additive_registry() {
        let f: Value =
            serde_json::from_slice(include_bytes!("testdata/descriptor-independent.json")).unwrap();
        let old = &f["operations"][0];
        let mut envelope = old["request_evidence"]["normalized_request"].clone();
        assert_eq!(envelope["kind"], "agreement.accept");
        envelope["payload"]["schema"] = json!("ledger-workflow-agreement-accept/3");
        let req = request::Request::parse_agreement(&records::bytes(&envelope).unwrap()).unwrap();
        let descriptors: Vec<RecordRef> =
            serde_json::from_value(old["descriptor_resolution"]["descriptors"].clone()).unwrap();
        let evidence = evidence(&req, req.payload(), &descriptors).unwrap();
        assert_eq!(
            evidence["descriptor_resolution"]["registry_digest"],
            "sha256:b9397d8545fccefeb32ff9c320f1f70c96b1c89d6bd58f7aba37068862ab6d54"
        );
        assert_ne!(
            evidence["request_hash"],
            old["request_evidence"]["request_hash"]
        );
        assert_eq!(evidence["normalized_request"], envelope);
    }
    #[test]
    fn exposure_acceptance_uses_only_its_frozen_registry() {
        let f: Value =
            serde_json::from_slice(include_bytes!("testdata/descriptor-independent.json")).unwrap();
        let old = &f["operations"][0];
        let mut envelope = old["request_evidence"]["normalized_request"].clone();
        envelope["payload"]["schema"] = json!("ledger-workflow-agreement-accept/4");
        let req = request::Request::parse_agreement(&records::bytes(&envelope).unwrap()).unwrap();
        let descriptors: Vec<RecordRef> =
            serde_json::from_value(old["descriptor_resolution"]["descriptors"].clone()).unwrap();
        let result = evidence(&req, req.payload(), &descriptors).unwrap();
        assert_eq!(
            result["descriptor_resolution"]["registry_digest"],
            "sha256:5fd45e8ac7af2bc4079e14074c935f30982fb88ac09765500ba67666a73fc4f4"
        );
        assert_ne!(
            result["request_hash"],
            old["request_evidence"]["request_hash"]
        );
        assert_eq!(result["normalized_request"], envelope);
    }
    #[test]
    fn five_independent_agreement_descriptor_and_raw_evidence_vectors() {
        let f: Value =
            serde_json::from_slice(include_bytes!("testdata/descriptor-independent.json")).unwrap();
        let operations = f["operations"].as_array().unwrap();
        assert_eq!(operations.len(), 5);
        for o in operations {
            let expected = &o["request_evidence"];
            let raw = records::bytes(&expected["normalized_request"]).unwrap();
            let req = request::Request::parse_agreement(&raw).unwrap();
            let descriptors: Vec<RecordRef> =
                serde_json::from_value(o["descriptor_resolution"]["descriptors"].clone()).unwrap();
            assert_eq!(
                locators(req.kind(), req.payload()).unwrap(),
                serde_json::from_value::<Vec<Value>>(o["locators"].clone()).unwrap()
            );
            assert_eq!(
                evidence(&req, req.payload(), &descriptors).unwrap(),
                *expected
            );
            let pretty = serde_json::to_vec_pretty(&expected["normalized_request"]).unwrap();
            let alternate = request::Request::parse_agreement(&pretty).unwrap();
            let alternate = evidence(&alternate, req.payload(), &descriptors).unwrap();
            assert_eq!(alternate["request_hash"], expected["request_hash"]);
            assert_ne!(
                alternate["original_request_bytes_sha256"],
                expected["original_request_bytes_sha256"]
            );
        }
    }
}
