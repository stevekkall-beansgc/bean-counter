//! Shared deterministic statement-line composition for retained adjustments.
use super::*;
use crate::store::errors::StoreError;
use crate::store::sqlite::m5::{
    self, Child, Command, PeriodCloseM3Assignment, PresentableAdjustment,
};
use ledgerlab_core::canonical::CanonicalBytes;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const AD_HOC: &str = "ledger-billing-ad-hoc-statement/1";
const AD_HOC_RECORD: &str = "ledger-billing-ad-hoc-statement-record/1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdjustmentRef {
    source: String,
    adjustment_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AdHocRequest {
    schema: String,
    customer: String,
    command_id: String,
    adjustments: Vec<AdjustmentRef>,
}

fn checked_id(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn encode(value: &Value) -> Result<Vec<u8>, ServiceError> {
    let bytes = CanonicalBytes::from_value(value)
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?
        .into_vec();
    if bytes.len() > 262_144 {
        return Err(service::reject("BILLING_M5_BOUNDS"));
    }
    Ok(bytes)
}

fn presentation_error(error: StoreError) -> ServiceError {
    match error {
        StoreError::BillingHistoryLimit => service::reject("BILLING_M5_BOUNDS"),
        StoreError::BillingUpgradeRequired => service::reject("BILLING_M5_SCHEMA_REQUIRED"),
        StoreError::BillingPeriod => service::reject("BILLING_M5_PERIOD"),
        StoreError::Integrity(_) | StoreError::InvalidStore(_) => {
            service::reject("BILLING_M5_INTEGRITY")
        }
        other => store_error(other),
    }
}

impl BillingLedger {
    pub async fn adjustment_statement(&self, raw: &[u8]) -> local::Result<Value> {
        self.adjustment_statement_inner(raw, None).await
    }

    #[cfg(test)]
    pub(super) async fn adjustment_statement_at(
        &self,
        raw: &[u8],
        accepted: ledgerlab_core::domain::Timestamp,
    ) -> local::Result<Value> {
        self.adjustment_statement_inner(raw, Some(accepted)).await
    }

    async fn adjustment_statement_inner(
        &self,
        raw: &[u8],
        accepted_override: Option<ledgerlab_core::domain::Timestamp>,
    ) -> local::Result<Value> {
        let value = ledgerlab_core::canonical::parse_bounded(raw, 262_144)
            .map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        let request: AdHocRequest =
            serde_json::from_value(value).map_err(|_| service::reject("BILLING_M5_REQUEST"))?;
        if request.schema != AD_HOC
            || !checked_id(&request.customer, 128)
            || !checked_id(&request.command_id, 128)
            || request.adjustments.is_empty()
            || request
                .adjustments
                .iter()
                .any(|a| !checked_id(&a.source, 256) || !checked_id(&a.adjustment_id, 128))
            || request.adjustments.windows(2).any(|pair| {
                (&pair[0].source, &pair[0].adjustment_id)
                    >= (&pair[1].source, &pair[1].adjustment_id)
            })
        {
            return Err(service::reject("BILLING_M5_REQUEST").into());
        }
        let identity = encode(&json!({"schema":"ledger-billing-m5-command-identity/1",
            "domain":"customer-admin","family":AD_HOC,"customer":request.customer,
            "key_kind":"command_id","key":request.command_id}))?;
        let mut tx = self
            .store
            .begin(Instant::now() + Self::WRITE_BUDGET)
            .await
            .map_err(store_error)?;
        if let Some(saved) = tx.m5_lookup(&identity).await.map_err(presentation_error)? {
            if saved.request != raw {
                return Err(service::reject("IDENTITY_CONFLICT").into());
            }
            let result = serde_json::from_slice(&saved.response)
                .map_err(|_| ServiceError::IntegrityFailure)?;
            tx.rollback().await.map_err(store_error)?;
            return Ok(result);
        }
        let snapshot = tx.billing_snapshot().await.map_err(store_error)?;
        service::validate_snapshot(&snapshot)?;
        let enforce_clock = accepted_override.is_none();
        let accepted = match accepted_override {
            Some(at) => at,
            None => local::now()?,
        };
        let references = request
            .adjustments
            .iter()
            .map(|a| (a.source.clone(), a.adjustment_id.clone()))
            .collect::<Vec<_>>();
        let state = tx
            .m5_ad_hoc_state(&request.customer, &references)
            .await
            .map_err(presentation_error)?;
        let mut selected = Vec::with_capacity(request.adjustments.len());
        for reference in &request.adjustments {
            if let Some(found) = state.adjustments.iter().find(|a| {
                a.source == reference.source && a.adjustment_id == reference.adjustment_id
            }) {
                selected.push(found.clone());
            } else {
                let code = match tx
                    .m5_adjustment_claimed(
                        &request.customer,
                        &reference.source,
                        &reference.adjustment_id,
                    )
                    .await
                    .map_err(presentation_error)?
                {
                    Some(true) => "BILLING_M5_PRESENTED",
                    _ => "BILLING_M5_SCOPE",
                };
                return Err(service::reject(code).into());
            }
        }
        let view_identity = json!({"customer":request.customer,"statement_id":request.command_id});
        let mut lines = Vec::with_capacity(selected.len());
        let mut net = 0i128;
        for adjustment in &selected {
            let (line, amount) =
                adjustment_line(&request.customer, "ad_hoc", &view_identity, adjustment)
                    .map_err(|_| service::reject("BILLING_M5_INTEGRITY"))?;
            net = net
                .checked_add(amount)
                .ok_or_else(|| service::reject("BILLING_M5_BOUNDS"))?;
            lines.push(line);
        }
        lines.sort_by(|left, right| left["line_id"].as_str().cmp(&right["line_id"].as_str()));
        let refs = request
            .adjustments
            .iter()
            .map(|a| json!({"source":a.source,"adjustment_id":a.adjustment_id}))
            .collect::<Vec<_>>();
        let mut result = json!({"schema":"ledger-billing-ad-hoc-statement-result/1","status":"issued",
            "customer":request.customer,"statement_id":request.command_id,"adjustments":refs,
            "lines":lines,"currency":"USD","scale":18,"net_atoms":net.to_string(),
            "direction":if net == 0 { "none" } else if net < 0 { "payable" } else { "receivable" },
            "complete":true});
        let unsigned = encode(&result)?;
        let mut digest = Sha256::new();
        digest.update(b"bean-counter/m5/ad-hoc-statement/1\0");
        digest.update(&unsigned);
        let statement_hash = m5::hex(&digest.finalize());
        result["statement_hash"] = json!(statement_hash);
        let response = encode(&result)?;
        let claim_refs = selected
            .iter()
            .map(|a| (a.source.as_str(), a.adjustment_id.as_str()))
            .collect::<Vec<_>>();
        let claims = claim_children(
            &identity,
            &request.customer,
            &claim_refs,
            "ad-hoc",
            &request.command_id,
            accepted.as_str(),
            state.command_sequence,
            state.first_record_sequence,
        )
        .map_err(|_| service::reject("BILLING_M5_BOUNDS"))?;
        let record_key = encode(
            &json!({"role":"ad-hoc-statement","customer":request.customer,
            "kind":AD_HOC_RECORD,"key":{"statement_id":request.command_id}}),
        )?;
        let record_id = m5::record_id(&identity, &record_key);
        let mut record = json!({"schema":AD_HOC_RECORD,"customer":request.customer,
            "statement_id":request.command_id,
            "adjustments":selected.iter().map(|a| json!({"customer":request.customer,"source":a.source,"adjustment_id":a.adjustment_id})).collect::<Vec<_>>(),
            "net_atoms":net.to_string(),"statement_hash":statement_hash,
            "record":{"record_id":record_id,
                "sequence":(state.first_record_sequence + claims.len() as i64).to_string(),
                "accepted_at":accepted.as_str(),"command_sequence":state.command_sequence.to_string()}});
        let payload = m5::seal_child(AD_HOC_RECORD, &mut record).map_err(presentation_error)?;
        let mut children: Vec<Child<'_>> = claims
            .iter()
            .map(|claim| Child {
                family: "ledger-billing-presentation-claim/1",
                customer: Some(&request.customer),
                source: Some(&claim.source),
                child_key: &claim.key,
                payload: &claim.payload,
            })
            .collect();
        children.push(Child {
            family: AD_HOC_RECORD,
            customer: Some(&request.customer),
            source: None,
            child_key: &record_key,
            payload: &payload,
        });
        children.sort_by(|left, right| left.child_key.cmp(right.child_key));
        let command = Command {
            family: AD_HOC,
            domain: "customer-admin",
            customer: Some(&request.customer),
            source: None,
            identity_key: &identity,
            accepted_at_us: accepted.micros(),
            enforce_clock,
            request: raw,
            response: &response,
            children: &children,
        };
        tx.m5_append_ad_hoc(
            &command,
            &m5::AdHocProjection {
                customer: &request.customer,
                statement_id: &request.command_id,
                adjustments: &selected,
                statement_hash: &statement_hash,
                statement_bytes: &response,
            },
        )
        .await
        .map_err(presentation_error)?;
        match tx.commit().await {
            Ok(()) => Ok(result),
            Err(CommitError::RolledBack(error)) => Err(store_error(error).into()),
            Err(CommitError::OutcomeUnknown) => {
                Err(service::reject("BILLING_M5_OUTCOME_UNKNOWN").into())
            }
        }
    }
}

pub(crate) struct ClaimChild {
    pub key: Vec<u8>,
    pub payload: Vec<u8>,
    pub source: String,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn claim_children(
    identity: &[u8],
    customer: &str,
    adjustments: &[(&str, &str)],
    kind: &str,
    statement_id: &str,
    accepted_at: &str,
    command_sequence: i64,
    first_record_sequence: i64,
) -> Result<Vec<ClaimChild>, &'static str> {
    let mut keyed = adjustments
        .iter()
        .map(|(source, adjustment_id)| {
            let key = CanonicalBytes::from_value(&json!({"role":"presentation-claim",
                "customer":customer,"source":source,
                "kind":"ledger-billing-presentation-claim/1",
                "key":{"adjustment_id":adjustment_id,"presentation_kind":kind}}))
            .map_err(|_| "M5 claim key")?
            .into_vec();
            Ok((key, (*source, *adjustment_id)))
        })
        .collect::<Result<Vec<_>, &'static str>>()?;
    keyed.sort_by(|left, right| left.0.cmp(&right.0));
    let mut children = Vec::with_capacity(keyed.len());
    for (index, (key, adjustment)) in keyed.into_iter().enumerate() {
        let record_id = m5::record_id(identity, &key);
        let mut payload = json!({
            "schema":"ledger-billing-presentation-claim/1",
            "customer":customer,"source":adjustment.0,
            "adjustment_id":adjustment.1,
            "presentation_kind":kind,"statement_id":statement_id,
            "record":{"record_id":record_id,
                "sequence":(first_record_sequence + index as i64).to_string(),
                "accepted_at":accepted_at,"command_sequence":command_sequence.to_string()}
        });
        let payload = m5::seal_child("ledger-billing-presentation-claim/1", &mut payload)
            .map_err(|_| "M5 claim payload")?;
        children.push(ClaimChild {
            key,
            payload,
            source: adjustment.0.to_string(),
        });
    }
    Ok(children)
}

pub(crate) fn adjustment_line(
    customer: &str,
    view_kind: &str,
    view_identity: &Value,
    adjustment: &PresentableAdjustment,
) -> Result<(Value, i128), &'static str> {
    if adjustment.source_stream == "m5" {
        if !matches!(
            adjustment.cause_kind.as_str(),
            "per-work-quantity-correction" | "cumulative-quantity-correction"
        ) {
            return Err("M5 unsupported adjustment source");
        }
        let payload = ledgerlab_core::canonical::parse_bounded(&adjustment.bundle, 262_144)
            .map_err(|_| "M5 adjustment payload")?;
        let request = ledgerlab_core::canonical::parse_bounded(&adjustment.ingress, 262_144)
            .map_err(|_| "M5 adjustment request")?;
        let amount = payload["signed_delta_atoms"]
            .as_str()
            .ok_or("M5 adjustment amount")?
            .parse::<i128>()
            .map_err(|_| "M5 adjustment amount")?;
        let calculation = &payload["calculation"];
        if payload["schema"] != "ledger-billing-post-close-adjustment/1"
            || payload["record"]["record_id"] != adjustment.source_record_id
            || payload["adjustment_id"] != adjustment.adjustment_id
            || payload["cause_kind"] != adjustment.cause_kind
            || payload["target_id"] != adjustment.target_id
            || payload["signed_delta_atoms"] != adjustment.signed_delta_atoms
            || payload["original_period_id"]["term_version"]
                .as_str()
                .and_then(|v| v.parse::<i64>().ok())
                != Some(adjustment.original_term_version)
            || payload["original_period_id"]["period_index"]
                .as_str()
                .and_then(|v| v.parse::<i64>().ok())
                != Some(adjustment.original_period_index)
            || payload["assigned_period_id"]["term_version"]
                .as_str()
                .and_then(|v| v.parse::<i64>().ok())
                != Some(adjustment.assigned_term_version)
            || payload["assigned_period_id"]["period_index"]
                .as_str()
                .and_then(|v| v.parse::<i64>().ok())
                != Some(adjustment.assigned_period_index)
            || request["schema"] != "ledger-billing-quantity-correction/1"
            || request["id"] != payload["cause_id"]
            || request["target"] != adjustment.target_id
            || calculation["kind"] != "post_close_adjustment"
            || calculation["booked_atoms"] != adjustment.signed_delta_atoms
            || calculation["exact_atoms_numerator"] != adjustment.signed_delta_atoms
            || calculation["exact_atoms_denominator"] != "1"
            || calculation["rounding"] != "none"
        {
            return Err("M5 adjustment projection");
        }
        let source_record = json!({"customer":customer,"source":adjustment.source,
            "kind":adjustment.source_record_kind,"id":adjustment.source_record_id});
        let mut line = json!({"source_records":[source_record],"basis":"post_close_adjustment",
            "agreement_id":adjustment.agreement_id,"agreement_version":adjustment.agreement_version.to_string(),
            "payer":customer,"recipient":adjustment.recipient.as_deref().ok_or("M5 adjustment recipient")?,
            "currency":"USD","scale":18,"amount_atoms":amount.to_string(),"calculation":calculation});
        let identity = CanonicalBytes::from_value(
            &json!({"view_kind":view_kind,"view_identity":view_identity,"line":line}),
        )
        .map_err(|_| "M5 adjustment line")?;
        line["line_id"] = json!(m5::hex(&m5::hash(
            b"bean-counter/m5/statement-line/1\0",
            identity.as_slice()
        )));
        return Ok((line, amount));
    }
    if adjustment.source_stream != "m3" || adjustment.cause_kind != "outcome-correction" {
        return Err("M5 unsupported adjustment source");
    }
    let bundle = ledgerlab_core::canonical::parse_bounded(&adjustment.bundle, 8 * 1024 * 1024)
        .map_err(|_| "M5 adjustment bundle")?;
    let rows = bundle.as_array().ok_or("M5 adjustment bundle")?;
    let ingress = ledgerlab_core::canonical::parse_bounded(&adjustment.ingress, 262_144)
        .map_err(|_| "M5 adjustment ingress")?;
    if ingress["id"] != adjustment.adjustment_id
        || ingress["target"] != adjustment.target_id
        || ![
            "ledger-billing-correction/1",
            "ledger-billing-correction/2",
            "ledger-billing-outcome/1",
            "ledger-billing-outcome/2",
        ]
        .contains(&ingress["schema"].as_str().unwrap_or_default())
        || !rows.iter().any(|row| {
            row["kind"] == adjustment.source_record_kind && row["id"] == adjustment.source_record_id
        })
    {
        return Err("M5 adjustment identity");
    }
    if (ingress["schema"] == "ledger-billing-correction/1"
        || ingress["schema"] == "ledger-billing-correction/2")
        && adjustment.source_sequence > 0
        && (adjustment.original_term_version < 1
            || adjustment.original_period_index < 0
            || adjustment.assigned_term_version < 1
            || adjustment.assigned_period_index < 0)
    {
        return Err("M5 adjustment period link");
    }
    let actions: Vec<&Value> = rows.iter().filter(|row| row["kind"] == "action").collect();
    let first = actions.first().ok_or("M5 adjustment actions")?;
    let payer = first["body"]["roles"]["payer"]
        .as_str()
        .ok_or("M5 adjustment roles")?;
    let recipient = first["body"]["roles"]["recipient"]
        .as_str()
        .ok_or("M5 adjustment roles")?;
    let scale = first["body"]["amount"]["scale"]
        .as_u64()
        .ok_or("M5 adjustment scale")?;
    let multiplier = match scale {
        2 => 10_000_000_000_000_000i128,
        18 => 1,
        _ => return Err("M5 adjustment scale"),
    };
    let mut raw_delta = 0i128;
    let mut prior = 0i128;
    let mut accepted = 0i128;
    for action in actions {
        if action["body"]["roles"]["payer"] != payer
            || action["body"]["roles"]["recipient"] != recipient
            || action["body"]["amount"]["scale"] != scale
            || action["body"]["amount"]["currency"] != "USD"
            || action["body"]["agreement_id"] != adjustment.agreement_id
        {
            return Err("M5 adjustment action");
        }
        let atoms: i128 = action["body"]["amount"]["atoms"]
            .as_str()
            .ok_or("M5 adjustment atoms")?
            .parse()
            .map_err(|_| "M5 adjustment atoms")?;
        raw_delta = raw_delta.checked_add(atoms).ok_or("M5 adjustment amount")?;
        let scaled = atoms
            .checked_mul(multiplier)
            .ok_or("M5 adjustment amount")?;
        match action["body"]["slot"].as_str() {
            Some("inverse") => prior = prior.checked_sub(scaled).ok_or("M5 adjustment amount")?,
            Some("replacement") => {
                accepted = accepted.checked_add(scaled).ok_or("M5 adjustment amount")?
            }
            _ => return Err("M5 adjustment slot"),
        }
    }
    if raw_delta.to_string() != adjustment.signed_delta_atoms {
        return Err("M5 adjustment projection amount");
    }
    let amount = raw_delta
        .checked_mul(multiplier)
        .ok_or("M5 adjustment amount")?;
    if accepted.checked_sub(prior) != Some(amount) {
        return Err("M5 adjustment arithmetic");
    }
    let change_kind = if ingress["schema"] == "ledger-billing-outcome/1"
        || ingress["schema"] == "ledger-billing-outcome/2"
    {
        if prior != 0 {
            return Err("M5 first outcome amount");
        }
        "first"
    } else if ingress["replacement"]["kind"] == "reverse" {
        "reversal"
    } else {
        "replacement"
    };
    if change_kind == "reversal" && accepted != 0 {
        return Err("M5 reversal amount");
    }
    let mut operands = json!({"change_kind":change_kind,"target_id":adjustment.target_id,
        "outcome_id":adjustment.adjustment_id,"accepted_atoms":accepted.to_string()});
    if change_kind != "first" {
        let prior_outcome_id = adjustment
            .prior_outcome_id
            .as_deref()
            .ok_or("M5 adjustment prior outcome")?;
        operands["prior_outcome_id"] = json!(prior_outcome_id);
        operands["prior_atoms"] = json!(prior.to_string());
    }
    let source_record = json!({"customer":customer,"source":adjustment.source,
        "kind":adjustment.source_record_kind,"id":adjustment.source_record_id});
    let mut line = json!({
        "source_records":[source_record],"basis":"outcome_revision",
        "agreement_id":adjustment.agreement_id,
        "agreement_version":adjustment.agreement_version.to_string(),
        "payer":payer,"recipient":recipient,"currency":"USD","scale":18,
        "amount_atoms":amount.to_string(),
        "calculation":{"kind":"outcome_revision","exact_atoms_numerator":amount.to_string(),
            "exact_atoms_denominator":"1","booked_atoms":amount.to_string(),
            "rounding":"none","operands":operands}
    });
    let identity = CanonicalBytes::from_value(
        &json!({"view_kind":view_kind,"view_identity":view_identity,"line":line}),
    )
    .map_err(|_| "M5 adjustment line")?;
    line["line_id"] = json!(m5::hex(&m5::hash(
        b"bean-counter/m5/statement-line/1\0",
        identity.as_slice()
    )));
    Ok((line, amount))
}

