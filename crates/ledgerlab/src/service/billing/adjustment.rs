use super::*;
use crate::store::sqlite::{BillingEntry, BillingSnapshot};
use ledgerlab_core::canonical::outcome as codec;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Adjustment {
    schema: String,
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
    fn parse(raw: &[u8], correction: bool) -> Result<Self> {
        let v = canonical::parse_bounded(raw, 262144).map_err(|_| reject("BILLING_EVENT"))?;
        let a: Self = serde_json::from_value(v).map_err(|_| reject("BILLING_EVENT"))?;
        require(
            a.schema
                == if correction {
                    "ledger-billing-correction/1"
                } else {
                    "ledger-billing-outcome/1"
                },
            "BILLING_EVENT",
        )?;
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
pub(super) fn verify_alias(a: &crate::store::sqlite::BillingAlias, e: &BillingEntry) -> Result<()> {
    let a_input = Adjustment::parse(&a.ingress, false)?;
    b::check(
        a_input.id == a.external_id
            && a_input.ingress()? == a.ingress
            && a_input.facts()? == e.facts
            && a_input.semantic()? == e.semantic_key,
    )
}
pub(super) fn verify_entry(s: &Setup, e: &BillingEntry, records: &Records) -> Result<()> {
    let event = records.one("event")?;
    let data = &event["body"]["data"];
    let a = Adjustment::parse(&e.ingress, data["type"] == "correction")?;
    b::check(
        a.ingress()? == e.ingress
            && a.facts()? == e.facts
            && a.semantic()? == e.semantic_key
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
pub(crate) fn prepare(
    snapshot: &BillingSnapshot,
    raw: &[u8],
    at: &Timestamp,
    correction: bool,
) -> Result<(Value, Option<ValidatedEntry>)> {
    let s = permissions::effective(snapshot)?;
    require(
        s.permissions.iter().any(|p| p == "read"),
        "BILLING_UNAUTHORIZED",
    )?;
    let a = Adjustment::parse(raw, correction)?;
    let ingress = a.ingress()?;
    let facts = a.facts()?;
    let semantic = a.semantic()?;
    let mut audit = history::load(&s, snapshot)?;
    if let Some(result) = duplicate(
        snapshot, &audit, &s.source, &a.id, &ingress, &facts, &semantic,
    )? {
        return Ok(result);
    }
    require(
        s.permissions
            .iter()
            .any(|p| p == if correction { "correct" } else { "submit" }),
        "BILLING_UNAUTHORIZED",
    )?;
    require(snapshot.entries.len() < 1000, "BILLING_HISTORY_LIMIT")?;
    let history = audit
        .targets
        .get_mut(&a.target)
        .ok_or_else(|| reject("BILLING_NOT_FOUND"))?;
    let base = &history.base;
    let mut additions = vec![];
    let proof = a.evidence(&s)?;
    if !history.records.rows.iter().any(|r| r["id"] == proof["id"]) {
        history.records.insert(proof.clone())?;
        additions.push(proof.clone());
    }
    let authentication = evidence(
        &json!(s.scope),
        "authentication",
        "authentication",
        json!({"method":"local_private_filesystem","principal":s.operator,"source":s.source,"external_id":a.id,"observed_at":at}),
    )?;
    history.records.insert(authentication.clone())?;
    additions.push(authentication.clone());
    let grant = evidence(
        &json!(s.scope),
        "grant",
        "grant",
        json!({"scope":s.scope,"principal":s.operator,"source":s.source,"permissions":s.permissions,"revision":s.grant_revision.to_string(),"active":true}),
    )?;
    if !history.records.rows.iter().any(|r| r["id"] == grant["id"]) {
        history.records.insert(grant.clone())?;
        additions.push(grant.clone());
    }
    let mut data = json!({"type":if correction{"correction"}else{"outcome"},"source":s.source,"external_id":a.id,"target":a.target,"chain_id":base.evaluation.event().chain(),"agreement_id":s.agreement,"family_id":a.family,"occurred_at":a.occurred_at,"evidence":[proof["id"]]});
    if correction {
        // Family identity is on the claim row; revision lookup is constrained by
        // the exact permanent claim identity, never an arbitrary latest row.
        let claim = core(codec::key(
            codec::ECONOMIC,
            "claim",
            &json!(s.scope),
            &json!({"agreement_id":s.agreement,"family_id":a.family,"target":a.target}),
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
        &json!(s.scope),
        &event_body,
    ))?;
    let authority = json!({"event_id":eid,"target":a.target,"agreement_id":s.agreement,"family_id":a.family,"source":s.source,"principal":s.operator,"grant":grant["id"],"grant_revision":s.grant_revision.to_string(),"active":true,"may_read":true,"may_submit":!correction,"may_correct":correction,"verified_evidence":[proof["id"]],"received_at":at,"accepted_at":at});
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
    let auth = row("authority-decision", &json!(s.scope), authority.clone())?;
    let admission = json!({"event_id":eid,"principal":s.operator,"credential_revision":"1","authentication":authentication["id"],"grant":grant["id"],"grant_revision":s.grant_revision.to_string(),"target_guard_revision":"1","target_state":"final_unreversed","aggregate_guard_revision":history.decisions.len().to_string(),"authorized_source":s.source,"agreement_id":s.agreement,"payer":s.customer,"book":"retail","family_id":a.family,"permission":data["type"],"target_snapshot":base.snapshot["id"],"authority_decision":auth["id"],"binding_id":s.binding,"received_at":at,"accepted_at":at,"decision":"allow","policy_snapshot":policy["id"],"basis":base.basis["id"]});
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
            count: snapshot.entries.len() as i64,
            source: s.source,
            external: a.id,
            semantic,
            ingress,
            facts,
            bundle,
        }),
    ))
}
