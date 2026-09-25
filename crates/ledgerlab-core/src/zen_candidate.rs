//! Unreviewed admission/1-candidate.1 semantics. Never interpreted as frozen billing records.
use crate::{
    canonical::{self, CanonicalBytes},
    Error, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const PROFILE: &str = "admission/1-candidate.1";
pub const RULE: &str = "five-products-exact/v1";
pub const PRODUCTS: [&str; 5] = ["Amber", "Birch", "Cedar", "Delta", "Elm"];
pub const INPUT: &str =
    "Products: Elm, Cedar, Amber, Delta, Birch. Return their names as a sorted JSON array.";

fn need(ok: bool, code: &'static str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::new(code, "candidate pilot refused"))
    }
}
pub fn bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(CanonicalBytes::from_value(value)?.into_vec())
}
pub fn digest(domain: &str, value: &impl Serialize) -> Result<String> {
    let mut h = Sha256::new();
    h.update(format!("ledgerlab/{domain}/{PROFILE}\0").as_bytes());
    h.update(bytes(value)?);
    Ok(format!("sha256:{}", canonical::hex(&h.finalize())))
}
fn no_null(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Array(a) => a.iter().all(no_null),
        Value::Object(o) => o.values().all(no_null),
        _ => true,
    }
}
pub fn parse<T: serde::de::DeserializeOwned>(raw: &[u8]) -> Result<T> {
    let v = canonical::parse_bounded(raw, 16 * 1024)?;
    need(no_null(&v), "NULL")?;
    serde_json::from_value(v).map_err(|_| Error::new("SHAPE", "closed candidate input required"))
}
fn label(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 96
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._/-:".contains(&c))
}
fn number(s: &str) -> Result<u64> {
    need(
        !s.is_empty() && (s == "0" || !s.starts_with('0')) && s.bytes().all(|c| c.is_ascii_digit()),
        "NUMBER",
    )?;
    s.parse()
        .map_err(|_| Error::new("NUMBER", "unsigned integer overflow"))
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Authorization {
    pub authorization_id: String,
    pub order_id: String,
    pub deliverable_version: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Setup {
    pub schema: String,
    pub synthetic: bool,
    pub customer: String,
    pub agreement: String,
    pub terms_version: String,
    pub admission_atoms: String,
    pub outcome_atoms: String,
    pub authorized_at_us: String,
    pub admit_before_us: String,
    pub outcome_before_us: String,
    pub acceptance_rule: String,
    pub authorizations: Vec<Authorization>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub order_id: String,
    pub authorization_id: String,
    pub customer: String,
    pub agreement: String,
    pub terms_hash: String,
    pub admission_atoms: String,
    pub outcome_atoms: String,
    pub deliverable_version: String,
    pub target: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub delivery_id: String,
    pub attempt_id: String,
    pub session_id: String,
    pub model: String,
    pub outcome_id: String,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub schema: String,
    pub operation: String,
    pub binding: Binding,
    pub evidence: Evidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Vec<String>>,
}
struct Order {
    binding: Binding,
    phase: &'static str,
    receipts: BTreeMap<String, Value>,
}
pub struct State {
    setup: Setup,
    orders: BTreeMap<String, Order>,
    slot: Option<String>,
    clock_floor: u64,
}
pub struct Decision {
    pub response: Value,
    pub changed: bool,
}
impl State {
    pub fn new(setup: Setup) -> Result<Self> {
        need(
            setup.schema == PROFILE
                && setup.synthetic
                && setup.customer == "synthetic-customer"
                && label(&setup.agreement)
                && label(&setup.terms_version)
                && setup.acceptance_rule == RULE,
            "SETUP",
        )?;
        need(
            number(&setup.admission_atoms)? > 0
                && number(&setup.admission_atoms)? <= 10000
                && number(&setup.outcome_atoms)? > 0
                && number(&setup.outcome_atoms)? <= 10000,
            "PRICE",
        )?;
        let start = number(&setup.authorized_at_us)?;
        need(
            start < number(&setup.admit_before_us)?
                && number(&setup.admit_before_us)? < number(&setup.outcome_before_us)?
                && number(&setup.outcome_before_us)? <= i64::MAX as u64,
            "WINDOW",
        )?;
        need(
            !setup.authorizations.is_empty() && setup.authorizations.len() <= 8,
            "AUTHORIZATIONS",
        )?;
        let terms_hash = digest("terms", &setup)?;
        let mut orders = BTreeMap::new();
        let mut authorities = BTreeSet::new();
        for a in &setup.authorizations {
            need(
                label(&a.order_id)
                    && label(&a.authorization_id)
                    && a.deliverable_version == RULE
                    && authorities.insert(a.authorization_id.clone()),
                "AUTHORIZATION",
            )?;
            let target = digest(
                "target",
                &json!([terms_hash, a.authorization_id, a.order_id]),
            )?;
            let binding = Binding {
                order_id: a.order_id.clone(),
                authorization_id: a.authorization_id.clone(),
                customer: setup.customer.clone(),
                agreement: setup.agreement.clone(),
                terms_hash: terms_hash.clone(),
                admission_atoms: setup.admission_atoms.clone(),
                outcome_atoms: setup.outcome_atoms.clone(),
                deliverable_version: a.deliverable_version.clone(),
                target,
            };
            need(
                orders
                    .insert(
                        a.order_id.clone(),
                        Order {
                            binding,
                            phase: "authorized",
                            receipts: BTreeMap::new(),
                        },
                    )
                    .is_none(),
                "AUTHORIZATION",
            )?;
        }
        Ok(Self {
            setup,
            orders,
            slot: None,
            clock_floor: start,
        })
    }
    pub fn apply(&mut self, cmd: &Command, at: u64) -> Result<Decision> {
        need(cmd.schema == PROFILE, "PROFILE")?;
        need(
            matches!(
                cmd.operation.as_str(),
                "reserve" | "admit" | "outcome" | "fail"
            ),
            "OPERATION",
        )?;
        for s in [
            &cmd.evidence.delivery_id,
            &cmd.evidence.attempt_id,
            &cmd.evidence.session_id,
            &cmd.evidence.model,
            &cmd.evidence.outcome_id,
        ] {
            need(label(s), "EVIDENCE")?;
        }
        let order = self
            .orders
            .get_mut(&cmd.binding.order_id)
            .ok_or_else(|| Error::new("UNAUTHORIZED_ORDER", "no accepted authorization"))?;
        need(order.binding == cmd.binding, "BINDING_CONFLICT")?;
        if cmd.operation == "outcome" {
            need(
                cmd.artifact
                    .as_ref()
                    .is_some_and(|a| a.iter().map(String::as_str).eq(PRODUCTS)),
                "ARTIFACT",
            )?;
        } else {
            need(cmd.artifact.is_none(), "ARTIFACT")?;
        }
        // Economic identity precedes deadlines and phase changes. Subordinate IDs are inert.
        if let Some(receipt) = order.receipts.get(&cmd.operation) {
            return Ok(Decision {
                response: json!({"candidate":true,"status":"duplicate","receipt":receipt}),
                changed: false,
            });
        }
        need(at >= self.clock_floor && at <= i64::MAX as u64, "CLOCK")?;
        let atoms = match cmd.operation.as_str() {
            "reserve" => {
                need(
                    order.phase == "authorized" && self.slot.is_none(),
                    "SLOT_UNAVAILABLE",
                )?;
                need(at < number(&self.setup.admit_before_us)?, "WINDOW")?;
                0
            }
            "admit" => {
                need(
                    order.phase == "reserved" && self.slot.as_ref() == Some(&cmd.binding.order_id),
                    "NOT_RESERVED",
                )?;
                need(at < number(&self.setup.admit_before_us)?, "WINDOW")?;
                number(&self.setup.admission_atoms)?
            }
            "outcome" => {
                need(order.phase == "admitted", "NOT_ADMITTED")?;
                need(at < number(&self.setup.outcome_before_us)?, "WINDOW")?;
                number(&self.setup.outcome_atoms)?
            }
            "fail" => {
                need(matches!(order.phase, "reserved" | "admitted"), "TERMINAL")?;
                0
            }
            _ => unreachable!(),
        };
        let mut body = json!({"schema":PROFILE,"binding":order.binding,"kind":cmd.operation,
            "currency":"USD","scale":2,"atoms":atoms.to_string(),"accepted_at_us":at.to_string()});
        if cmd.operation == "admit" {
            body["reservation_receipt"] = order.receipts["reserve"]["id"].clone();
        }
        if cmd.operation == "outcome" {
            body["admission_receipt"] = order.receipts["admit"]["id"].clone();
            body["artifact_hash"] = json!(digest("artifact", &cmd.artifact)?);
            body["acceptance_rule"] = json!(RULE);
        }
        let receipt = json!({"id":digest("receipt", &body)?,"body":body});
        order.phase = match cmd.operation.as_str() {
            "reserve" => "reserved",
            "admit" => "admitted",
            "outcome" => "completed",
            _ => "failed",
        };
        self.slot = if matches!(order.phase, "reserved" | "admitted") {
            Some(cmd.binding.order_id.clone())
        } else {
            None
        };
        self.clock_floor = at;
        order
            .receipts
            .insert(cmd.operation.clone(), receipt.clone());
        Ok(Decision {
            response: json!({"candidate":true,"status":"accepted","receipt":receipt}),
            changed: true,
        })
    }
    pub fn statement(&self) -> Result<Value> {
        let mut net = 0u64;
        let mut orders = Vec::new();
        for order in self.orders.values() {
            let mut subtotal = 0u64;
            for receipt in order.receipts.values() {
                subtotal += number(
                    receipt["body"]["atoms"]
                        .as_str()
                        .ok_or_else(|| Error::new("INTEGRITY", "atoms"))?,
                )?;
            }
            net += subtotal;
            orders.push(json!({"binding":order.binding,"phase":order.phase,"net_atoms":subtotal.to_string(),"receipts":order.receipts}));
        }
        Ok(
            json!({"schema":PROFILE,"candidate":true,"status":"ok","complete":true,"payment_collected":false,
            "provider_cost_accounted":false,"currency":"USD","scale":2,"net_atoms":net.to_string(),
            "slot_owner":self.slot,"orders":orders,"deliverable_input":INPUT}),
        )
    }
}