pub(crate) fn outcome_assignment_line(
    customer: &str,
    view_kind: &str,
    view_identity: &Value,
    assignment: &PeriodCloseM3Assignment,
) -> Result<(Value, i128), &'static str> {
    if assignment.kind != "receipt" {
        return Err("M5 outcome assignment kind");
    }
    let ingress = ledgerlab_core::canonical::parse_bounded(&assignment.ingress, 262_144)
        .map_err(|_| "M5 outcome ingress")?;
    let bundle = ledgerlab_core::canonical::parse_bounded(&assignment.bundle, 8 * 1024 * 1024)
        .map_err(|_| "M5 outcome bundle")?;
    let raw_delta = bundle
        .as_array()
        .ok_or("M5 outcome bundle")?
        .iter()
        .filter(|row| row["kind"] == "action")
        .try_fold(0i128, |sum, row| {
            let atoms: i128 = row["body"]["amount"]["atoms"]
                .as_str()
                .ok_or("M5 outcome atoms")?
                .parse()
                .map_err(|_| "M5 outcome atoms")?;
            sum.checked_add(atoms).ok_or("M5 outcome atoms")
        })?;
    let outcome = PresentableAdjustment {
        source: assignment.source.clone(),
        adjustment_id: ingress["id"].as_str().ok_or("M5 outcome id")?.to_owned(),
        cause_kind: "outcome-correction".into(),
        target_id: ingress["target"]
            .as_str()
            .ok_or("M5 outcome target")?
            .to_owned(),
        original_term_version: 0,
        original_period_index: 0,
        assigned_term_version: 0,
        assigned_period_index: 0,
        source_stream: "m3".into(),
        source_sequence: 0,
        signed_delta_atoms: raw_delta.to_string(),
        source_record_id: assignment.id.clone(),
        source_record_kind: assignment.kind.clone(),
        bundle: assignment.bundle.clone(),
        ingress: assignment.ingress.clone(),
        agreement_id: assignment
            .agreement_id
            .clone()
            .ok_or("M5 outcome agreement")?,
        agreement_version: assignment.agreement_version.ok_or("M5 outcome agreement")?,
        prior_outcome_id: assignment.prior_outcome_id.clone(),
        recipient: None,
    };
    adjustment_line(customer, view_kind, view_identity, &outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy_assignment(schema: &str, id: &str, actions: Vec<Value>) -> PeriodCloseM3Assignment {
        let mut rows = vec![json!({"kind":"receipt","id":format!("receipt-{id}")})];
        rows.extend(actions);
        PeriodCloseM3Assignment {
            source: "urn:example:legacy".into(),
            kind: "receipt".into(),
            id: format!("receipt-{id}"),
            bundle: CanonicalBytes::from_value(&json!(rows)).unwrap().into_vec(),
            ingress: CanonicalBytes::from_value(&json!({
                "schema":schema,"id":id,"target":"legacy-target","family":"quality",
                "occurred_at":"2026-01-01T00:00:00.000000Z","evidence":"retained",
                "replacement":{"kind":"code","code":"legacy"}
            }))
            .unwrap()
            .into_vec(),
            agreement_id: Some("legacy-agreement".into()),
            agreement_version: Some(1),
            prior_outcome_id: (schema == "ledger-billing-correction/1")
                .then(|| "legacy-outcome".into()),
        }
    }

    fn legacy_action(slot: &str, atoms: &str) -> Value {
        json!({"kind":"action","body":{"agreement_id":"legacy-agreement",
            "roles":{"payer":"legacy-customer","recipient":"example-company"},
            "amount":{"currency":"USD","scale":2,"atoms":atoms},"slot":slot}})
    }

    #[test]
    fn standard_presentation_claim_matches_frozen_command_golden() {
        let identity = br#"{"customer":"customer-usage-1","domain":"customer-admin","family":"ledger-billing-period-close/1","key":"1:1","key_kind":"logical_period_id","schema":"ledger-billing-m5-command-identity/1"}"#;
        let children = claim_children(
            identity,
            "customer-usage-1",
            &[("urn:example:usage-work", "adj-open-1")],
            "standard-period",
            "7bd206cafcc48bbef94a105c1a489ba198713a99e4890668e14fcb0abfe85e9d",
            "2026-03-05T12:00:00.000000Z",
            9,
            33,
        )
        .unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(
            std::str::from_utf8(&children[0].key).unwrap(),
            r#"{"customer":"customer-usage-1","key":{"adjustment_id":"adj-open-1","presentation_kind":"standard-period"},"kind":"ledger-billing-presentation-claim/1","role":"presentation-claim","source":"urn:example:usage-work"}"#
        );
        let payload = ledgerlab_core::canonical::parse(&children[0].payload).unwrap();
        assert_eq!(
            payload["record"]["record_id"],
            "m5r_e4a420b8e1340ad5927fa8307f7bf75197c7642f1a2eec65560034a765cb5f0e"
        );
        assert_eq!(
            payload["record"]["payload_hash"],
            "e5b37b9a41ed1158d7da84a0975373f3ab963bc1b05d818fe07789cb2028ba02"
        );
    }

    #[test]
    fn ad_hoc_presentation_claim_matches_frozen_command_golden() {
        let identity = br#"{"customer":"customer-usage-1","domain":"customer-admin","family":"ledger-billing-ad-hoc-statement/1","key":"adhoc-1","key_kind":"command_id","schema":"ledger-billing-m5-command-identity/1"}"#;
        let children = claim_children(
            identity,
            "customer-usage-1",
            &[("urn:example:usage-work", "adj-open-1")],
            "ad-hoc",
            "adhoc-1",
            "2026-03-02T10:00:00.000000Z",
            8,
            31,
        )
        .unwrap();
        let payload = ledgerlab_core::canonical::parse(&children[0].payload).unwrap();
        assert_eq!(
            payload["record"]["record_id"],
            "m5r_9a71668e08b50ac38d437ae05deb98ec466bcde202b2837c2c8115f8dbbfab04"
        );
        assert_eq!(
            payload["record"]["payload_hash"],
            "312ab446195e0486ed4ee68a554a113709e4aa5a2ff1837c90bf9117c5e1fd74"
        );
    }

    #[test]
    fn migrated_v1_outcomes_and_corrections_render_statement_lines() {
        let view = json!({"customer":"legacy-customer","period_id":{
            "term_version":"1","period_index":"0"}});
        let first = legacy_assignment(
            "ledger-billing-outcome/1",
            "legacy-outcome",
            vec![legacy_action("replacement", "10")],
        );
        let (first_line, first_amount) =
            outcome_assignment_line("legacy-customer", "standard", &view, &first).unwrap();
        assert_eq!(first_amount, 100_000_000_000_000_000);
        assert_eq!(
            first_line["calculation"]["operands"]["change_kind"],
            "first"
        );
        let correction = legacy_assignment(
            "ledger-billing-correction/1",
            "legacy-correction",
            vec![
                legacy_action("inverse", "-10"),
                legacy_action("replacement", "7"),
            ],
        );
        let (correction_line, correction_amount) =
            outcome_assignment_line("legacy-customer", "standard", &view, &correction).unwrap();
        assert_eq!(correction_amount, -30_000_000_000_000_000);
        assert_eq!(
            correction_line["calculation"]["operands"]["change_kind"],
            "replacement"
        );
        assert_eq!(
            correction_line["calculation"]["operands"]["prior_outcome_id"],
            "legacy-outcome"
        );
    }
}
