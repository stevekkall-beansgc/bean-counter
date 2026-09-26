use super::*;
use crate::store::sqlite::BillingSnapshot;
pub(super) struct History {
    pub records: Records,
    pub base: b::Base,
    pub decisions: Vec<c::outcomes::Decision>,
    pub source: String,
    pub agreement_id: String,
    pub agreement_version: i64,
    pub terms: Setup,
}
pub(super) struct Audit {
    pub targets: BTreeMap<String, History>,
    pub receipts: BTreeMap<i64, Value>,
    pub rows: BTreeMap<i64, Vec<Value>>,
    pub entry_terms: BTreeMap<i64, Setup>,
}
pub(super) fn load(s: &Setup, snapshot: &BillingSnapshot) -> Result<Audit> {
    let mut audit = Audit {
        targets: BTreeMap::new(),
        receipts: BTreeMap::new(),
        rows: BTreeMap::new(),
        entry_terms: BTreeMap::new(),
    };
    let mut identities = BTreeSet::new();
    let mut semantics = BTreeSet::new();
    for (i, e) in snapshot.entries.iter().enumerate() {
        let customer = e.customer.as_deref().unwrap_or(&s.customer).to_owned();
        b::check(
            e.ordinal == i as i64 + 1
                && identities.insert((customer.clone(), e.source.clone(), e.external_id.clone()))
                && semantics.insert((customer.clone(), e.source.clone(), e.semantic_key.clone())),
        )?;
        let (agreement_id, agreement_version) =
            match (e.agreement_id.as_deref(), e.agreement_version) {
                (Some(id), Some(version)) if e.customer.is_some() => (id.to_owned(), version),
                (None, None) if e.customer.is_none() => {
                    let initial = control::first_terms(snapshot, &customer, &e.source)?;
                    (initial.agreement, 1)
                }
                _ => return Err(b::integrity()),
            };
        let terms = control::terms_for_entry(
            snapshot,
            &customer,
            &e.source,
            &agreement_id,
            agreement_version,
        )?;
        let v = core(canonical::parse_bounded(&e.bundle, 8 * 1024 * 1024))?;
        b::check(bytes(&v)? == e.bundle)?;
        let rows = array(&v)?.clone();
        if rows.iter().any(|r| r["kind"] == "base-acceptance") {
            let records = decode_entry(&terms, e)?;
            if let Some(accepted_at_us) = e.accepted_at_us {
                let receipt = records.one("base-acceptance")?;
                let accepted_at = b::time(&receipt["body"]["accepted_at"])?;
                b::check(accepted_at.micros() == accepted_at_us)?;
                let selected =
                    control::selected_terms(snapshot, &customer, &e.source, &accepted_at)?
                        .ok_or_else(b::integrity)?;
                b::check(selected.0.agreement == agreement_id && selected.1 == agreement_version)?;
            }
            permissions::verify_grants(&records, snapshot, &customer, &e.source)?;
            let receipt = records.one("base-acceptance")?.clone();
            b::check(rows.len() == array(&receipt["body"]["members"])?.len() + 1)?;
            let base = b::decode_base(&records, &reference(&receipt))?;
            let target = text(&receipt["body"]["target"])?.to_owned();
            let key = target_key(&customer, &target);
            b::check(
                audit
                    .targets
                    .insert(
                        key,
                        History {
                            records,
                            base,
                            decisions: vec![],
                            source: e.source.clone(),
                            agreement_id: agreement_id.clone(),
                            agreement_version,
                            terms: terms.clone(),
                        },
                    )
                    .is_none(),
            )?;
            audit.receipts.insert(e.ordinal, receipt);
        } else {
            let event = rows
                .iter()
                .find(|r| r["kind"] == "event")
                .ok_or_else(b::integrity)?;
            let target = text(&event["body"]["data"]["target"])?;
            let key = target_key(&customer, target);
            let history = audit.targets.get_mut(&key).ok_or_else(b::integrity)?;
            b::check(history.source == e.source)?;
            let group = Records::new(
                &rows.iter().map(bytes).collect::<Result<Vec<_>>>()?,
                json!(history.terms.scope),
            )?;
            if let Some(accepted_at_us) = e.accepted_at_us {
                let receipt = group.one("receipt")?;
                b::check(
                    b::time(&receipt["body"]["accepted_at"])?.micros() == accepted_at_us
                        && history.agreement_id == agreement_id
                        && history.agreement_version == agreement_version,
                )?;
            }
            adjustment::verify_entry(&history.terms, e, &group)?;
            for r in &rows {
                history.records.insert(r.clone())?;
            }
            permissions::verify_grants(&history.records, snapshot, &customer, &e.source)?;
            let groups = super::super::retained::decision_groups(&history.records, &history.base)?;
            history.decisions =
                super::super::retained::economic::replay(&history.records, &history.base, &groups)?;
            audit
                .receipts
                .insert(e.ordinal, group.one("receipt")?.clone());
        }
        audit.rows.insert(e.ordinal, rows);
        audit.entry_terms.insert(e.ordinal, terms);
    }
    for a in &snapshot.aliases {
        let target_customer = a.customer.as_deref().unwrap_or(&s.customer);
        b::check(identities.insert((
            target_customer.to_owned(),
            a.source.clone(),
            a.external_id.clone(),
        )))?;
        let e = snapshot
            .entries
            .iter()
            .find(|e| e.ordinal == a.ordinal)
            .ok_or_else(b::integrity)?;
        let entry_customer = e.customer.as_deref().unwrap_or(&s.customer);
        let terms = audit.entry_terms.get(&e.ordinal).ok_or_else(b::integrity)?;
        b::check(a.source == e.source && target_customer == entry_customer)?;
        if audit.receipts[&e.ordinal]["kind"] == "base-acceptance" {
            let event = domain::normalize(&a.ingress, terms.scope.clone(), &terms.source)
                .map_err(|_| b::integrity())?
                .resolve(None)
                .map_err(|_| b::integrity())?;
            b::check(
                event.candidate().external_id() == a.external_id
                    && event.dto().customer == target_customer
                    && event.candidate().ingress_bytes().as_slice() == a.ingress
                    && bytes(&json!(["base", event.candidate().operation_id()]))? == e.semantic_key
                    && bytes(&event.completion_facts(&[]).map_err(|_| b::integrity())?)? == e.facts,
            )?;
        } else {
            adjustment::verify_alias(a, e, &s.customer)?;
        }
    }
    Ok(audit)
}

pub(super) fn target_key(customer: &str, target: &str) -> String {
    format!("{customer}\0{target}")
}
