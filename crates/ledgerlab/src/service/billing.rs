//! Ordinary local retail billing. Uses the existing evaluator and frozen v2
//! envelopes; it never creates a supplier invocation or R3 capacity claim.
use super::retained::base as b;
use b::{array, bytes, core, reference, row, text, Records, Result};
use ledgerlab_core::{
    canonical::{self, Domain},
    domain::{self, Revision, Roles, Scope, Timestamp},
    money::Decimal,
    policy::chaining as c,
    wire::EventKind,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
pub(crate) mod adjustment;
pub(crate) mod control;
mod history;
pub(crate) mod permissions;
mod setup_policy;

/// Trusted local control input. Event input never supplies these values.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    #[serde(skip)]
    pub grant_revision: u64,
    pub schema: String,
    pub scope: Scope,
    pub store_id: String,
    pub operator: String,
    pub source: String,
    pub customer: String,
    pub host: String,
    pub agreement: String,
    pub binding: String,
    pub price: String,
    pub accepted_at: Timestamp,
    pub acceptor: String,
    /// Retained asserted commercial assent, supplied by the operator, not a URL
    /// fetched by Ledger Lab or a claim that the program obtained consent.
    pub assent_evidence: String,
    pub operator_attestation: String,
    pub finality_attestation: String,
    pub permissions: Vec<String>,
    /// The existing outcome policy vocabulary, supplied explicitly with terms.
    /// The document identity is computed by this service, never caller-selected.
    pub outcome_policy: Value,
}
pub(crate) fn reject(code: &str) -> crate::ServiceError {
    crate::ServiceError::Rejection(code.into())
}
pub(crate) fn require(ok: bool, code: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(reject(code))
    }
}
fn string(v: &Value) -> Result<String> {
    String::from_utf8(bytes(v)?).map_err(|_| b::integrity())
}
fn evidence(scope: &Value, purpose: &str, kind: &str, body: Value) -> Result<Value> {
    let hash = core(canonical::digest(Domain::Document, &json!([kind, 1, body])))?;
    row(
        "evidence",
        scope,
        json!({"purpose":purpose,"media_type":"application/json","document_type":kind,"document_version":1,"document_id":format!("doc_{}",&hash[7..]),"document_hash":hash,"utf8":string(&body)?}),
    )
}
fn doc(r: &Value) -> String {
    r["body"]["document_id"]
        .as_str()
        .expect("constructed evidence")
        .into()
}
impl Setup {
    pub(crate) fn parse(raw: &[u8]) -> Result<Self> {
        let value = canonical::parse_bounded(raw, 65536).map_err(|_| reject("BILLING_SETUP"))?;
        let mut s: Self = serde_json::from_value(value).map_err(|_| reject("BILLING_SETUP"))?;
        s.grant_revision = 1;
        require(s.schema == "ledger-local-billing/1", "BILLING_SETUP")?;
        for v in [
            &s.store_id,
            &s.operator,
            &s.customer,
            &s.host,
            &s.agreement,
            &s.binding,
            &s.acceptor,
        ] {
            require(
                !v.is_empty() && v.len() <= 128 && !v.chars().any(char::is_control),
                "BILLING_IDENTIFIER",
            )?;
        }
        require(
            !s.source.is_empty()
                && s.source.len() <= 256
                && !s.source.chars().any(char::is_control),
            "BILLING_IDENTIFIER",
        )?;
        require(s.customer != s.host, "BILLING_PARTIES")?;
        for v in [
            &s.assent_evidence,
            &s.operator_attestation,
            &s.finality_attestation,
        ] {
            require(
                !v.trim().is_empty() && v.len() <= 8192,
                "BILLING_ASSENT_REQUIRED",
            )?;
        }
        let price = Decimal::parse(&s.price).map_err(|_| reject("BILLING_PRICE"))?;
        require(!price.is_zero(), "BILLING_PRICE")?;
        require(
            s.outcome_policy.is_object() && s.outcome_policy.get("document").is_none(),
            "BILLING_POLICY",
        )?;
        require(
            s.permissions.len() <= 3
                && s.permissions
                    .iter()
                    .all(|p| ["read", "submit", "correct"].contains(&p.as_str()))
                && s.permissions.iter().collect::<BTreeSet<_>>().len() == s.permissions.len(),
            "BILLING_PERMISSIONS",
        )?;
        setup_policy::validate(&s)?;
        Ok(s)
    }
}

/// Semantically validate the complete schema-8 billing history against the
/// exact setup from which schema 9 will seed its first customer/agreement.
pub(crate) fn validate_legacy_upgrade(
    snapshot: &mut crate::store::sqlite::BillingSnapshot,
    migration_at: &Timestamp,
) -> Result<Setup> {
    let initial = Setup::parse(&snapshot.setup)?;
    require(
        snapshot.customers.is_empty()
            && snapshot.agreements.is_empty()
            && snapshot.controls.is_empty()
            && snapshot.scoped_permissions.is_empty(),
        "BILLING_UPGRADE_STATE",
    )?;
    snapshot
        .customers
        .push(crate::store::sqlite::BillingCustomer {
            customer: initial.customer.clone(),
            tenant: initial.scope.tenant().to_owned(),
            environment: initial.scope.environment().to_owned(),
        });
    snapshot
        .agreements
        .push(crate::store::sqlite::BillingAgreement {
            customer: initial.customer.clone(),
            source: initial.source.clone(),
            revision: 1,
            transition: "start".into(),
            agreement_id: initial.agreement.clone(),
            agreement_version: 1,
            effective_at_us: initial.accepted_at.micros(),
            recorded_at_us: initial.accepted_at.micros(),
            setup: Some(snapshot.setup.clone()),
        });
    control::validate_customer_registry(snapshot)?;
    permissions::effective(snapshot)?;
    let audit = history::load(&initial, snapshot)?;
    control::ensure_new_ledger_time(snapshot, &audit, migration_at)?;
    Ok(initial)
}

