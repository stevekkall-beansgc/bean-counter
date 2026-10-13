//! Exact semantic record identity. These data factories authenticate no actor,
//! assent or grant; the coordinator and retained certificate do that separately.
use super::super::request::OperationKey;
use super::*;
pub(super) fn semantic_document(
    kind: &str,
    key: &OperationKey,
    customer: &str,
    agreement: &str,
    revision: u64,
    body: Value,
) -> Result<Document> {
    if revision == 0 {
        return Err(ServiceError::IntegrityFailure);
    }
    let revision = revision.to_string();
    let (prefix, domain, identity) = match kind {
        "workflow-accepted-agreement" => {
            exact_keys(
                &body,
                &[
                    "schema",
                    "operation_key",
                    "scope",
                    "customer",
                    "agreement_id",
                    "version",
                    "publication_ref",
                    "binding_ref",
                    "terms_ref",
                    "terms_digest",
                    "authority_observation_ref",
                    "accepted_at",
                    "commit_sequence",
                ],
            )?;
            if body["schema"] != "ledger-workflow-accepted-agreement/1"
                || body["operation_key"] != json!(key)
                || body["scope"] != json!(key.scope)
                || body["customer"] != customer
                || body["agreement_id"] != agreement
                || body["version"] != revision
            {
                return Err(ServiceError::IntegrityFailure);
            }
            (
                "waccepted_",
                "workflow-agreement-acceptance",
                json!([
                    key.logical_store_id,
                    key.scope,
                    customer,
                    agreement,
                    revision
                ]),
            )
        }
        "workflow-head-transition" => {
            let mut keys = vec![
                "schema",
                "head_kind",
                "scope",
                "customer",
                "agreement_id",
                "operation_key",
                "actor",
                "old_revision",
                "new_revision",
                "accepted_binding_ref",
                "accepted_at",
                "commit_sequence",
            ];
            if body.get("prior_head_ref").is_some() {
                keys.push("prior_head_ref");
            }
            if body.get("resolved_effective_time").is_some() {
                keys.push("resolved_effective_time");
            }
            exact_keys(&body, &keys)?;
            let old = text(&body, "old_revision")?
                .parse::<u64>()
                .map_err(|_| ServiceError::IntegrityFailure)?;
            let old_text = old.to_string();
            if body["schema"] != "ledger-workflow-head-transition/1"
                || body["operation_key"] != json!(key)
                || body["scope"] != json!(key.scope)
                || body["customer"] != customer
                || body["agreement_id"] != agreement
                || body["new_revision"] != revision
                || body["old_revision"] != old_text.as_str()
                || old.checked_add(1).map(|n| n.to_string()).as_deref() != Some(revision.as_str())
                || !matches!(
                    body["head_kind"].as_str(),
                    Some("agreement_acceptance" | "agreement_activation")
                )
                || (old == 0) != body.get("prior_head_ref").is_none()
            {
                return Err(ServiceError::IntegrityFailure);
            }
            (
                "whead_",
                "workflow-head-transition",
                json!([
                    key.logical_store_id,
                    key.scope,
                    customer,
                    agreement,
                    body["head_kind"],
                    revision
                ]),
            )
        }
        "workflow-activation" => {
            let mut keys = vec![
                "schema",
                "action",
                "head_transition_ref",
                "binding_ref",
                "effective_at",
                "commit_sequence",
                "accepted_at",
            ];
            if body.get("target_transition_ref").is_some() {
                keys.push("target_transition_ref");
            }
            exact_keys(&body, &keys)?;
            if body["schema"] != "ledger-workflow-activation-transition/1"
                || !matches!(
                    body["action"].as_str(),
                    Some("activate" | "replace" | "revoke")
                )
                || (body["action"] == "activate") != body.get("target_transition_ref").is_none()
            {
                return Err(ServiceError::IntegrityFailure);
            }
            (
                "wactivation_",
                "workflow-activation-transition",
                json!([
                    key.logical_store_id,
                    key.scope,
                    customer,
                    agreement,
                    revision
                ]),
            )
        }
        _ => return Err(ServiceError::IntegrityFailure),
    };
    Timestamp::parse(text(&body, "accepted_at")?).map_err(|_| ServiceError::IntegrityFailure)?;
    let sequence = text(&body, "commit_sequence")?
        .parse::<u64>()
        .map_err(|_| ServiceError::IntegrityFailure)?;
    let sequence_text = sequence.to_string();
    if sequence == 0 || body["commit_sequence"] != sequence_text.as_str() {
        return Err(ServiceError::IntegrityFailure);
    }
    let id = records::hash(domain, 1, &identity)?;
    let digest = records::hash("workflow-record", 1, &json!([kind, key.scope, body]))?;
    let bytes = records::bytes(&body)?;
    require(bytes.len() <= 262144, "PROGRAM_BUDGET_EXCEEDED")?;
    Ok(Document {
        reference: RecordRef {
            kind: kind.into(),
            id: format!("{prefix}{}", &id[7..]),
            digest,
        },
        schema: text(&body, "schema")?.into(),
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn semantic_ids_and_record_bytes_match_sealed_independent_agr03() {
        let expected: Value =
            serde_json::from_slice(include_bytes!("testdata/semantic-independent.json")).unwrap();
        let vectors = expected["vectors"].as_array().unwrap();
        for v in vectors {
            let head = if v["ref"]["kind"] == "workflow-activation" {
                vectors
                    .iter()
                    .find(|h| h["ref"] == v["body"]["head_transition_ref"])
                    .unwrap()
            } else {
                v
            };
            let body = &head["body"];
            let key =
                OperationKey::parse(&records::bytes(&body["operation_key"]).unwrap()).unwrap();
            let revision = body[if v["ref"]["kind"] == "workflow-accepted-agreement" {
                "version"
            } else {
                "new_revision"
            }]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
            let actual = semantic_document(
                v["ref"]["kind"].as_str().unwrap(),
                &key,
                body["customer"].as_str().unwrap(),
                body["agreement_id"].as_str().unwrap(),
                revision,
                v["body"].clone(),
            )
            .unwrap();
            assert_eq!(json!(actual.reference), v["ref"], "{}", v["name"]);
            assert_eq!(
                actual.bytes,
                v["canonical_utf8"].as_str().unwrap().as_bytes()
            );
        }
        // A body mutation cannot overwrite this permanent semantic identity.
        let v = &vectors[0];
        let b = &v["body"];
        let key = OperationKey::parse(&records::bytes(&b["operation_key"]).unwrap()).unwrap();
        let mut changed = b.clone();
        changed["accepted_at"] = json!("2026-10-13T00:00:00.000000Z");
        let d = semantic_document(
            "workflow-accepted-agreement",
            &key,
            b["customer"].as_str().unwrap(),
            b["agreement_id"].as_str().unwrap(),
            1,
            changed,
        )
        .unwrap();
        assert_eq!(d.reference.id, v["ref"]["id"]);
        assert_ne!(d.reference.digest, v["ref"]["digest"]);
    }
}
