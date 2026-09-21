use crate::store::records::*;
use ledgerlab_core::{
    canonical::CanonicalBytes,
    domain::{DecisionPlan, Event},
};
use serde_json::Value;
fn bytes(v: &Value) -> Vec<u8> {
    CanonicalBytes::from_value(v)
        .expect("validated core record")
        .into_vec()
}
fn text(v: &Value, k: &str) -> String {
    v[k].as_str().expect("validated core text").into()
}
fn number(v: &Value, k: &str) -> i64 {
    v[k].as_i64().unwrap_or_else(|| {
        v[k].as_str()
            .expect("core counter")
            .parse()
            .expect("bounded core counter")
    })
}
fn row(
    v: &Value,
    event: &Event,
    decision_id: &str,
    facts_hash: &str,
    received_us: i64,
) -> JournalRecord {
    let b = &v["body"];
    let row = match v["kind"].as_str().unwrap() {
        "document" => JournalRow::Document {
            id: text(v, "id"),
            kind: text(v, "document_type"),
        },
        "party" => JournalRow::Party {
            id: text(v, "id"),
            role_metadata_doc: text(b, "role_metadata_doc"),
        },
        "source-grant-record" => JournalRow::SourceGrant {
            id: text(v, "id"),
            principal_id: text(b, "principal_id"),
            source: text(b, "source"),
            grant_doc: text(b, "grant_doc"),
        },
        "binding-record" => JournalRow::Binding {
            id: text(v, "id"),
            agreement_id: text(b, "agreement_id"),
            version: number(b, "version"),
            policy_doc: text(b, "policy_doc"),
            assent_doc: text(b, "assent_doc"),
            roles_doc: text(b, "roles_doc"),
            context_doc: text(b, "context_doc"),
            currency: text(b, "currency"),
            scale: number(b, "scale"),
        },
        "event" => JournalRow::Event {
            id: text(v, "id"),
            source: text(b, "source"),
            external_id: text(b, "id"),
            operation_id: text(b, "operation_id"),
            kind: text(b, "type"),
            chain_id: text(b, "chain"),
            decision_id: decision_id.to_owned(),
            ingress_hash: event.candidate().ingress_hash().to_owned(),
            claim_facts_hash: facts_hash.to_owned(),
            ingress_bytes: event.candidate().ingress_bytes().as_slice().to_vec(),
            occurred_us: event.dto().occurred_at.as_ref().map(|t| t.micros()),
            received_us,
        },
        "snapshot-ref" => JournalRow::Snapshot {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            document_id: text(b, "document_id"),
            purpose: text(b, "purpose"),
        },
        "delivery-key" => JournalRow::DeliveryKey {
            source: text(b, "source"),
            external_id: text(b, "external_id"),
            ingress_hash: text(b, "ingress_hash"),
            canonical_event_id: text(b, "canonical_event_id"),
            kind: text(b, "kind"),
            observed_us: received_us,
        },
        "claim" => JournalRow::Claim {
            id: text(v, "id"),
            source: text(b, "source"),
            operation_id: text(b, "operation_id"),
            kind: text(b, "kind"),
            token: text(b, "token"),
            facts_hash: text(b, "facts_hash"),
            event_id: text(b, "event_id"),
        },
        "effect" => JournalRow::Effect {
            id: text(v, "id"),
            agreement_id: text(b, "agreement_id"),
            component: text(b, "component"),
            claim_id: text(b, "claim_id"),
            namespace: text(b, "namespace"),
            facts_hash: text(b, "facts_hash"),
            action_id: text(b, "action_id"),
            match_key_bytes: bytes(&b["match_key"]),
        },
        "action" => JournalRow::Action {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            decision_id: text(b, "decision_id"),
            effect_id: text(b, "effect_id"),
            obligation_id: text(b, "obligation_id"),
            kind: text(b, "kind"),
            book: text(b, "book"),
            component: text(b, "component"),
            binding_id: text(b, "binding_id"),
            snapshot_doc: text(b, "snapshot_doc"),
            roles_doc: text(b, "roles_doc"),
            currency: text(&b["amount"], "currency"),
            scale: number(&b["amount"], "scale"),
            atoms: text(&b["amount"], "atoms"),
            reverses: b.get("reverses").map(|x| x.as_str().unwrap().to_owned()),
            allocation_parent: b
                .get("allocation_parent")
                .map(|x| x.as_str().unwrap().to_owned()),
        },
        "action-source" => JournalRow::ActionSource {
            action_id: text(b, "action_id"),
            event_id: text(b, "event_id"),
        },
        "action-dependency" => JournalRow::ActionDependency {
            action_id: text(b, "action_id"),
            input_action_id: text(b, "input_action_id"),
        },
        "explanation" => JournalRow::Explanation {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            ordinal: number(b, "ordinal"),
            code: text(b, "code"),
            rule_id: b.get("rule_id").map(|x| x.as_str().unwrap().to_owned()),
        },
        "intention" => JournalRow::Intention {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            obligation_id: text(b, "obligation_id"),
            destination_id: text(b, "destination_id"),
            idempotency_key: text(b, "idempotency_key"),
        },
        "control-transition" => JournalRow::ControlTransition {
            id: text(v, "id"),
            control_kind: text(b, "control_kind"),
            control_id: text(b, "control_id"),
            from_revision: number(b, "from_revision"),
            to_revision: number(b, "to_revision"),
            from_event_count: number(b, "from_event_count"),
            to_event_count: number(b, "to_event_count"),
            event_id: text(b, "event_id"),
            document_id: text(b, "document_id"),
        },
        "chain-revision" => JournalRow::ChainRevision {
            chain_id: text(b, "chain_id"),
            revision: number(b, "revision"),
            event_id: text(b, "event_id"),
            decision_id: text(b, "decision_id"),
        },
        "decision-manifest" => JournalRow::Manifest {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            chain_id: text(b, "chain_id"),
            revision: number(b, "revision"),
            decision_hash: text(v, "content_hash"),
        },
        "receipt" => JournalRow::Receipt {
            id: text(v, "id"),
            event_id: text(b, "event_id"),
            decision_id: text(b, "decision_id"),
        },
        k => panic!("unmapped fixture {k}"),
    };
    JournalRecord {
        scope: Scope {
            tenant: event.scope().tenant().into(),
            environment: event.scope().environment().into(),
        },
        canonical: CanonicalRecord {
            canonical_bytes: bytes(b),
            content_hash: text(v, "content_hash"),
        },
        row,
    }
}