pub(crate) fn validate_snapshot(snapshot: &crate::store::sqlite::BillingSnapshot) -> Result<()> {
    control::validate_customer_registry(snapshot)?;
    let initial = Setup::parse(&snapshot.setup)?;
    permissions::effective(snapshot)?;
    history::load(&initial, snapshot)?;
    let pairs = snapshot
        .agreements
        .iter()
        .map(|row| (row.customer.as_str(), row.source.as_str()))
        .collect::<BTreeSet<_>>();
    for (customer, source) in pairs {
        permissions::effective_for(snapshot, customer, source)?;
    }
    Ok(())
}

/// Build from newly evaluated inputs, never from a fixture or accepted journal.
/// The complete result is replayed through the existing decoder before use.
pub(crate) fn base(s: &Setup, raw: &[u8], received: &Timestamp) -> Result<Vec<Value>> {
    let scope = serde_json::to_value(&s.scope).map_err(|_| b::integrity())?;
    let event = domain::normalize(raw, s.scope.clone(), &s.source)
        .map_err(|e| reject(e.code))?
        .resolve(None)
        .map_err(|e| reject(e.code))?;
    require(
        event.source() == s.source && event.dto().customer == s.customer,
        "BILLING_SCOPE",
    )?;
    require(
        event.dto().kind == EventKind::Generated
            && event.dto().links.as_ref().is_none_or(Vec::is_empty)
            && event.dto().invocation_id.is_none()
            && event.dto().corrects.is_none()
            && event.dto().evidence.as_ref().is_none_or(Vec::is_empty),
        "BILLING_UNSUPPORTED_EVENT",
    )?;
    require(
        received.micros() >= s.accepted_at.micros(),
        "BILLING_TERMS_NOT_ACTIVE",
    )?;
    require(
        s.permissions.iter().any(|p| p == "submit") && s.permissions.iter().any(|p| p == "read"),
        "BILLING_UNAUTHORIZED",
    )?;
    let assent = evidence(
        &scope,
        "assent",
        "assent",
        json!({"mode":"real","agreement_id":s.agreement,"acceptor":s.acceptor,"bearer":s.customer,"payer":s.customer,"recipient":s.host,"accepted_at":s.accepted_at,"retained_evidence":s.assent_evidence,"operator":s.operator,"operator_attestation":s.operator_attestation,"terms":{"fixed_price":s.price,"currency":"USD","scale":2,"unit":"call","maximum_quantity":"1","outcomes":s.outcome_policy}}),
    )?;
    let grant = evidence(
        &scope,
        "grant",
        "grant",
        json!({"scope":scope,"principal":s.operator,"source":s.source,"permissions":s.permissions,"revision":s.grant_revision.to_string(),"active":true}),
    )?;
    let context_doc = evidence(
        &scope,
        "base_context",
        "base-context",
        json!({"customer":s.customer,"funding":"byok","agreement":s.agreement}),
    )?;
    let roles = core(Roles::new(
        [
            &s.host,
            &s.host,
            &s.customer,
            &s.customer,
            &s.customer,
            &s.host,
        ],
        None,
    ))?;
    let binding = c::Binding {
        id: s.binding.clone(),
        agreement: s.agreement.clone(),
        book: c::Book::Retail,
        roles,
        assent: doc(&assent),
        offer: None,
        sources: vec![s.source.clone()],
        event_types: vec![EventKind::Generated],
        unit: "call".into(),
        maximum_quantity: core(Decimal::parse("1"))?,
        maximum_exposure: None,
        outcome: None,
        correction_sources: vec![s.source.clone()],
        allowed_modifiers: vec![],
        allocation_view: false,
    };
    let bundle = c::Bundle::compile(
        "USD",
        2,
        vec![c::Policy {
            binding,
            rules: vec![c::Rule {
                id: "fixed-price".into(),
                on: EventKind::Generated,
                component: "generation.base".into(),
                when: vec![],
                matcher: None,
                operation: c::Operation::Base(c::Price::Fixed(core(Decimal::parse(&s.price))?)),
            }],
        }],
    )
    .map_err(|e| reject(e.code))?;
    let context = c::Context {
        document: doc(&context_doc),
        customer: s.customer.clone(),
        funding: c::Funding::Byok,
        tier: None,
        priority: None,
        stage: None,
    };
    let authority = c::SourceAuthority {
        source: s.source.clone(),
        grant: doc(&grant),
        revision: core(Revision::new(s.grant_revision))?,
        active: true,
        event_types: vec![EventKind::Generated],
        relations: vec![],
    };
    let evaluation = bundle
        .evaluate(c::Input {
            event: &event,
            context: &context,
            history: &[],
            source_authority: &authority,
            invocations: &[],
            costs: &[],
            received_at: received,
        })
        .map_err(|e| reject(e.code))?;
    let original = serde_json::to_value(&evaluation).map_err(|_| b::integrity())?;
    let material = b::projection(&original)?;
    let policy_doc = evidence(&scope, "policy", "policy", s.outcome_policy.clone())?;
    let mut policy = s.outcome_policy.clone();
    policy["document"] = json!(doc(&policy_doc));
    let target = c::outcomes::Target::freeze(
        &evaluation,
        b::policy(&policy)?,
        c::outcomes::TargetVerification {
            rated_final: true,
            accepted_at: received.clone(),
            policy_document: doc(&policy_doc),
            verified_assents: vec![doc(&assent)],
            verified_offers: vec![],
            verified_delegations: vec![],
        },
    )
    .map_err(|e| reject(e.code))?;
    require(
        target.policy().families.iter().all(|f| {
            f.binding_id == s.binding && f.source == s.source && f.correction_source == s.source
        }),
        "BILLING_POLICY_SCOPE",
    )?;
    let finality = evidence(
        &scope,
        "finality",
        "finality",
        json!({"event":event.id(),"principal":s.operator,"statement":s.finality_attestation,"terms":doc(&assent)}),
    )?;
    let mut rows = vec![
        assent.clone(),
        grant,
        context_doc,
        policy_doc.clone(),
        finality.clone(),
    ];
    let ev = &material["event"];
    let mut data = json!({"type":"base","source":event.source(),"external_id":ev["id"],"chain_id":ev["chain"],"work_type":ev["type"],"evidence":[]});
    for k in [
        "customer",
        "operation_id",
        "occurred_at",
        "status",
        "quantity",
        "unit",
    ] {
        if let Some(v) = ev.get(k) {
            data[k] = v.clone();
        }
    }
    let event_row = row("event", &scope, json!({"data":data}))?;
    let id = event_row["id"].clone();
    rows.push(event_row.clone());
    let mut binding_body = material["bundle"]["policies"][0]["binding"].clone();
    let original_binding = binding_body.clone();
    let ob = binding_body.as_object_mut().ok_or_else(b::integrity)?;
    ob.remove("id");
    ob.remove("agreement");
    ob.insert("binding_id".into(), json!(s.binding));
    ob.insert("agreement_id".into(), json!(s.agreement));
    ob.insert("assent".into(), assent["id"].clone());
    ob.insert("binding_utf8".into(), json!(string(&original_binding)?));
    ob.insert(
        "booked_net".into(),
        serde_json::to_value(target.retail_basis()).map_err(|_| b::integrity())?,
    );
    for k in [
        "sources",
        "event_types",
        "correction_sources",
        "allowed_modifiers",
    ] {
        binding_body[k] = json!(b::ordered(array(&binding_body[k])?.clone())?);
    }
    let binding_row = row("binding-snapshot", &scope, binding_body)?;
    rows.push(binding_row.clone());
    let mut maps = BTreeMap::<(String, String), Value>::new();
    maps.insert(("event".into(), event.id().into()), reference(&event_row));
    maps.insert(
        ("binding".into(), s.binding.clone()),
        reference(&binding_row),
    );
    for r in &rows {
        if r["kind"] == "evidence" {
            maps.insert(("document".into(), doc(r)), reference(r));
        }
    }
    let mut postings = vec![];
    for (ordinal, a) in array(&material["actions"])?.iter().enumerate() {
        let posting = row(
            "base-posting",
            &scope,
            json!({"event_id":id,"agreement_id":s.agreement,"book":"retail","ordinal":ordinal,"binding_id":s.binding,"amount":a["amount"],"roles":a["binding"]["roles"]}),
        )?;
        let obligation = row(
            "obligation",
            &scope,
            json!({"agreement_id":s.agreement,"book":"retail","currency":"USD","scale":2,"roles":a["binding"]["roles"]}),
        )?;
        maps.insert(
            ("action".into(), text(&a["id"])?.into()),
            reference(&posting),
        );
        maps.insert(
            ("obligation".into(), text(&a["obligation_id"])?.into()),
            reference(&obligation),
        );
        postings.push(reference(&posting));
        rows.push(posting);
        if !rows.iter().any(|r| r["id"] == obligation["id"]) {
            rows.push(obligation);
        }
    }
    let mut identities = BTreeSet::new();
    identity_refs(&material, &mut identities);
    identities.insert(("event".into(), event.id().into()));
    identities.insert(("binding".into(), s.binding.clone()));
    let mut mappings = vec![];
    for (kind, native) in identities {
        let projection = match maps.get(&(kind.clone(), native.clone())) {
            Some(r) => r.clone(),
            None => {
                let r = row(
                    "base-identity",
                    &scope,
                    json!({"target":id,"original_target":event.id(),"original_kind":kind,"original_id":native}),
                )?;
                let rf = reference(&r);
                rows.push(r);
                rf
            }
        };
        mappings.push(json!({"original_kind":kind,"original_id":native,"original_target":event.id(),"projection":projection,"target":id}));
    }
    let mut eval_body = json!({"event_id":id,"source_state":"accepted","predecessors":[],"received_at":received,"original_evaluation_utf8":string(&original)?,"evaluation_utf8":string(&material)?,"bindings":b::ordered(vec![reference(&binding_row)])?,"postings":b::ordered(postings.clone())?,"identity_mappings":b::ordered(mappings)?});
    for k in [
        "event_id",
        "event_hash",
        "event_utf8",
        "ingress_hash",
        "ingress_utf8",
    ] {
        eval_body[format!("original_{k}")] = original["event"][k].clone();
    }
    let eval_row = row("base-evaluation", &scope, eval_body)?;
    rows.push(eval_row.clone());
    let basis = row(
        "target-basis",
        &scope,
        json!({"target":id,"agreement_id":s.agreement,"book":"retail","payer":s.customer,"amount":target.retail_basis(),"postings":b::ordered(postings)?,"finality":"final","finality_evidence":finality["id"],"stage":"uncapped"}),
    )?;
    rows.push(basis.clone());
    let mut limits = vec![];
    for l in array(&policy["limits"])? {
        limits.push(json!({"binding_id":l["binding_id"],"premium":l["premium"],"discount_capacity":target.retail_basis()}));
    }
    let mut families = vec![];
    for f in array(&policy["families"])? {
        let limit = limits
            .iter()
            .find(|l| l["binding_id"] == f["binding_id"])
            .ok_or_else(b::integrity)?;
        let rules=array(&f["codes"])?.iter().map(|code|if code["amount"]["kind"]=="fixed" {json!({"code":code["code"],"kind":"fixed","fixed_atoms":code["amount"]["money"]["atoms"]})}else{json!({"code":code["code"],"kind":"percentage","rate":code["amount"]["rate"]})}).collect();
        let r = row(
            "policy-snapshot",
            &scope,
            json!({"agreement_id":s.agreement,"binding_id":s.binding,"book":"retail","roles":original_binding["roles"],"assent":assent["id"],"family_id":f["family"],"policy_version":policy["version"],"submission_source":f["source"],"correction_source":f["correction_source"],"evidence_required":f["evidence_required"],"ordinary":f["ordinary"],"corrections":f["corrections"],"allow_reversal":f["allow_reversal"],"replacement_codes":b::ordered(array(&f["replacement_codes"])?.clone())?,"rules":b::ordered(rules)?,"max_premium_atoms":limit["premium"]["atoms"],"max_discount_atoms":limit["discount_capacity"]["atoms"],"currency":"USD","scale":2,"basis_kind":"frozen_target_retail_net","rounding":"nearest_ties_away"}),
        )?;
        families.push(reference(&r));
        rows.push(r);
    }
    let snapshot = row(
        "target-snapshot",
        &scope,
        json!({"target":id,"accepted_at":received,"base_evaluation":reference(&eval_row),"bindings":b::ordered(vec![reference(&binding_row)])?,"families":b::ordered(families)?,"retail_basis":reference(&basis),"policy_utf8":string(&policy)?,"policy_document":doc(&policy_doc),"policy_document_hash":policy_doc["body"]["document_hash"],"verified_policy_document":doc(&policy_doc),"policy_evidence":[policy_doc["id"]],"rated_final":true,"finality_evidence":finality["id"],"verified_assents":[assent["id"]],"verified_offers":[],"verified_delegations":[],"limits":b::ordered(limits)?}),
    )?;
    rows.push(snapshot.clone());
    let members = b::members(&rows)?;
    let receipt = json!({"schema":"ledger-base-receipt/2-candidate.4","target":id,"base_evaluation":reference(&eval_row),"target_snapshot":reference(&snapshot),"accepted_at":received,"membership_hash":b::hash("base-membership",&json!(members))?});
    let acceptance = row(
        "base-acceptance",
        &scope,
        json!({"target":id,"base_evaluation":reference(&eval_row),"target_snapshot":reference(&snapshot),"accepted_at":received,"members":members,"original_receipt_utf8":string(&receipt)?}),
    )?;
    rows.push(acceptance.clone());
    let retained = Records::new(&rows.iter().map(bytes).collect::<Result<Vec<_>>>()?, scope)?;
    b::decode_base(&retained, &reference(&acceptance))?;
    Ok(rows)
}
fn identity_refs(v: &Value, out: &mut BTreeSet<(String, String)>) {
    match v {
        Value::Object(m) => {
            for (k, v) in m {
                if k != "extensions" {
                    identity_refs(v, out)
                }
            }
        }
        Value::Array(a) => {
            for v in a {
                identity_refs(v, out)
            }
        }
        Value::String(s) => {
            if let Some((p, h)) = s.split_once('_') {
                if h.len() == 64
                    && h.bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                {
                    let kind = match p {
                        "ev" => "event",
                        "ac" => "action",
                        "ef" => "effect",
                        "ob" => "obligation",
                        "cl" => "claim",
                        "lk" => "link",
                        "doc" => "document",
                        _ => return,
                    };
                    out.insert((kind.into(), s.clone()));
                }
            }
        }
        _ => (),
    }
}

