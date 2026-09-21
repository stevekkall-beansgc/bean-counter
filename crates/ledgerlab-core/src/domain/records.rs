use super::{prefixed, slug, text, validate_source, Event, Revision, Scope, Timestamp};
use crate::canonical::{self, CanonicalBytes, Domain};
use crate::money::{validate_currency, Decimal, ExactRatio, Money};
use crate::policy::{CompiledPolicy, Operation, PredicateValue};
use crate::wire::{EventKind, Relation};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Roles {
    provider: String,
    cost_originator: String,
    bearer: String,
    payer: String,
    beneficiary: String,
    recipient: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payer_delegation: Option<String>,
}
impl Roles {
    fn validate(&self) -> Result<()> {
        for s in [
            &self.provider,
            &self.cost_originator,
            &self.bearer,
            &self.payer,
            &self.beneficiary,
            &self.recipient,
        ] {
            text(s, 128)?;
        }
        if let Some(s) = &self.payer_delegation {
            prefixed(s, "doc_")?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) priority: Option<bool>,
    pub(crate) funding: String,
    currency: String,
    scale: u8,
    binding_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Assent {
    schema: String,
    mode: String,
    agreement_id: String,
    terms_version: Revision,
    acceptor: String,
    bearer: String,
    payer: String,
    recipient: String,
    accepted_at: Timestamp,
    evidence_ref: String,
    evidence_digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceGrant {
    schema: String,
    id: String,
    principal_id: String,
    source: String,
    event_types: Vec<EventKind>,
    relations: Vec<Relation>,
    permissions: Vec<String>,
    starts_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ends_at: Option<Timestamp>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema: String,
    id: String,
    agreement_id: String,
    version: u32,
    policy: String,
    roles: String,
    assent: String,
    context: String,
    acceptor: String,
    accepted_at: Timestamp,
    starts_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ends_at: Option<Timestamp>,
    service: String,
    customer: String,
    sources: Vec<String>,
    event_types: Vec<EventKind>,
    unit: String,
    maximum_quantity: Decimal,
    correction_sources: Vec<String>,
    allocation_view: bool,
}
#[derive(Clone, Debug)]
enum DocumentData {
    Policy(CompiledPolicy),
    Roles(Roles),
    Assent(Assent),
    SourceGrant(SourceGrant),
    Context(Context),
    Binding(Box<Binding>),
}
#[derive(Clone, Debug)]
pub struct Document {
    id: String,
    kind: &'static str,
    purpose: &'static str,
    body: Value,
    bytes: CanonicalBytes,
    content_hash: String,
    data: DocumentData,
}
impl Document {
    /// Parse and normalize one of the six locally retained Phase 1 input documents.
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut value = canonical::parse(bytes)?;
        canonical::no_null(&value)?;
        let schema = value
            .get("schema")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::new("SCHEMA", "document discriminator"))?
            .to_string();
        let (kind, purpose, data, body) = match schema.as_str() {
            "ledger-policy/1" => {
                let p = CompiledPolicy::from_value(value)?;
                ("policy", "policy", DocumentData::Policy(p.clone()), p.body)
            }
            "ledger-roles/1" => {
                value
                    .as_object_mut()
                    .ok_or_else(|| Error::new("SCHEMA", "roles object"))?
                    .remove("schema");
                let roles: Roles = canonical::dto(value.clone())?;
                roles.validate()?;
                value
                    .as_object_mut()
                    .expect("object")
                    .insert("schema".into(), json!(schema));
                ("roles", "roles", DocumentData::Roles(roles), value)
            }
            "ledger-assent/1" => {
                let a: Assent = canonical::dto(value)?;
                for s in [
                    &a.agreement_id,
                    &a.acceptor,
                    &a.bearer,
                    &a.payer,
                    &a.recipient,
                ] {
                    text(s, 128)?;
                }
                text(&a.evidence_ref, 256)?;
                prefixed(&a.evidence_digest, "sha256:")?;
                if !["demo", "real"].contains(&a.mode.as_str()) {
                    return Err(Error::new("SCHEMA", "assent mode"));
                }
                (
                    "assent",
                    "assent",
                    DocumentData::Assent(a.clone()),
                    json!(a),
                )
            }
            "ledger-source-grant/1" => {
                let mut g: SourceGrant = canonical::dto(value)?;
                text(&g.id, 128)?;
                text(&g.principal_id, 128)?;
                validate_source(&g.source)?;
                bounded_set(&mut g.event_types, 1, 7)?;
                bounded_set(&mut g.relations, 0, 5)?;
                bounded_set(&mut g.permissions, 1, 7)?;
                if g.permissions.iter().any(|s| {
                    ![
                        "submit",
                        "read",
                        "preview",
                        "authorize_invocation",
                        "terms_admin",
                        "source_admin",
                        "operate",
                    ]
                    .contains(&s.as_str())
                }) {
                    return Err(Error::new("SCHEMA", "grant permission"));
                }
                interval(&g.starts_at, g.ends_at.as_ref())?;
                (
                    "source-grant",
                    "source_grant",
                    DocumentData::SourceGrant(g.clone()),
                    json!(g),
                )
            }
            "ledger-context/1" => {
                let mut c: Context = canonical::dto(value)?;
                validate_currency(&c.currency, c.scale)?;
                if !["byok", "platform"].contains(&c.funding.as_str()) {
                    return Err(Error::new("SCHEMA", "funding"));
                }
                if let Some(tier) = &c.tier {
                    slug(tier)?;
                }
                for id in &c.binding_ids {
                    text(id, 128)?;
                }
                bounded_set(&mut c.binding_ids, 1, 16)?;
                (
                    "context",
                    "chain_context",
                    DocumentData::Context(c.clone()),
                    json!(c),
                )
            }
            "ledger-binding/1" => {
                let mut b: Binding = canonical::dto(value)?;
                for s in [&b.id, &b.agreement_id, &b.acceptor, &b.customer] {
                    text(s, 128)?;
                }
                for id in [&b.policy, &b.roles, &b.assent, &b.context] {
                    prefixed(id, "doc_")?;
                }
                slug(&b.service)?;
                slug(&b.unit)?;
                for s in b.sources.iter().chain(&b.correction_sources) {
                    validate_source(s)?;
                }
                bounded_set(&mut b.sources, 1, 32)?;
                bounded_set(&mut b.correction_sources, 0, 32)?;
                bounded_set(&mut b.event_types, 1, 7)?;
                if b.version == 0 {
                    return Err(Error::new("SCHEMA", "binding version"));
                }
                interval(&b.starts_at, b.ends_at.as_ref())?;
                (
                    "binding",
                    "binding",
                    DocumentData::Binding(Box::new(b.clone())),
                    json!(b),
                )
            }
            _ => {
                return Err(Error::new(
                    "UNSUPPORTED_SLICE",
                    "document family not in first slice",
                ))
            }
        };
        let bytes = CanonicalBytes::from_value(&body)?;
        if bytes.as_slice().len() > canonical::CANDIDATE_LIMIT {
            return Err(Error::new("LIMIT", "document bytes"));
        }
        let tuple = json!([kind, 1, body]);
        let id = canonical::identity(Domain::Document, &tuple)?;
        let content_hash = canonical::digest(Domain::Document, &tuple)?;
        Ok(Self {
            id,
            kind,
            purpose,
            body,
            bytes,
            content_hash,
            data,
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn document_type(&self) -> &str {
        self.kind
    }
    pub fn body(&self) -> &Value {
        &self.body
    }
    pub fn bytes(&self) -> &CanonicalBytes {
        &self.bytes
    }
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
    pub fn envelope(&self, scope: &Scope) -> Record {
        Record {
            kind: "document".into(),
            scope: scope.clone(),
            id: json!(self.id),
            body: self.body.clone(),
            content_hash: self.content_hash.clone(),
            document_type: Some(self.kind.into()),
        }
    }
}
fn bounded_set<T: Serialize>(v: &mut Vec<T>, min: usize, max: usize) -> Result<()> {
    if v.len() < min || v.len() > max {
        return Err(Error::new("LIMIT", "document collection"));
    }
    canonical::sort_set(v)
}
fn interval(start: &Timestamp, end: Option<&Timestamp>) -> Result<()> {
    if end.is_some_and(|end| end.micros() <= start.micros()) {
        return Err(Error::new("TIMESTAMP", "empty validity interval"));
    }
    Ok(())
}
fn current(start: &Timestamp, end: Option<&Timestamp>, at: &Timestamp) -> bool {
    at.micros() >= start.micros() && end.is_none_or(|e| at.micros() < e.micros())
}

/// Environmental observations supplied by the coordinator after ordered locks.
/// `received_at` checks current rights only; it is not a pricing input in this slice.
#[derive(Clone, Debug)]
pub struct AcceptanceContext {
    pub principal_id: String,
    pub grant_revision: Revision,
    pub grant_active: bool,
    pub binding_active: bool,
    pub chain_revision: Revision,
    pub chain_event_count: Revision,
    pub received_at: Timestamp,
}
#[derive(Clone, Debug)]
pub struct ResolvedInput {
    event: Event,
    documents: Vec<Document>,
    context: AcceptanceContext,
}
impl ResolvedInput {
    pub fn new(event: Event, documents: Vec<Document>, context: AcceptanceContext) -> Result<Self> {
        let input = Self {
            event,
            documents,
            context,
        };
        input.validate()?;
        Ok(input)
    }
    pub fn event(&self) -> &Event {
        &self.event
    }
    pub fn documents(&self) -> &[Document] {
        &self.documents
    }
    fn document(&self, purpose: &str) -> &Document {
        self.documents
            .iter()
            .find(|d| d.purpose == purpose)
            .expect("validated document family")
    }
    fn validate(&self) -> Result<()> {
        if self.documents.len() != 6 {
            return Err(Error::new(
                "UNRESOLVED_INPUT",
                "six input documents required",
            ));
        }
        let mut purposes = BTreeSet::new();
        let mut bytes = 0;
        for d in &self.documents {
            if !purposes.insert(d.purpose) {
                return Err(Error::new("DUPLICATE", "document purpose"));
            }
            bytes += d.bytes.as_slice().len();
        }
        if bytes > 8 * 1024 * 1024 {
            return Err(Error::new("LIMIT", "resolved bytes"));
        }
        let event = &self.event;
        if event.dto().kind != EventKind::Generated
            || event.dto().links.as_ref().is_some_and(|l| !l.is_empty())
            || event.dto().evidence.as_ref().is_some_and(|l| !l.is_empty())
            || event.dto().invocation_id.is_some()
            || event.dto().corrects.is_some()
        {
            return Err(Error::new(
                "UNSUPPORTED_SLICE",
                "only unlinked generated completion without invocation/correction/evidence",
            ));
        }
        let DocumentData::Policy(policy) = &self.document("policy").data else {
            unreachable!()
        };
        let DocumentData::Roles(roles) = &self.document("roles").data else {
            unreachable!()
        };
        let DocumentData::Assent(assent) = &self.document("assent").data else {
            unreachable!()
        };
        let DocumentData::SourceGrant(grant) = &self.document("source_grant").data else {
            unreachable!()
        };
        let DocumentData::Context(chain) = &self.document("chain_context").data else {
            unreachable!()
        };
        let DocumentData::Binding(binding) = &self.document("binding").data else {
            unreachable!()
        };
        for (actual, purpose) in [
            (&binding.policy, "policy"),
            (&binding.roles, "roles"),
            (&binding.assent, "assent"),
            (&binding.context, "chain_context"),
        ] {
            if actual != self.document(purpose).id() {
                return Err(Error::new(
                    "DOCUMENT_MISMATCH",
                    "binding document reference",
                ));
            }
        }
        if chain.binding_ids != [binding.id.clone()] || binding.allocation_view {
            return Err(Error::new(
                "UNSUPPORTED_SLICE",
                "one retail binding without allocation view",
            ));
        }
        if chain.currency != policy.currency || chain.scale != policy.scale {
            return Err(Error::new("CURRENCY_MISMATCH", "context/policy"));
        }
        if !self.context.grant_active
            || !self.context.binding_active
            || self.context.grant_revision.value() == 0
            || grant.principal_id != self.context.principal_id
            || grant.source != event.source()
            || !grant.event_types.contains(&event.dto().kind)
            || !["submit", "read"]
                .iter()
                .all(|p| grant.permissions.iter().any(|v| v == p))
            || !current(
                &grant.starts_at,
                grant.ends_at.as_ref(),
                &self.context.received_at,
            )
        {
            return Err(Error::new("SOURCE_UNAUTHORIZED", "resolved active grant"));
        }
        if !binding.sources.iter().any(|s| s == event.source())
            || !binding.event_types.contains(&event.dto().kind)
            || binding.customer != event.dto().customer
            || binding.service != "generation"
            || event
                .dto()
                .binding_id
                .as_ref()
                .is_some_and(|b| b != &binding.id)
            || !current(
                &binding.starts_at,
                binding.ends_at.as_ref(),
                &self.context.received_at,
            )
            || binding.accepted_at.micros() > self.context.received_at.micros()
        {
            return Err(Error::new("TERMS_NOT_ACCEPTED", "binding scope/validity"));
        }
        if event.dto().unit.as_deref() != Some(&binding.unit) {
            return Err(Error::new("UNIT_MISMATCH", "binding unit"));
        }
        let quantity = event
            .dto()
            .quantity
            .as_ref()
            .expect("normalized work quantity");
        // Decimal comparison uses a bounded exact difference, never rounded atoms.
        if quantity
            .ratio()
            .add(&binding.maximum_quantity.ratio().negated())?
            .is_positive()
        {
            return Err(Error::new("QUANTITY", "binding maximum"));
        }
        if assent.mode != "demo" || event.scope().environment() != "sandbox" {
            return Err(Error::new(
                "UNSUPPORTED_SLICE",
                "Phase 1 requires explicit sandbox demo assent",
            ));
        }
        if roles.payer_delegation.is_some()
            || roles.bearer != roles.payer
            || roles.bearer != binding.customer
            || assent.agreement_id != binding.agreement_id
            || assent.terms_version.value() != u64::from(binding.version)
            || assent.acceptor != binding.acceptor
            || assent.accepted_at != binding.accepted_at
            || assent.bearer != roles.bearer
            || assent.payer != roles.payer
            || assent.recipient != roles.recipient
        {
            return Err(Error::new("TERMS_NOT_ACCEPTED", "assent/roles agreement"));
        }
        if self.context.chain_event_count.value() >= 1000 {
            return Err(Error::new("LIMIT", "chain events"));
        }
        self.context.chain_revision.next()?;
        self.context.chain_event_count.next()?;
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Charge,
    Discount,
}
#[derive(Clone, Debug, Serialize)]
pub struct Action {
    schema: &'static str,
    id: String,
    scope: Scope,
    event_id: String,
    decision_id: String,
    effect_id: String,
    obligation_id: String,
    kind: ActionKind,
    book: &'static str,
    component: String,
    amount: Money,
    roles: Roles,
    roles_doc: String,
    rule_id: String,
    binding_id: String,
    snapshot_doc: String,
    sources: Vec<String>,
    links: Vec<String>,
    inputs: Vec<String>,
}
impl Action {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn amount(&self) -> &Money {
        &self.amount
    }
    pub fn inputs(&self) -> &[String] {
        &self.inputs
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExplanationInput {
    Decimal {
        name: String,
        value: Decimal,
        exact: ExactRatio,
    },
    Money {
        name: String,
        value: Money,
    },
    Boolean {
        name: String,
        value: bool,
    },
    SourceId {
        name: String,
        value: String,
    },
    BindingField {
        name: String,
        value: String,
    },
    DocumentRef {
        name: String,
        value: String,
    },
    ActionRef {
        name: String,
        value: String,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct Explanation {
    schema: &'static str,
    id: String,
    scope: Scope,
    event_id: String,
    ordinal: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    rule_id: Option<String>,
    outcome: &'static str,
    code: &'static str,
    binding_id: String,
    input_refs: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    basis_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    basis: Option<ExactRatio>,
    inputs: Vec<ExplanationInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unrounded_atoms: Option<ExactRatio>,
    #[serde(skip_serializing_if = "Option::is_none")]
    rounded_atoms: Option<String>,
    action_ids: Vec<String>,
}
impl Explanation {
    pub fn code(&self) -> &str {
        self.code
    }
    pub fn id(&self) -> &str {
        &self.id
    }
}
#[derive(Clone, Debug, Serialize)]
struct ActionBreakdown {
    action_id: String,
    kind: ActionKind,
    component: String,
    amount: Money,
}
#[derive(Clone, Debug, Serialize)]
struct ObligationDelta {
    schema: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    obligation_id: String,
    agreement_id: String,
    book: &'static str,
    amount: Money,
    roles: Roles,
    actions: Vec<ActionBreakdown>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Intention {
    schema: &'static str,
    id: String,
    scope: Scope,
    event_id: String,
    destination_id: &'static str,
    idempotency_key: String,
    obligation_id: String,
    action_ids: Vec<String>,
    amount: Money,
    depends_on: Vec<String>,
    payload: ObligationDelta,
}
impl Intention {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn amount(&self) -> &Money {
        &self.amount
    }
    pub fn payload_bytes(&self) -> Result<CanonicalBytes> {
        CanonicalBytes::from_value(&self.payload)
    }
    pub fn request_digest(&self) -> Result<String> {
        canonical::digest(Domain::IntentionPayload, &self.payload)
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    schema: &'static str,
    id: String,
    event_id: String,
    decision_id: String,
    chain_id: String,
    revision: Revision,
    action_ids: Vec<String>,
    intention_ids: Vec<String>,
    content_hash: String,
    decision_hash: String,
}
impl Receipt {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
    pub fn decision_hash(&self) -> &str {
        &self.decision_hash
    }
    pub fn bytes(&self) -> Result<CanonicalBytes> {
        CanonicalBytes::from_value(self)
    }
}

/// An immutable canonical journal envelope. Construction stays inside the core.
#[derive(Clone, Debug, Serialize)]
pub struct Record {
    kind: String,
    scope: Scope,
    id: Value,
    body: Value,
    content_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    document_type: Option<String>,
}
impl Record {
    pub fn kind(&self) -> &str {
        &self.kind
    }
    pub fn id(&self) -> &Value {
        &self.id
    }
    pub fn body(&self) -> &Value {
        &self.body
    }
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
    pub fn bytes(&self) -> Result<CanonicalBytes> {
        CanonicalBytes::from_value(self)
    }
    fn generic(kind: &str, scope: &Scope, id: Value, body: impl Serialize) -> Result<Self> {
        let body = serde_json::to_value(body).map_err(|e| Error::new("SCHEMA", e.to_string()))?;
        let content_hash = canonical::digest(Domain::RecordContent, &json!([kind, 1, body]))?;
        Ok(Self {
            kind: kind.into(),
            scope: scope.clone(),
            id,
            body,
            content_hash,
            document_type: None,
        })
    }
}
#[derive(Clone, Debug)]
pub struct DecisionPlan {
    records: Vec<Record>,
    actions: Vec<Action>,
    explanations: Vec<Explanation>,
    intentions: Vec<Intention>,
    receipt: Receipt,
    claim_facts: Value,
    effect_facts: Vec<Value>,
}
impl DecisionPlan {
    pub fn records(&self) -> &[Record] {
        &self.records
    }
    pub fn actions(&self) -> &[Action] {
        &self.actions
    }
    pub fn explanations(&self) -> &[Explanation] {
        &self.explanations
    }
    pub fn intentions(&self) -> &[Intention] {
        &self.intentions
    }
    pub fn receipt(&self) -> &Receipt {
        &self.receipt
    }
    pub fn claim_facts(&self) -> &Value {
        &self.claim_facts
    }
    pub fn effect_facts(&self) -> &[Value] {
        &self.effect_facts
    }
    pub fn journal_bytes(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        for record in &self.records {
            bytes.extend(record.bytes()?.as_slice());
            bytes.push(b'\n');
        }
        if bytes.len() > canonical::BUNDLE_LIMIT {
            return Err(Error::new("LIMIT", "decision bundle"));
        }
        Ok(bytes)
    }
}
fn record_sort(records: &mut [Record]) -> Result<()> {
    let mut keys = std::collections::BTreeMap::new();
    for r in records.iter() {
        let key = (
            r.kind.clone(),
            CanonicalBytes::from_value(&r.id)?.into_vec(),
        );
        if keys.insert(key, ()).is_some() {
            return Err(Error::new("DUPLICATE", "journal kind/ID"));
        }
    }
    records.sort_by_cached_key(|r| {
        (
            r.kind.clone(),
            CanonicalBytes::from_value(&r.id)
                .expect("checked key")
                .into_vec(),
        )
    });
    Ok(())
}

/// Assemble records from the same typed evaluation path used by `policy::evaluate`.
pub fn assemble(input: &ResolvedInput) -> Result<DecisionPlan> {
    assemble_inner(input, None)
}
pub(crate) fn assemble_inner(
    input: &ResolvedInput,
    failure: Option<&'static str>,
) -> Result<DecisionPlan> {
    input.validate()?;
    let e = &input.event;
    let scope = e.scope();
    let event_id = e.id();
    let decision_id = canonical::identity(Domain::Decision, &json!([event_id]))?;
    let receipt_id = canonical::identity(Domain::Receipt, &json!([event_id]))?;
    let claim_id = e.completion_claim_id()?;
    let claim_facts = e.completion_facts(&[])?;
    let policy_doc = input.document("policy");
    let roles_doc = input.document("roles");
    let context_doc = input.document("chain_context");
    let grant_doc = input.document("source_grant");
    let DocumentData::Policy(policy) = &policy_doc.data else {
        unreachable!()
    };
    let DocumentData::Roles(roles) = &roles_doc.data else {
        unreachable!()
    };
    let DocumentData::Context(context) = &context_doc.data else {
        unreachable!()
    };
    let DocumentData::SourceGrant(grant) = &grant_doc.data else {
        unreachable!()
    };
    let DocumentData::Binding(binding) = &input.document("binding").data else {
        unreachable!()
    };
    let mut documents: Vec<_> = input
        .documents
        .iter()
        .map(|d| json!({"purpose":d.purpose,"document_id":d.id}))
        .collect();
    canonical::sort_set(&mut documents)?;
    let mut context_value = context_doc.body.clone();
    context_value
        .as_object_mut()
        .expect("context")
        .remove("schema");
    let snapshot = json!({"schema":"ledger-snapshot/1","scope":scope,"dsl_version":1,"semantics_version":1,"documents":documents,"context":context_value,"authority":[{"active":true,"grant_document":grant_doc.id,"grant_id":grant.id,"principal_id":input.context.principal_id,"revision":input.context.grant_revision,"source":e.source()}],"prior_actions":[],"decision_context":{}});
    let snapshot_id = canonical::identity(Domain::Document, &json!(["snapshot", 1, snapshot]))?;
    let snapshot_hash = canonical::digest(Domain::Document, &json!(["snapshot", 1, snapshot]))?;
    let obligation_id = canonical::identity(
        Domain::Obligation,
        &json!([
            scope,
            binding.agreement_id,
            "retail",
            policy.currency,
            policy.scale,
            roles
        ]),
    )?;
    let mut records = vec![Record {
        kind: "document".into(),
        scope: scope.clone(),
        id: json!(snapshot_id),
        body: snapshot,
        content_hash: snapshot_hash,
        document_type: Some("snapshot".into()),
    }];
    for (purpose, id) in input
        .documents
        .iter()
        .map(|d| (d.purpose, d.id.as_str()))
        .chain(std::iter::once(("decision_snapshot", snapshot_id.as_str())))
    {
        let id_ref = canonical::identity(Domain::SnapshotRef, &json!([event_id, purpose, id]))?;
        records.push(Record::generic("snapshot-ref",scope,json!(id_ref),json!({"schema":"ledger-snapshot-ref/1","id":id_ref,"scope":scope,"event_id":event_id,"purpose":purpose,"document_id":id}))?);
    }
    records.push(Record {
        kind: "event".into(),
        scope: scope.clone(),
        id: json!(event_id),
        body: json!(e.dto()),
        content_hash: e.content_hash().into(),
        document_type: None,
    });
    let ingress = canonical::parse(e.candidate().ingress_bytes().as_slice())?;
    records.push(Record::generic("delivery-key",scope,json!([scope,e.source(),e.dto().id]),json!({"schema":"ledger-delivery-key/1","scope":scope,"source":e.source(),"external_id":e.dto().id,"canonical_event_id":event_id,"kind":"original","ingress":ingress,"ingress_hash":e.candidate().ingress_hash()}))?);
    records.push(Record::generic("claim",scope,json!(claim_id),json!({"schema":"ledger-claim/1","id":claim_id,"scope":scope,"source":e.source(),"operation_id":e.candidate().operation_id(),"kind":"completion","token":"completion","event_id":event_id,"facts_hash":canonical::digest(Domain::ClaimFacts,&claim_facts)?}))?);
    let steps = policy.evaluate_steps(e, context, failure)?;
    let mut actions: Vec<Action> = Vec::new();
    let mut explanations = Vec::new();
    let mut effect_facts = Vec::new();
    let mut rule_actions = vec![None::<String>; policy.rules.len()];
    for (ordinal, step) in steps.into_iter().enumerate() {
        let rule = step.rule.map(|i| &policy.rules[i]);
        let explanation_id = canonical::identity(Domain::Explanation, &json!([event_id, ordinal]))?;
        let mut input_refs = vec![policy_doc.id.clone()];
        let mut inputs = Vec::new();
        for (name, value) in step.predicate_inputs {
            if name.starts_with("binding.") {
                input_refs.push(context_doc.id.clone());
            }
            inputs.push(match value {
                PredicateValue::Text(value) => ExplanationInput::BindingField { name, value },
                PredicateValue::Boolean(value) => ExplanationInput::Boolean { name, value },
                PredicateValue::Source(value) => ExplanationInput::SourceId { name, value },
            });
        }
        let mut basis_name = None;
        let mut dependencies = Vec::new();
        if let Some(rule) = rule {
            match &rule.operation {
                Operation::Base { fixed } => {
                    if step.unrounded.is_some() {
                        inputs.push(ExplanationInput::Decimal {
                            name: "fixed".into(),
                            value: fixed.clone(),
                            exact: fixed.ratio(),
                        });
                    }
                }
                Operation::Discount {
                    percent,
                    basis,
                    basis_name: name,
                } => {
                    if step.unrounded.is_some() {
                        inputs.push(ExplanationInput::Decimal {
                            name: "percent".into(),
                            value: percent.clone(),
                            exact: percent.ratio(),
                        });
                        if let Some(action_id) = &rule_actions[*basis] {
                            dependencies.push(action_id.clone());
                            input_refs.push(action_id.clone());
                            inputs.push(ExplanationInput::ActionRef {
                                name: "basis".into(),
                                value: action_id.clone(),
                            });
                        }
                        basis_name = Some(name.clone());
                    }
                }
            }
        }
        input_refs.sort();
        input_refs.dedup();
        canonical::sort_set(&mut input_refs)?;
        let mut action_ids = Vec::new();
        if let Some(atoms) = step.rounded.filter(|n| *n != 0) {
            let rule = rule.expect("applied step has rule");
            let rule_index = step.rule.expect("rule index");
            let effect_id = canonical::identity(
                Domain::Effect,
                &json!([
                    scope,
                    binding.agreement_id,
                    rule.component,
                    claim_id,
                    "self",
                    "original"
                ]),
            )?;
            let action_id = canonical::identity(Domain::Action, &json!([effect_id]))?;
            let amount = Money::new(&policy.currency, policy.scale, atoms)?;
            let action = Action {
                schema: "ledger-action/1",
                id: action_id.clone(),
                scope: scope.clone(),
                event_id: event_id.into(),
                decision_id: decision_id.clone(),
                effect_id: effect_id.clone(),
                obligation_id: obligation_id.clone(),
                kind: match rule.operation {
                    Operation::Base { .. } => ActionKind::Charge,
                    Operation::Discount { .. } => ActionKind::Discount,
                },
                book: "retail",
                component: rule.component.clone(),
                amount,
                roles: roles.clone(),
                roles_doc: roles_doc.id.clone(),
                rule_id: rule.id.clone(),
                binding_id: binding.id.clone(),
                snapshot_doc: snapshot_id.clone(),
                sources: vec![event_id.into()],
                links: vec![],
                inputs: dependencies,
            };
            let facts = json!({"schema":"ledger-effect-facts/1","scope":scope,"agreement_id":binding.agreement_id,"component":rule.component,"claim_id":claim_id,"match_key":"self","namespace":"original","kind":action.kind,"book":action.book,"amount":action.amount,"roles":roles,"sources":action.sources,"links":action.links,"inputs":action.inputs});
            records.push(Record::generic("effect",scope,json!(effect_id),json!({"schema":"ledger-effect/1","id":effect_id,"scope":scope,"agreement_id":binding.agreement_id,"component":rule.component,"claim_id":claim_id,"match_key":"self","namespace":"original","facts_hash":canonical::digest(Domain::EffectFacts,&facts)?,"action_id":action_id}))?);
            effect_facts.push(facts);
            records.push(Record::generic("action", scope, json!(action_id), &action)?);
            records.push(Record::generic("action-source",scope,json!([scope,action_id,event_id]),json!({"schema":"ledger-action-source/1","scope":scope,"action_id":action_id,"event_id":event_id}))?);
            for predecessor in &action.inputs {
                records.push(Record::generic("action-dependency",scope,json!([scope,action_id,predecessor]),json!({"schema":"ledger-action-dependency/1","scope":scope,"action_id":action_id,"input_action_id":predecessor}))?);
            }
            action_ids.push(action_id.clone());
            rule_actions[rule_index] = Some(action_id);
            actions.push(action);
        }
        let explanation = Explanation {
            schema: "ledger-explanation/1",
            id: explanation_id.clone(),
            scope: scope.clone(),
            event_id: event_id.into(),
            ordinal: ordinal as u8,
            rule_id: rule.map(|r| r.id.clone()),
            outcome: if !action_ids.is_empty() {
                "applied"
            } else if step.rounded == Some(0) {
                "zero"
            } else {
                "skipped"
            },
            code: step.code,
            binding_id: binding.id.clone(),
            input_refs,
            basis_name,
            basis: step.basis.map(|(_, r)| r),
            inputs,
            unrounded_atoms: step.unrounded,
            rounded_atoms: step.rounded.map(|a| a.to_string()),
            action_ids,
        };
        records.push(Record::generic(
            "explanation",
            scope,
            json!(explanation_id),
            &explanation,
        )?);
        explanations.push(explanation);
    }
    if actions.len() > 128 || explanations.len() > 256 {
        return Err(Error::new("LIMIT", "evaluation outputs"));
    }
    if CanonicalBytes::from_value(&explanations)?.as_slice().len() > 1024 * 1024 {
        return Err(Error::new("LIMIT", "explanations"));
    }
    let mut action_ids: Vec<_> = actions.iter().map(|a| a.id.clone()).collect();
    canonical::sort_set(&mut action_ids)?;
    let mut intentions = Vec::new();
    if !actions.is_empty() {
        let amount = actions
            .iter()
            .try_fold(Money::new(&policy.currency, policy.scale, 0)?, |sum, a| {
                sum.checked_add(&a.amount)
            })?;
        let id = canonical::identity(
            Domain::Intention,
            &json!([scope, "fake", obligation_id, action_ids]),
        )?;
        let mut breakdown: Vec<_> = actions
            .iter()
            .map(|a| ActionBreakdown {
                action_id: a.id.clone(),
                kind: a.kind.clone(),
                component: a.component.clone(),
                amount: a.amount.clone(),
            })
            .collect();
        canonical::sort_set(&mut breakdown)?;
        let payload = ObligationDelta {
            schema: "ledger-obligation-delta/1",
            kind: "obligation_delta",
            obligation_id: obligation_id.clone(),
            agreement_id: binding.agreement_id.clone(),
            book: "retail",
            amount: amount.clone(),
            roles: roles.clone(),
            actions: breakdown,
        };
        let intention = Intention {
            schema: "ledger-intention/1",
            id: id.clone(),
            scope: scope.clone(),
            event_id: event_id.into(),
            destination_id: "fake",
            idempotency_key: id.clone(),
            obligation_id,
            action_ids: action_ids.clone(),
            amount,
            depends_on: vec![],
            payload,
        };
        records.push(Record::generic("intention", scope, json!(id), &intention)?);
        intentions.push(intention);
    }
    let revision = input.context.chain_revision.next()?;
    let count = input.context.chain_event_count.next()?;
    let transition = canonical::identity(
        Domain::ControlTransition,
        &json!([scope, "chain", e.chain(), revision]),
    )?;
    records.push(Record::generic("control-transition",scope,json!(transition),json!({"schema":"ledger-control-transition/1","id":transition,"scope":scope,"control_kind":"chain","control_id":e.chain(),"from_revision":input.context.chain_revision,"to_revision":revision,"event_id":event_id,"document_id":snapshot_id,"from_event_count":input.context.chain_event_count,"to_event_count":count}))?);
    records.push(Record::generic("chain-revision",scope,json!([scope,e.chain(),revision]),json!({"schema":"ledger-chain-revision/1","scope":scope,"chain_id":e.chain(),"revision":revision,"event_id":event_id,"decision_id":decision_id}))?);
    let mut membership = records.clone();
    membership.extend(input.documents.iter().map(|d| d.envelope(scope)));
    record_sort(&mut membership)?;
    let members: Vec<_> = membership
        .iter()
        .map(|r| json!({"kind":r.kind,"id":r.id,"content_hash":r.content_hash}))
        .collect();
    let explanation_ids: Vec<_> = explanations.iter().map(|e| &e.id).collect();
    let manifest = json!({"schema":"ledger-decision-manifest/1","id":decision_id,"scope":scope,"event_id":event_id,"chain_id":e.chain(),"revision":revision,"explanation_ids":explanation_ids,"members":members});
    let decision_hash = canonical::digest(Domain::DecisionContent, &manifest)?;
    records.push(Record {
        kind: "decision-manifest".into(),
        scope: scope.clone(),
        id: json!(decision_id),
        body: manifest,
        content_hash: decision_hash.clone(),
        document_type: None,
    });
    let mut intention_ids: Vec<_> = intentions.iter().map(|i| i.id.clone()).collect();
    canonical::sort_set(&mut intention_ids)?;
    let receipt = Receipt {
        schema: "ledger-receipt/1",
        id: receipt_id.clone(),
        event_id: event_id.into(),
        decision_id,
        chain_id: e.chain().into(),
        revision,
        action_ids,
        intention_ids,
        content_hash: e.content_hash().into(),
        decision_hash,
    };
    records.push(Record::generic(
        "receipt",
        scope,
        json!(receipt_id),
        &receipt,
    )?);
    record_sort(&mut records)?;
    let plan = DecisionPlan {
        records,
        actions,
        explanations,
        intentions,
        receipt,
        claim_facts,
        effect_facts,
    };
    plan.journal_bytes()?;
    Ok(plan)
}

/// Original-input replay. Exact complete journal bytes, framing, fields, order,
/// references and hashes must match; a rehashed malformed record still fails.
/// This does not accept, insert records, obtain authority, or dispatch anything.
pub fn verify(input: &ResolvedInput, journal: &[u8]) -> Result<DecisionPlan> {
    if journal.len() > canonical::BUNDLE_LIMIT {
        return Err(Error::new("LIMIT", "journal bytes"));
    }
    let plan = assemble(input)?;
    if plan.journal_bytes()?.as_slice() != journal {
        return Err(Error::new(
            "INTEGRITY_FAILURE",
            "original-input journal mismatch",
        ));
    }
    Ok(plan)
}
