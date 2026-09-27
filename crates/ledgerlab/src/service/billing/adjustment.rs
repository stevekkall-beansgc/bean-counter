use super::*;
use crate::store::sqlite::{BillingEntry, BillingSnapshot};
use ledgerlab_core::canonical::outcome as codec;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Adjustment {
    schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    customer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    id: String,
    target: String,
    family: String,
    occurred_at: Timestamp,
    evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    replacement: Option<Replacement>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
enum Replacement {
    Code { code: String },
    Reverse,
}
impl Adjustment {
    fn parse(raw: &[u8], correction: bool, customer: &str, source: &str) -> Result<Self> {
        let v = canonical::parse_bounded(raw, 262144).map_err(|_| reject("BILLING_EVENT"))?;
        let a: Self = serde_json::from_value(v).map_err(|_| reject("BILLING_EVENT"))?;
        let expected_schema = if correction {
            ["ledger-billing-correction/1", "ledger-billing-correction/2"]
        } else {
            ["ledger-billing-outcome/1", "ledger-billing-outcome/2"]
        };
        require(
            expected_schema.contains(&a.schema.as_str()),
            "BILLING_EVENT",
        )?;
        if a.schema.ends_with("/2") {
            require(
                a.customer.as_deref() == Some(customer) && a.source.as_deref() == Some(source),
                "BILLING_SCOPE",
            )?;
        } else {
            require(a.customer.is_none() && a.source.is_none(), "BILLING_EVENT")?;
        }
        require(
            !a.id.is_empty() && a.id.len() <= 128 && !a.id.chars().any(char::is_control),
            "BILLING_IDENTIFIER",
        )?;
        require(
            !a.evidence.trim().is_empty() && a.evidence.len() <= 8192,
            "BILLING_EVIDENCE_REQUIRED",
        )?;
        require(
            if correction {
                a.code.is_none() && a.expected_revision.is_some() && a.replacement.is_some()
            } else {
                a.code.is_some() && a.expected_revision.is_none() && a.replacement.is_none()
            },
            "BILLING_EVENT",
        )?;
        if let Some(n) = &a.expected_revision {
            core(Revision::parse(n))?;
        }
        Ok(a)
    }
    fn correction(&self) -> bool {
        self.expected_revision.is_some()
    }
    fn is_m2(&self) -> bool {
        self.schema.ends_with("/2")
    }
    fn ingress(&self) -> Result<Vec<u8>> {
        bytes(&json!(self))
    }
    fn facts(&self) -> Result<Vec<u8>> {
        let mut v = json!(self);
        v.as_object_mut().unwrap().remove("id");
        bytes(&v)
    }
    fn semantic(&self) -> Result<Vec<u8>> {
        bytes(&if self.correction() {
            json!(["correction", self.id])
        } else {
            json!(["outcome", self.target, self.family])
        })
    }
    fn evidence(&self, s: &Setup) -> Result<Value> {
        evidence(
            &json!(s.scope),
            if self.correction() {
                "correction"
            } else {
                "outcome"
            },
            if self.correction() {
                "correction"
            } else {
                "outcome"
            },
            json!({"scope":s.scope,"target":self.target,"family":self.family,"source":s.source,"occurred_at":self.occurred_at,"retained_evidence":self.evidence}),
        )
    }
}
pub(super) fn verify_alias(
    a: &crate::store::sqlite::BillingAlias,
    e: &BillingEntry,
    legacy_customer: &str,
) -> Result<()> {
    let customer = e.customer.as_deref().unwrap_or(legacy_customer);
    let a_input = Adjustment::parse(&a.ingress, false, customer, &e.source)?;
    b::check(
        a.customer.as_deref().unwrap_or(legacy_customer) == customer
            && a.source == e.source
            && a_input.id == a.external_id
            && a_input.ingress()? == a.ingress
            && a_input.facts()? == e.facts
            && a_input.semantic()? == e.semantic_key,
    )
}
pub(super) fn verify_entry(s: &Setup, e: &BillingEntry, records: &Records) -> Result<()> {
    let event = records.one("event")?;
    let data = &event["body"]["data"];
    let a = Adjustment::parse(
        &e.ingress,
        data["type"] == "correction",
        &s.customer,
        &s.source,
    )?;
    b::check(
        a.ingress()? == e.ingress
            && a.facts()? == e.facts
            && a.semantic()? == e.semantic_key
            && a.is_m2() == e.customer.is_some()
            && e.source == s.source
            && e.external_id == a.id
            && data["source"] == s.source
            && data["external_id"] == a.id
            && data["target"] == a.target
            && data["family_id"] == a.family
            && data["agreement_id"] == s.agreement
            && data["occurred_at"] == json!(a.occurred_at),
    )?;
    let proof = a.evidence(s)?;
    b::check(data["evidence"] == json!([proof["id"]]))?;
    if a.correction() {
        b::check(
            data["expected_revision_number"] == json!(a.expected_revision)
                && data["replacement"] == json!(a.replacement),
        )?;
    } else {
        b::check(data["code"] == json!(a.code))?;
    }
    let authority = &records.one("authority-decision")?["body"];
    b::check(
        authority["principal"] == s.operator
            && authority["source"] == s.source
            && authority["active"] == true
            && authority["may_read"] == true,
    )?;
    Ok(())
}
/// A parsed outcome or correction awaiting its retained dedup lookups and its
/// target history, both selected by the durable index.
pub(crate) struct Submission<'a> {
    adjustment: Adjustment,
    ingress: Vec<u8>,
    facts: Vec<u8>,
    semantic: Vec<u8>,
    initial: Setup,
    authority: Setup,
    customer: &'a str,
    source: &'a str,
    at: &'a Timestamp,
    correction: bool,
}
impl<'a> Submission<'a> {
    pub(crate) fn customer(&self) -> &'a str {
        self.customer
    }
    pub(crate) fn source(&self) -> &'a str {
        self.source
    }
    pub(crate) fn external_id(&self) -> &str {
        &self.adjustment.id
    }
    pub(crate) fn semantic_key(&self) -> &[u8] {
        &self.semantic
    }
    /// The target this decision adjusts, used to select its retained history.
    pub(crate) fn target(&self) -> &str {
        &self.adjustment.target
    }
}
/// Parse and scope one outcome or correction. Authority, schema and identifier
/// checks happen here, in the original order, before any retained lookup.
pub(crate) fn begin<'a>(
    snapshot: &BillingSnapshot,
    customer: &'a str,
    source: &'a str,
    raw: &[u8],
    at: &'a Timestamp,
    correction: bool,
) -> Result<Submission<'a>> {
    let authority = permissions::effective_for(snapshot, customer, source)?;
    let scope = control::customer_scope(snapshot, customer)?;
    b::check(authority.scope == scope)?;
    require(
        authority.permissions.iter().any(|p| p == "read"),
        "BILLING_UNAUTHORIZED",
    )?;
    let adjustment = Adjustment::parse(raw, correction, customer, source)?;
    let ingress = adjustment.ingress()?;
    let facts = adjustment.facts()?;
    let semantic = adjustment.semantic()?;
    let initial = Setup::parse(&snapshot.setup)?;
    Ok(Submission {
        adjustment,
        ingress,
        facts,
        semantic,
        initial,
        authority,
        customer,
        source,
        at,
        correction,
    })
}
pub(crate) fn finish(
    snapshot: &BillingSnapshot,
    submission: &Submission<'_>,
    dedup: &super::Dedup,
    target_entries: &[BillingEntry],
) -> Result<(Value, Option<ValidatedEntry>)> {
    let Submission {
        adjustment: a,
        ingress,
        facts,
        semantic,
        initial,
        authority: s,
        customer,
        source,
        at,
        correction,
    } = submission;
    let (customer, source) = (*customer, *source);
    if let Some(result) = super::resolve_duplicate(
        snapshot,
        dedup,
        &super::Incoming {
            initial,
            customer,
            source,
            external: &a.id,
            ingress,
            facts,
        },
    )? {
        return Ok(result);
    }
    require(a.is_m2(), "BILLING_EVENT")?;
    require(
        s.permissions
            .iter()
            .any(|p| p == if *correction { "correct" } else { "submit" }),
        "BILLING_UNAUTHORIZED",
    )?;
    require(
        snapshot.entry_count < super::MAX_RETAINED_ENTRIES,
        "BILLING_HISTORY_LIMIT",
    )?;
    control::ensure_new_ledger_time(snapshot, snapshot.ledger_time_max, at)?;
    // Only this target's retained decisions are decoded, in ordinal order.
    let mut audit = history::load_target(initial, snapshot, target_entries)?;
    let history = audit
        .targets
        .get_mut(&history::target_key(customer, &a.target))
        .ok_or_else(|| reject("BILLING_NOT_FOUND"))?;
    require(history.source == source, "BILLING_NOT_FOUND")?;
    let original = history.terms.clone();
    let agreement_id = history.agreement_id.clone();
    let agreement_version = history.agreement_version;
    let base = &history.base;
    let mut additions = vec![];
    let proof = a.evidence(&original)?;
    if !history.records.rows.iter().any(|r| r["id"] == proof["id"]) {
        history.records.insert(proof.clone())?;
        additions.push(proof.clone());
    }
    let authentication = evidence(
        &json!(original.scope),
        "authentication",
        "authentication",
        json!({"method":"local_private_filesystem","principal":s.operator,"source":source,"external_id":a.id,"observed_at":at}),
    )?;
    history.records.insert(authentication.clone())?;
    additions.push(authentication.clone());
    let grant = evidence(
        &json!(original.scope),
        "grant",
        "grant",
        json!({"scope":original.scope,"principal":s.operator,"source":source,"permissions":s.permissions,"revision":s.grant_revision.to_string(),"active":true}),
    )?;
    if !history.records.rows.iter().any(|r| r["id"] == grant["id"]) {
        history.records.insert(grant.clone())?;
        additions.push(grant.clone());
    }
    let mut data = json!({"type":if *correction{"correction"}else{"outcome"},"source":source,"external_id":a.id,"target":a.target,"chain_id":base.evaluation.event().chain(),"agreement_id":agreement_id,"family_id":a.family,"occurred_at":a.occurred_at,"evidence":[proof["id"]]});
    if *correction {
        // Family identity is on the claim row; revision lookup is constrained by
        // the exact permanent claim identity, never an arbitrary latest row.
        let claim = core(codec::key(
            codec::ECONOMIC,
            "claim",
            &json!(original.scope),
            &json!({"agreement_id":agreement_id,"family_id":a.family,"target":a.target}),
        ))?;
        let old = history
            .records
            .rows
            .iter()
            .filter(|r| r["kind"] == "claim-revision" && r["body"]["claim_id"] == claim)
            .max_by_key(|r| {
                r["body"]["number"]
                    .as_str()
                    .and_then(|n| n.parse::<u64>().ok())
                    .unwrap_or(0)
            })
            .ok_or_else(|| reject("BILLING_NOT_FOUND"))?;
        require(
            old["body"]["number"] == json!(a.expected_revision),
            "STALE_REVISION",
        )?;
        data["claim_id"] = claim;
        data["expected_revision"] = old["id"].clone();
        data["expected_revision_number"] = json!(a.expected_revision);
        data["replacement"] = json!(a.replacement);
    } else {
        data["code"] = json!(a.code);
    }
    let event_body = json!({"schema":"ledger-event/2-candidate.4","data":data});
    let eid = core(codec::key(
        codec::ECONOMIC,
        "event",
        &json!(original.scope),
        &event_body,
    ))?;
    let authority = json!({"event_id":eid,"target":a.target,"agreement_id":agreement_id,"family_id":a.family,"source":source,"principal":s.operator,"grant":grant["id"],"grant_revision":s.grant_revision.to_string(),"active":true,"may_read":true,"may_submit":!correction,"may_correct":correction,"verified_evidence":[proof["id"]],"received_at":at,"accepted_at":at});
    let request = b::request(&history.records, base, &data)?;
    let verified = b::verified(&history.records, &request, &authority)?;
    let decision = match c::outcomes::evaluate(
        &request,
        &verified,
        std::slice::from_ref(&base.target),
        std::slice::from_ref(&base.evaluation),
        &history.decisions,
    )
    .map_err(|e| reject(e.code))?
    {
        c::outcomes::Submission::Accepted(d) => d,
        _ => return Err(b::integrity()),
    };
    let policy = history
        .records
        .rows
        .iter()
        .find(|r| r["kind"] == "policy-snapshot" && r["body"]["family_id"] == a.family)
        .ok_or_else(b::integrity)?;
    let auth = row(
        "authority-decision",
        &json!(original.scope),
        authority.clone(),
    )?;
    let admission = json!({"event_id":eid,"principal":s.operator,"credential_revision":"1","authentication":authentication["id"],"grant":grant["id"],"grant_revision":s.grant_revision.to_string(),"target_guard_revision":"1","target_state":"final_unreversed","aggregate_guard_revision":history.decisions.len().to_string(),"authorized_source":source,"agreement_id":agreement_id,"payer":customer,"book":"retail","family_id":a.family,"permission":data["type"],"target_snapshot":base.snapshot["id"],"authority_decision":auth["id"],"binding_id":original.binding,"received_at":at,"accepted_at":at,"decision":"allow","policy_snapshot":policy["id"],"basis":base.basis["id"]});
    let rows = super::super::retained::economic::project(
        &history.records,
        base,
        event_body,
        authority,
        admission,
        &decision,
        additions,
    )?;
    let receipt = rows
        .iter()
        .find(|r| r["kind"] == "receipt")
        .ok_or_else(b::integrity)?
        .clone();
    let bundle = bytes(&json!(rows))?;
    require(bundle.len() <= 8 * 1024 * 1024, "BILLING_HISTORY_LIMIT")?;
    Ok((
        json!({"status":"accepted","receipt":receipt}),
        Some(ValidatedEntry {
            alias: None,
            count: snapshot.entry_count,
            customer: Some(customer.into()),
            source: source.into(),
            external: a.id.clone(),
            target: Some(a.target.clone()),
            kind: Some(if *correction { "correction" } else { "outcome" }),
            accepted_at_us: Some(at.micros()),
            agreement_id: Some(agreement_id),
            agreement_version: Some(agreement_version),
            semantic: semantic.clone(),
            ingress: ingress.clone(),
            facts: facts.clone(),
            bundle,
        }),
    ))
}