/// Only this coordinator constructs a complete, verified economic append.
pub(crate) struct ValidatedEntry {
    count: i64,
    alias: Option<i64>,
    customer: Option<String>,
    source: String,
    external: String,
    accepted_at_us: Option<i64>,
    agreement_id: Option<String>,
    agreement_version: Option<i64>,
    semantic: Vec<u8>,
    ingress: Vec<u8>,
    facts: Vec<u8>,
    bundle: Vec<u8>,
}
impl ValidatedEntry {
    pub(crate) fn alias(&self) -> Option<i64> {
        self.alias
    }
    pub(crate) fn expected_count(&self) -> i64 {
        self.count
    }
    pub(crate) fn source(&self) -> &str {
        &self.source
    }
    pub(crate) fn customer(&self) -> Option<&str> {
        self.customer.as_deref()
    }
    pub(crate) fn external_id(&self) -> &str {
        &self.external
    }
    pub(crate) fn semantic_key(&self) -> &[u8] {
        &self.semantic
    }
    pub(crate) fn ingress(&self) -> &[u8] {
        &self.ingress
    }
    pub(crate) fn facts(&self) -> &[u8] {
        &self.facts
    }
    pub(crate) fn bundle(&self) -> &[u8] {
        &self.bundle
    }
    pub(crate) fn byte_len(&self) -> usize {
        self.semantic.len() + self.ingress.len() + self.facts.len() + self.bundle.len()
    }
    pub(crate) fn accepted_at_us(&self) -> Option<i64> {
        self.accepted_at_us
    }
    pub(crate) fn agreement_id(&self) -> Option<&str> {
        self.agreement_id.as_deref()
    }
    pub(crate) fn agreement_version(&self) -> Option<i64> {
        self.agreement_version
    }
}
fn decode_entry(s: &Setup, e: &crate::store::sqlite::BillingEntry) -> Result<Records> {
    let v = core(canonical::parse_bounded(&e.bundle, 8 * 1024 * 1024))?;
    b::check(bytes(&v)? == e.bundle)?;
    let records = Records::new(
        &array(&v)?.iter().map(bytes).collect::<Result<Vec<_>>>()?,
        json!(s.scope),
    )?;
    let base = b::decode_base(&records, &reference(records.one("base-acceptance")?))?;
    let event = base.evaluation.event();
    b::check(
        event.source() == e.source
            && event.candidate().external_id() == e.external_id
            && event.candidate().ingress_bytes().as_slice() == e.ingress
            && bytes(&event.completion_facts(&[]).map_err(|_| b::integrity())?)? == e.facts
            && bytes(&json!(["base", event.candidate().operation_id()]))? == e.semantic_key
            && event.dto().customer == s.customer
            && event.scope() == &s.scope
            && base.evaluation.bundle().policies().len() == 1
            && base.evaluation.bundle().policies()[0].binding.agreement == s.agreement,
    )?;
    Ok(records)
}
fn receipt(records: &Records) -> Result<Value> {
    Ok(records.one("base-acceptance")?.clone())
}
type Prepared = (Value, Option<ValidatedEntry>);
struct DuplicateIdentity<'a> {
    legacy_customer: &'a str,
    customer: &'a str,
    source: &'a str,
    external: &'a str,
    ingress: &'a [u8],
    facts: &'a [u8],
    semantic: &'a [u8],
}
fn duplicate(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    audit: &history::Audit,
    identity: DuplicateIdentity<'_>,
) -> Result<Option<Prepared>> {
    // Identity takes precedence over semantic lookup across the entire snapshot.
    for e in &snapshot.entries {
        let entry_customer = e.customer.as_deref().unwrap_or(identity.legacy_customer);
        if entry_customer == identity.customer
            && e.source == identity.source
            && e.external_id == identity.external
        {
            require(e.ingress == identity.ingress, "IDENTITY_CONFLICT")?;
            return Ok(Some((
                json!({"status":"duplicate","kind":"identity","receipt":audit.receipts[&e.ordinal]}),
                None,
            )));
        }
    }
    for a in &snapshot.aliases {
        let alias_customer = a.customer.as_deref().unwrap_or(identity.legacy_customer);
        if alias_customer == identity.customer
            && a.source == identity.source
            && a.external_id == identity.external
        {
            require(a.ingress == identity.ingress, "IDENTITY_CONFLICT")?;
            return Ok(Some((
                json!({"status":"duplicate","kind":"identity","receipt":audit.receipts[&a.ordinal]}),
                None,
            )));
        }
    }
    for e in &snapshot.entries {
        let entry_customer = e.customer.as_deref().unwrap_or(identity.legacy_customer);
        if entry_customer == identity.customer
            && e.source == identity.source
            && e.semantic_key == identity.semantic
        {
            require(e.facts == identity.facts, "SEMANTIC_CONFLICT")?;
            require(snapshot.aliases.len() < 1000, "BILLING_HISTORY_LIMIT")?;
            let plan = ValidatedEntry {
                count: snapshot.entries.len() as i64,
                alias: Some(e.ordinal),
                customer: Some(identity.customer.into()),
                source: identity.source.into(),
                external: identity.external.into(),
                accepted_at_us: None,
                agreement_id: None,
                agreement_version: None,
                ingress: identity.ingress.into(),
                facts: vec![],
                semantic: vec![],
                bundle: vec![],
            };
            return Ok(Some((
                json!({"status":"duplicate","kind":"semantic","receipt":audit.receipts[&e.ordinal]}),
                Some(plan),
            )));
        }
    }
    Ok(None)
}