pub(super) fn writes(
    plan: &DecisionPlan,
    event: &Event,
    received_us: i64,
) -> Vec<(&'static str, usize, WriteOp)> {
    let records: Vec<Value> = plan
        .records()
        .iter()
        .map(|r| {
            serde_json::from_slice(r.bytes().expect("core envelope").as_slice()).expect("core JSON")
        })
        .collect();
    let manifest = records
        .iter()
        .find(|v| v["kind"] == "decision-manifest")
        .expect("complete plan");
    let claim = records
        .iter()
        .find(|v| v["kind"] == "claim")
        .expect("completion plan");
    let transition = &records
        .iter()
        .find(|v| v["kind"] == "control-transition")
        .expect("chain transition")["body"];
    let scope = Scope {
        tenant: event.scope().tenant().into(),
        environment: event.scope().environment().into(),
    };
    let mut ops = Vec::new();
    for (kind, name) in [
        ("document", "snapshot_document"),
        ("snapshot-ref", "snapshot_refs"),
        ("event", "event"),
        ("delivery-key", "original_delivery_key"),
        ("claim", "claim"),
        ("effect", "effects"),
        ("action", "actions"),
        ("action-source", "action_sources"),
        ("action-dependency", "action_dependency"),
        ("explanation", "explanations"),
        ("intention", "intention"),
        ("held", "delivery_state"),
        ("control-transition", "control_transition"),
        ("head", "chain_head"),
        ("chain-revision", "chain_revision"),
        ("decision-manifest", "manifest"),
        ("receipt", "receipt"),
    ] {
        match kind {
            "held" => {
                for (i, intention) in plan.intentions().iter().enumerate() {
                    ops.push((
                        name,
                        i,
                        WriteOp::HoldDelivery(HeldDelivery {
                            scope: scope.clone(),
                            intention_id: intention.id().into(),
                            next_attempt_us: received_us,
                        }),
                    ));
                }
            }
            "head" => ops.push((
                name,
                0,
                WriteOp::AdvanceChain(ChainAdvance {
                    scope: scope.clone(),
                    id: event.chain().into(),
                    from_revision: number(transition, "from_revision"),
                    to_revision: number(transition, "to_revision"),
                    from_event_count: number(transition, "from_event_count"),
                    to_event_count: number(transition, "to_event_count"),
                }),
            )),
            _ => {
                for (i, v) in records.iter().filter(|v| v["kind"] == kind).enumerate() {
                    ops.push((
                        name,
                        i,
                        WriteOp::Journal(Box::new(row(
                            v,
                            event,
                            manifest["id"].as_str().expect("decision id"),
                            claim["body"]["facts_hash"].as_str().expect("claim digest"),
                            received_us,
                        ))),
                    ));
                }
            }
        }
    }
    ops
}