pub(crate) fn prepare(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
    source: &str,
    raw: &[u8],
    at: &Timestamp,
) -> Result<(Value, Option<ValidatedEntry>)> {
    require(
        !customer.is_empty()
            && customer.len() <= 128
            && !customer.chars().any(char::is_control)
            && !source.is_empty()
            && source.len() <= 256
            && !source.chars().any(char::is_control),
        "BILLING_IDENTIFIER",
    )?;
    let authority = permissions::effective_for(snapshot, customer, source)?;
    let scope = control::customer_scope(snapshot, customer)?;
    b::check(authority.scope == scope)?;
    require(
        authority.permissions.iter().any(|p| p == "read"),
        "BILLING_UNAUTHORIZED",
    )?;
    let event = domain::normalize(raw, scope.clone(), source)
        .map_err(|e| reject(e.code))?
        .resolve(None)
        .map_err(|e| reject(e.code))?;
    require(
        event.source() == source && event.dto().customer == customer,
        "BILLING_SCOPE",
    )?;
    let ingress = event.candidate().ingress_bytes().as_slice().to_vec();
    let semantic = bytes(&json!(["base", event.candidate().operation_id()]))?;
    let facts = bytes(&event.completion_facts(&[]).map_err(|e| reject(e.code))?)?;
    let initial = Setup::parse(&snapshot.setup)?;
    let audit = history::load(&initial, snapshot)?;
    if let Some(result) = duplicate(
        snapshot,
        &audit,
        DuplicateIdentity {
            legacy_customer: &initial.customer,
            customer,
            source: event.source(),
            external: event.candidate().external_id(),
            ingress: &ingress,
            facts: &facts,
            semantic: &semantic,
        },
    )? {
        return Ok(result);
    }
    require(
        authority.permissions.iter().any(|p| p == "submit"),
        "BILLING_UNAUTHORIZED",
    )?;
    require(snapshot.entries.len() < 1000, "BILLING_HISTORY_LIMIT")?;
    control::ensure_new_ledger_time(snapshot, &audit, at)?;
    let (mut terms, agreement_version) = control::selected_terms(snapshot, customer, source, at)?
        .ok_or_else(|| reject("BILLING_TERMS_NOT_ACTIVE"))?;
    terms.permissions = authority.permissions.clone();
    terms.grant_revision = authority.grant_revision;
    let agreement_id = terms.agreement.clone();
    let rows = base(&terms, raw, at)?;
    let bundle = bytes(&json!(rows))?;
    require(bundle.len() <= 8 * 1024 * 1024, "BILLING_HISTORY_LIMIT")?;
    let records = Records::new(
        &rows.iter().map(bytes).collect::<Result<Vec<_>>>()?,
        json!(scope),
    )?;
    let result = json!({"status":"accepted","receipt":receipt(&records)?});
    Ok((
        result,
        Some(ValidatedEntry {
            alias: None,
            count: snapshot.entries.len() as i64,
            customer: Some(customer.into()),
            source: event.source().into(),
            external: event.candidate().external_id().into(),
            accepted_at_us: Some(at.micros()),
            agreement_id: Some(agreement_id),
            agreement_version: Some(agreement_version),
            semantic,
            ingress,
            facts,
            bundle,
        }),
    ))
}
pub(crate) fn statement(
    snapshot: &crate::store::sqlite::BillingSnapshot,
    customer: &str,
    target: Option<&str>,
) -> Result<Value> {
    let initial = Setup::parse(&snapshot.setup)?;
    let scope = control::customer_scope(snapshot, customer)?;
    let sources = snapshot
        .agreements
        .iter()
        .filter(|row| row.customer == customer)
        .map(|row| row.source.as_str())
        .collect::<BTreeSet<_>>();
    require(!sources.is_empty(), "BILLING_SCOPE")?;
    let mut entries = vec![];
    let mut net = 0i128;
    let mut roots = vec![];
    let audit = history::load(&initial, snapshot)?;
    let customer_entries = snapshot
        .entries
        .iter()
        .filter(|entry| entry.customer.as_deref().unwrap_or(&initial.customer) == customer)
        .collect::<Vec<_>>();
    let target_entry = target
        .map(|target| {
            customer_entries
                .iter()
                .copied()
                .find(|entry| {
                    let receipt = &audit.receipts[&entry.ordinal];
                    let rows = &audit.rows[&entry.ordinal];
                    let entry_target = if receipt["kind"] == "base-acceptance" {
                        receipt["body"]["target"].as_str()
                    } else {
                        rows.iter()
                            .find(|row| row["kind"] == "event")
                            .and_then(|row| row["body"]["data"]["target"].as_str())
                    };
                    entry_target == Some(target) || receipt["id"] == target
                })
                .ok_or_else(|| reject("BILLING_NOT_FOUND"))
        })
        .transpose()?;
    if let Some(entry) = target_entry {
        let authority = permissions::effective_for(snapshot, customer, &entry.source)?;
        // Missing targets and targets outside the caller's read authority use
        // the same response so an identifier cannot probe another source.
        if !authority
            .permissions
            .iter()
            .any(|permission| permission == "read")
        {
            return Err(reject("BILLING_NOT_FOUND"));
        }
    } else {
        for source in sources {
            let authority = permissions::effective_for(snapshot, customer, source)?;
            require(
                authority
                    .permissions
                    .iter()
                    .any(|permission| permission == "read"),
                "BILLING_UNAUTHORIZED",
            )?;
        }
    }
    // A targeted explanation contains that target's full history, limited to
    // its authorized source. This keeps its agreements, cutoff, roots, and hash
    // inside that source's read authority instead of inheriting customer-wide
    // metadata.
    let visible_entries = if let Some(entry) = target_entry {
        let target = target.expect("target entry is resolved only for targeted explain");
        customer_entries
            .iter()
            .copied()
            .filter(|candidate| {
                if candidate.source != entry.source {
                    return false;
                }
                let receipt = &audit.receipts[&candidate.ordinal];
                let rows = &audit.rows[&candidate.ordinal];
                let entry_target = if receipt["kind"] == "base-acceptance" {
                    receipt["body"]["target"].as_str()
                } else {
                    rows.iter()
                        .find(|row| row["kind"] == "event")
                        .and_then(|row| row["body"]["data"]["target"].as_str())
                };
                entry_target == Some(target) || receipt["id"] == target
            })
            .collect::<Vec<_>>()
    } else {
        customer_entries
    };
    let mut represented_agreements = BTreeSet::new();
    for (local_ordinal, e) in visible_entries.iter().enumerate() {
        let rows = &audit.rows[&e.ordinal];
        let accepted = &audit.receipts[&e.ordinal];
        let terms = &audit.entry_terms[&e.ordinal];
        let agreement_version = e.agreement_version.unwrap_or(1);
        represented_agreements.insert((
            e.source.clone(),
            terms.agreement.clone(),
            agreement_version,
        ));
        let entry_target = if accepted["kind"] == "base-acceptance" {
            accepted["body"]["target"].clone()
        } else {
            rows.iter()
                .find(|r| r["kind"] == "event")
                .ok_or_else(b::integrity)?["body"]["data"]["target"]
                .clone()
        };
        roots.push(reference(accepted));
        if target.is_some_and(|id| entry_target != id && accepted["id"] != id) {
            continue;
        }
        let mut total = 0i128;
        let postings = rows
            .iter()
            .filter(|r| r["kind"] == "base-posting" || r["kind"] == "action")
            .cloned()
            .collect::<Vec<_>>();
        for p in &postings {
            // A statement spans independent accepted decisions. Their exact
            // aggregate may exceed the per-record Money bound without making
            // any retained posting invalid. Keep checked wide integer totals.
            total = total
                .checked_add(b::money(&p["body"]["amount"])?.atoms())
                .ok_or_else(b::integrity)?;
        }
        net = net.checked_add(total).ok_or_else(b::integrity)?;
        entries.push(json!({"ordinal":(local_ordinal + 1).to_string(),"source":e.source,"external_id":e.external_id,"agreement_id":terms.agreement,"agreement_version":agreement_version.to_string(),"target":entry_target,"receipt":accepted,"postings":postings,"net_atoms":total.to_string(),"records":rows}));
    }
    require(target.is_none() || !entries.is_empty(), "BILLING_NOT_FOUND")?;
    let agreements = represented_agreements
        .into_iter()
        .map(|(source, agreement_id, version)| {
            json!({"source":source,"agreement_id":agreement_id,"version":version.to_string()})
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "schema":"ledger-billing-statement/2",
        "status":"ok",
        "customer":customer,
        "scope":scope,
        "agreements":agreements,
        "currency":"USD",
        "scale":2,
        "net_atoms":net.to_string(),
        "complete":true,
        "cutoff":visible_entries.len().to_string(),
        "snapshot_hash":core(canonical::digest(Domain::Document,&json!(["billing-statement",2,customer,visible_entries.len().to_string(),roots])))?,
        "entries":entries,
        "kind":"billing_statement",
        "payment_collected":false
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_snapshot(setup: &Setup) -> crate::store::sqlite::BillingSnapshot {
        let accepted = setup.accepted_at.micros();
        crate::store::sqlite::BillingSnapshot {
            permissions: vec![],
            scoped_permissions: vec![],
            customers: vec![crate::store::sqlite::BillingCustomer {
                customer: setup.customer.clone(),
                tenant: setup.scope.tenant().to_owned(),
                environment: setup.scope.environment().to_owned(),
            }],
            agreements: vec![crate::store::sqlite::BillingAgreement {
                customer: setup.customer.clone(),
                source: setup.source.clone(),
                revision: 1,
                transition: "start".into(),
                agreement_id: setup.agreement.clone(),
                agreement_version: 1,
                effective_at_us: accepted,
                recorded_at_us: accepted,
                setup: Some(control::canonical_bytes(setup).unwrap()),
            }],
            controls: vec![],
            aliases: vec![],
            setup: control::canonical_bytes(setup).unwrap(),
            entries: vec![],
        }
    }

    #[test]
    fn configured_retail_base_replays_without_supplier_invocation() {
        let setup =
            Setup::parse(include_bytes!("../../../../examples/billing/setup.json")).unwrap();
        let rows = base(
            &setup,
            include_bytes!("../../../../examples/billing/event.json"),
            &Timestamp::parse("2026-09-23T12:01:00Z").unwrap(),
        )
        .unwrap();
        let postings = rows
            .iter()
            .filter(|r| r["kind"] == "base-posting")
            .collect::<Vec<_>>();
        assert_eq!(postings.len(), 1);
        assert_eq!(postings[0]["body"]["amount"]["atoms"], "250");
        assert!(rows
            .iter()
            .all(|r| !r["kind"].as_str().unwrap().starts_with("reservation-")));
    }

    #[test]
    fn agreement_selection_uses_inclusive_ledger_effective_boundaries() {
        let setup =
            Setup::parse(include_bytes!("../../../../examples/billing/setup.json")).unwrap();
        let mut snapshot = empty_snapshot(&setup);
        let cutover = Timestamp::parse("2026-09-10T00:00:00.000000Z").unwrap();
        let mut amended = setup.clone();
        amended.price = "7.50".into();
        snapshot
            .agreements
            .push(crate::store::sqlite::BillingAgreement {
                customer: setup.customer.clone(),
                source: setup.source.clone(),
                revision: 2,
                transition: "amend".into(),
                agreement_id: setup.agreement.clone(),
                agreement_version: 2,
                effective_at_us: cutover.micros(),
                recorded_at_us: cutover.micros(),
                setup: Some(control::canonical_bytes(&amended).unwrap()),
            });
        let end = Timestamp::parse("2026-09-15T00:00:00.000000Z").unwrap();
        snapshot
            .agreements
            .push(crate::store::sqlite::BillingAgreement {
                customer: setup.customer.clone(),
                source: setup.source.clone(),
                revision: 3,
                transition: "end".into(),
                agreement_id: setup.agreement.clone(),
                agreement_version: 2,
                effective_at_us: end.micros(),
                recorded_at_us: end.micros(),
                setup: None,
            });

        let before = Timestamp::parse("2026-09-09T23:59:59.999999Z").unwrap();
        assert_eq!(
            control::selected_terms(&snapshot, &setup.customer, &setup.source, &before)
                .unwrap()
                .unwrap()
                .0
                .price,
            "2.50"
        );
        assert_eq!(
            control::selected_terms(&snapshot, &setup.customer, &setup.source, &cutover)
                .unwrap()
                .unwrap()
                .0
                .price,
            "7.50"
        );
        assert!(
            control::selected_terms(&snapshot, &setup.customer, &setup.source, &end)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn new_control_time_must_advance_past_the_latest_accepted_work() {
        let setup =
            Setup::parse(include_bytes!("../../../../examples/billing/setup.json")).unwrap();
        let snapshot = empty_snapshot(&setup);
        let mut audit = history::Audit {
            targets: BTreeMap::new(),
            receipts: BTreeMap::new(),
            rows: BTreeMap::new(),
            entry_terms: BTreeMap::new(),
        };
        let accepted_at = Timestamp::parse("2026-09-20T12:00:00.000000Z").unwrap();
        audit
            .receipts
            .insert(1, json!({"body":{"accepted_at":accepted_at}}));
        assert!(control::ensure_new_ledger_time(&snapshot, &audit, &accepted_at).is_err());
        let advanced = Timestamp::parse("2026-09-20T12:00:00.000001Z").unwrap();
        assert!(control::ensure_new_ledger_time(&snapshot, &audit, &advanced).is_ok());
    }

    #[test]
    fn agreement_commands_require_the_current_agreement_and_a_future_boundary() {
        let setup =
            Setup::parse(include_bytes!("../../../../examples/billing/setup.json")).unwrap();
        let snapshot = empty_snapshot(&setup);
        let mut amended = setup.clone();
        amended.price = "4.25".into();
        let recorded_at = Timestamp::parse("2026-09-05T00:00:00.000000Z").unwrap();
        let request = json!({
            "schema":"ledger-billing-amendment/2",
            "customer":setup.customer,
            "source":setup.source,
            "change_id":"amend-price",
            "expected_revision":"1",
            "effective_at":"2026-09-06T00:00:00.000000Z",
            "setup":amended
        });
        let encoded = control::canonical_bytes(&request).unwrap();
        let (_, plan) = control::prepare(
            &snapshot,
            &encoded,
            &recorded_at,
            &setup.customer,
            &setup.source,
        )
        .unwrap();
        assert_eq!(plan.unwrap().agreement_version, 2);

        let ending = json!({
            "schema":"ledger-billing-ending/2",
            "customer":setup.customer,
            "source":setup.source,
            "agreement":"different-agreement",
            "change_id":"end-wrong-agreement",
            "expected_revision":"1",
            "effective_at":"2026-09-06T00:00:00.000000Z",
            "reason":"End request for the wrong agreement must refuse"
        });
        assert!(control::prepare(
            &snapshot,
            &control::canonical_bytes(&ending).unwrap(),
            &recorded_at,
            &setup.customer,
            &setup.source,
        )
        .is_err());
    }
}
